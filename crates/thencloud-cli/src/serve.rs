//! `thencloud serve`: a folder over WebDAV on 127.0.0.1, for the file
//! managers that speak it (macOS Finder, Windows Explorer, GNOME Files,
//! Dolphin). Everything is decrypted here, in this
//! process; the server sees only the requests the web client would make.
//!
//! Only this machine can connect, and only with the address printed at
//! start: it carries a random secret as its first path segment, so another
//! user on the machine, or a web page making requests to localhost, gets
//! nowhere. Requests whose `Host` isn't the loopback address are refused as
//! well (DNS rebinding).
//!
//! Reads fetch and decrypt one 4 MiB chunk at a time. Writes go to an
//! unlinked temporary file and are uploaded when the request ends, as a new
//! version when the file exists. Deleting moves to the trash.

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::{Buf, Bytes};
use dav_server::body::Body;
use dav_server::davpath::DavPath;
use dav_server::fakels::FakeLs;
use dav_server::fs::{
    DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
    OpenOptions, ReadDirMeta,
};
use dav_server::{DavHandler, DavMethodSet};
use futures_util::{FutureExt, stream};
use hyper::body::Incoming;
use hyper::http::{Method, Request, Response, StatusCode, header};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use thencloud_crypto::api::AppScope;
use thencloud_crypto::{CHUNK_SIZE, Key};

use crate::{Client, Entry, Error, now_ms};

/// Listings are reused for this long, and dropped at once when this bridge
/// changes something in them.
const LIST_TTL: Duration = Duration::from_secs(5);

pub struct ServeOptions {
    pub read_only: bool,
    /// Where written files are staged before upload (unlinked right away).
    pub temp_dir: PathBuf,
}

/// A random secret for the first path segment.
pub fn new_secret() -> String {
    Key::generate().as_bytes()[..20]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A folder's children, and when they were fetched.
type Listing = (Instant, Arc<Vec<Entry>>);

struct Tree {
    client: Client,
    root: Entry,
    read_only: bool,
    temp_dir: PathBuf,
    /// Folder listings by node id.
    dirs: Mutex<HashMap<String, Listing>>,
    quota: Mutex<Option<(Instant, u64, u64)>>,
}

fn fs_error(e: &Error) -> FsError {
    match e {
        Error::Api(404, ..) => FsError::NotFound,
        Error::Api(401 | 403, ..) => FsError::Forbidden,
        Error::Api(409, code, _) if code == "name_taken" => FsError::Exists,
        Error::Api(413, ..) => FsError::TooLarge,
        Error::Api(429 | 507, ..) => FsError::InsufficientStorage,
        _ => FsError::GeneralFailure,
    }
}

impl Tree {
    fn fail(&self, e: Error) -> FsError {
        eprintln!("thencloud serve: {e}");
        fs_error(&e)
    }

    fn writable(&self) -> FsResult<()> {
        if self.read_only {
            Err(FsError::Forbidden)
        } else {
            Ok(())
        }
    }

    fn list(&self, folder: &Entry) -> FsResult<Arc<Vec<Entry>>> {
        let id = &folder.node.id;
        if let Some((at, kids)) = self.dirs.lock().unwrap().get(id)
            && at.elapsed() < LIST_TTL
        {
            return Ok(kids.clone());
        }
        let kids = Arc::new(self.client.list(folder).map_err(|e| self.fail(e))?);
        self.dirs
            .lock()
            .unwrap()
            .insert(id.clone(), (Instant::now(), kids.clone()));
        Ok(kids)
    }

    fn stale(&self, folder_id: &str) {
        self.dirs.lock().unwrap().remove(folder_id);
    }

    /// A child by name: exactly, then ignoring case (as Windows and macOS
    /// expect, and as the server's name index sees it).
    fn child(&self, folder: &Entry, name: &str) -> FsResult<Option<Entry>> {
        let kids = self.list(folder)?;
        Ok(kids
            .iter()
            .find(|k| k.meta.name == name)
            .or_else(|| {
                kids.iter()
                    .find(|k| k.meta.name.to_lowercase() == name.to_lowercase())
            })
            .cloned())
    }

    fn resolve(&self, path: &DavPath) -> FsResult<Entry> {
        let mut here = self.root.clone();
        for part in segments(path)? {
            if !here.is_folder() {
                return Err(FsError::NotFound);
            }
            here = self.child(&here, &part)?.ok_or(FsError::NotFound)?;
        }
        Ok(here)
    }

    /// The folder `path` is in, and its last segment.
    fn parent(&self, path: &DavPath) -> FsResult<(Entry, String)> {
        let mut parts = segments(path)?;
        let name = parts.pop().ok_or(FsError::Forbidden)?;
        let mut here = self.root.clone();
        for part in parts {
            here = self.child(&here, &part)?.ok_or(FsError::NotFound)?;
            if !here.is_folder() {
                return Err(FsError::NotFound);
            }
        }
        Ok((here, name))
    }

    fn quota(&self) -> (u64, Option<u64>) {
        if let Some((at, used, total)) = *self.quota.lock().unwrap()
            && at.elapsed() < Duration::from_secs(10)
        {
            return (used, Some(total));
        }
        let me = self
            .client
            .fetch_me()
            .unwrap_or_else(|_| self.client.me.clone());
        let (used, total) = (me.used_bytes.max(0) as u64, me.quota_bytes.max(0) as u64);
        *self.quota.lock().unwrap() = Some((Instant::now(), used, total));
        (used, Some(total))
    }
}

/// The decoded path segments, refusing anything that isn't UTF-8.
fn segments(path: &DavPath) -> FsResult<Vec<String>> {
    let text = std::str::from_utf8(path.as_bytes()).map_err(|_| FsError::NotFound)?;
    Ok(text
        .split('/')
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect())
}

/// Run a blocking call to the server off the async threads.
fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> FsResult<T> + Send + 'static,
) -> FsFuture<'static, T> {
    async move {
        tokio::task::spawn_blocking(f)
            .await
            .unwrap_or(Err(FsError::GeneralFailure))
    }
    .boxed()
}

#[derive(Clone)]
struct CloudDav(Arc<Tree>);

#[derive(Debug, Clone)]
struct Meta {
    size: u64,
    mtime: i64,
    folder: bool,
    /// The current version (or the node, for a folder) as an ETag.
    tag: Option<String>,
}

impl Meta {
    fn of(e: &Entry) -> Meta {
        Meta {
            size: if e.is_folder() { 0 } else { e.meta.size },
            mtime: e.meta.mtime,
            folder: e.is_folder(),
            tag: Some(match &e.node.version {
                Some(v) => v.id.clone(),
                None => format!("{}-{}", e.node.id, e.node.revision),
            }),
        }
    }
}

fn ms_time(ms: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64)
}

impl DavMetaData for Meta {
    fn len(&self) -> u64 {
        self.size
    }
    fn modified(&self) -> FsResult<SystemTime> {
        Ok(ms_time(self.mtime))
    }
    fn is_dir(&self) -> bool {
        self.folder
    }
    fn etag(&self) -> Option<String> {
        self.tag.clone()
    }
}

struct DirEntry(Entry);

impl DavDirEntry for DirEntry {
    fn name(&self) -> Vec<u8> {
        self.0.meta.name.clone().into_bytes()
    }
    fn metadata(&self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        let m: Box<dyn DavMetaData> = Box::new(Meta::of(&self.0));
        async move { Ok(m) }.boxed()
    }
}

/// A file opened for reading: decrypted a chunk at a time.
struct ReadFile {
    tree: Arc<Tree>,
    entry: Entry,
    pos: u64,
    chunk: Option<(u32, Arc<Vec<u8>>)>,
}

/// A file opened for writing: staged in a temporary file, uploaded on flush.
struct WriteFile {
    tree: Arc<Tree>,
    parent: Entry,
    name: String,
    existing: Option<Entry>,
    temp: Arc<Mutex<File>>,
    pos: u64,
    dirty: bool,
}

impl fmt::Debug for ReadFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReadFile")
    }
}

impl fmt::Debug for WriteFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WriteFile")
    }
}

fn io_error(e: io::Error) -> FsError {
    eprintln!("thencloud serve: {e}");
    FsError::GeneralFailure
}

impl DavFile for ReadFile {
    fn metadata(&mut self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        let m: Box<dyn DavMetaData> = Box::new(Meta::of(&self.entry));
        async move { Ok(m) }.boxed()
    }

    fn write_buf(&mut self, _buf: Box<dyn Buf + Send>) -> FsFuture<'_, ()> {
        async { Err(FsError::Forbidden) }.boxed()
    }

    fn write_bytes(&mut self, _buf: Bytes) -> FsFuture<'_, ()> {
        async { Err(FsError::Forbidden) }.boxed()
    }

    fn read_bytes(&mut self, count: usize) -> FsFuture<'_, Bytes> {
        async move {
            let size = self.entry.meta.size;
            if self.pos >= size || count == 0 {
                return Ok(Bytes::new());
            }
            let index = (self.pos / CHUNK_SIZE as u64) as u32;
            let plain = match &self.chunk {
                Some((i, c)) if *i == index => c.clone(),
                _ => {
                    let (tree, entry) = (self.tree.clone(), self.entry.clone());
                    let c = blocking(move || {
                        tree.client
                            .chunk(&entry, index)
                            .map(Arc::new)
                            .map_err(|e| tree.fail(e))
                    })
                    .await?;
                    self.chunk = Some((index, c.clone()));
                    c
                }
            };
            let start = (self.pos % CHUNK_SIZE as u64) as usize;
            // Past the real size is padding.
            let end = (plain.len() as u64).min(size - index as u64 * CHUNK_SIZE as u64) as usize;
            let end = end.min(start + count);
            if start >= end {
                return Ok(Bytes::new());
            }
            self.pos += (end - start) as u64;
            Ok(Bytes::copy_from_slice(&plain[start..end]))
        }
        .boxed()
    }

    fn seek(&mut self, pos: SeekFrom) -> FsFuture<'_, u64> {
        let size = self.entry.meta.size as i64;
        let to = match pos {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::Current(d) => self.pos as i64 + d,
            SeekFrom::End(d) => size + d,
        };
        async move {
            if to < 0 {
                return Err(FsError::GeneralFailure);
            }
            self.pos = to as u64;
            Ok(self.pos)
        }
        .boxed()
    }

    fn flush(&mut self) -> FsFuture<'_, ()> {
        async { Ok(()) }.boxed()
    }
}

impl WriteFile {
    fn len(&self) -> FsResult<u64> {
        let f = self.temp.lock().unwrap();
        Ok(f.metadata().map_err(io_error)?.len())
    }

    fn write(&mut self, data: &[u8]) -> FsResult<()> {
        let mut f = self.temp.lock().unwrap();
        f.seek(SeekFrom::Start(self.pos)).map_err(io_error)?;
        f.write_all(data).map_err(io_error)?;
        self.pos += data.len() as u64;
        self.dirty = true;
        Ok(())
    }
}

impl DavFile for WriteFile {
    fn metadata(&mut self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        async move {
            let m: Box<dyn DavMetaData> = Box::new(Meta {
                size: self.len()?,
                mtime: now_ms(),
                folder: false,
                tag: None,
            });
            Ok(m)
        }
        .boxed()
    }

    fn write_buf(&mut self, mut buf: Box<dyn Buf + Send>) -> FsFuture<'_, ()> {
        async move {
            while buf.has_remaining() {
                let n = buf.chunk().len();
                self.write(buf.chunk())?;
                buf.advance(n);
            }
            Ok(())
        }
        .boxed()
    }

    fn write_bytes(&mut self, buf: Bytes) -> FsFuture<'_, ()> {
        async move { self.write(&buf) }.boxed()
    }

    fn read_bytes(&mut self, count: usize) -> FsFuture<'_, Bytes> {
        async move {
            let mut f = self.temp.lock().unwrap();
            f.seek(SeekFrom::Start(self.pos)).map_err(io_error)?;
            let mut out = Vec::with_capacity(count.min(CHUNK_SIZE));
            (&mut *f)
                .take(count as u64)
                .read_to_end(&mut out)
                .map_err(io_error)?;
            self.pos += out.len() as u64;
            Ok(Bytes::from(out))
        }
        .boxed()
    }

    fn seek(&mut self, pos: SeekFrom) -> FsFuture<'_, u64> {
        async move {
            let to = match pos {
                SeekFrom::Start(p) => p as i64,
                SeekFrom::Current(d) => self.pos as i64 + d,
                SeekFrom::End(d) => self.len()? as i64 + d,
            };
            if to < 0 {
                return Err(FsError::GeneralFailure);
            }
            self.pos = to as u64;
            Ok(self.pos)
        }
        .boxed()
    }

    fn flush(&mut self) -> FsFuture<'_, ()> {
        async move {
            if !self.dirty {
                return Ok(());
            }
            let (tree, temp) = (self.tree.clone(), self.temp.clone());
            let (parent, name, existing) = (
                self.parent.clone(),
                self.name.clone(),
                self.existing.clone(),
            );
            let uploaded = blocking(move || {
                let mut f = temp.lock().unwrap();
                let size = f.metadata().map_err(io_error)?.len();
                f.seek(SeekFrom::Start(0)).map_err(io_error)?;
                let r = tree.client.upload_from(
                    &mut *f,
                    size,
                    now_ms(),
                    &parent,
                    &name,
                    existing.as_ref(),
                );
                tree.stale(&parent.node.id);
                r.map_err(|e| tree.fail(e))
            })
            .await?;
            self.existing = Some(uploaded);
            self.dirty = false;
            Ok(())
        }
        .boxed()
    }
}

impl DavFileSystem for CloudDav {
    fn open<'a>(
        &'a self,
        path: &'a DavPath,
        options: OpenOptions,
    ) -> FsFuture<'a, Box<dyn DavFile>> {
        let tree = self.0.clone();
        let path = path.clone();
        blocking(move || {
            if !options.write && !options.append {
                let entry = tree.resolve(&path)?;
                if entry.is_folder() {
                    return Err(FsError::Forbidden);
                }
                let f: Box<dyn DavFile> = Box::new(ReadFile {
                    tree: tree.clone(),
                    entry,
                    pos: 0,
                    chunk: None,
                });
                return Ok(f);
            }
            tree.writable()?;
            let (parent, name) = tree.parent(&path)?;
            let existing = tree.child(&parent, &name)?;
            match &existing {
                Some(e) if e.is_folder() => return Err(FsError::Forbidden),
                Some(_) if options.create_new => return Err(FsError::Exists),
                None if !options.create && !options.create_new => {
                    return Err(FsError::NotFound);
                }
                _ => {}
            }
            let mut temp = tempfile::tempfile_in(&tree.temp_dir).map_err(io_error)?;
            // A partial write (a range, or appending) starts from what's there.
            if let Some(e) = existing.as_ref().filter(|_| !options.truncate) {
                tree.client
                    .download(e, &mut temp)
                    .map_err(|e| tree.fail(e))?;
            }
            let pos = if options.append {
                temp.stream_position().map_err(io_error)?
            } else {
                0
            };
            // A new or emptied file is uploaded even if nothing is written.
            let dirty = existing.is_none() || options.truncate;
            let name = existing.as_ref().map_or(name, |e| e.meta.name.clone());
            let f: Box<dyn DavFile> = Box::new(WriteFile {
                tree: tree.clone(),
                parent,
                name,
                existing,
                temp: Arc::new(Mutex::new(temp)),
                pos,
                dirty,
            });
            Ok(f)
        })
    }

    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        _meta: ReadDirMeta,
    ) -> FsFuture<'a, FsStream<Box<dyn DavDirEntry>>> {
        let tree = self.0.clone();
        let path = path.clone();
        blocking(move || {
            let folder = tree.resolve(&path)?;
            if !folder.is_folder() {
                return Err(FsError::Forbidden);
            }
            let kids: Vec<FsResult<Box<dyn DavDirEntry>>> = tree
                .list(&folder)?
                .iter()
                // A name with a slash in it can't be addressed by a path.
                .filter(|e| !e.meta.name.contains('/'))
                .map(|e| Ok(Box::new(DirEntry(e.clone())) as Box<dyn DavDirEntry>))
                .collect();
            let s: FsStream<Box<dyn DavDirEntry>> = Box::pin(stream::iter(kids));
            Ok(s)
        })
    }

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, Box<dyn DavMetaData>> {
        let tree = self.0.clone();
        let path = path.clone();
        blocking(move || {
            let m: Box<dyn DavMetaData> = Box::new(Meta::of(&tree.resolve(&path)?));
            Ok(m)
        })
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        let tree = self.0.clone();
        let path = path.clone();
        blocking(move || {
            tree.writable()?;
            let (parent, name) = tree.parent(&path)?;
            if tree.child(&parent, &name)?.is_some() {
                return Err(FsError::Exists);
            }
            let r = tree.client.mkdir(&parent, &name);
            tree.stale(&parent.node.id);
            r.map(|_| ()).map_err(|e| tree.fail(e))
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        self.remove(path)
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        self.remove(path)
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<'a, ()> {
        let tree = self.0.clone();
        let (from, to) = (from.clone(), to.clone());
        blocking(move || {
            tree.writable()?;
            let e = tree.resolve(&from)?;
            if e.node.id == tree.root.node.id {
                return Err(FsError::Forbidden);
            }
            let (parent, name) = tree.parent(&to)?;
            let mut meta = e.meta.clone();
            meta.name = name;
            let r = tree.client.update(&e, &parent, meta);
            tree.stale(&parent.node.id);
            if let Some(old) = &e.node.parent_id {
                tree.stale(old);
            }
            r.map(|_| ()).map_err(|e| tree.fail(e))
        })
    }

    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<'a, ()> {
        let tree = self.0.clone();
        let (from, to) = (from.clone(), to.clone());
        blocking(move || {
            tree.writable()?;
            let e = tree.resolve(&from)?;
            if e.is_folder() {
                return Err(FsError::Forbidden);
            }
            let (parent, name) = tree.parent(&to)?;
            let existing = tree.child(&parent, &name)?;
            if existing.as_ref().is_some_and(|x| x.is_folder()) {
                return Err(FsError::Forbidden);
            }
            let mut temp = tempfile::tempfile_in(&tree.temp_dir).map_err(io_error)?;
            let size = tree
                .client
                .download(&e, &mut temp)
                .map_err(|e| tree.fail(e))?;
            temp.seek(SeekFrom::Start(0)).map_err(io_error)?;
            let r = tree.client.upload_from(
                &mut temp,
                size,
                e.meta.mtime,
                &parent,
                &name,
                existing.as_ref(),
            );
            tree.stale(&parent.node.id);
            r.map(|_| ()).map_err(|e| tree.fail(e))
        })
    }

    fn set_modified<'a>(&'a self, path: &'a DavPath, tm: SystemTime) -> FsFuture<'a, ()> {
        let tree = self.0.clone();
        let path = path.clone();
        blocking(move || {
            tree.writable()?;
            let e = tree.resolve(&path)?;
            let (parent, _) = tree.parent(&path)?;
            let mut meta = e.meta.clone();
            meta.mtime = tm
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64);
            let r = tree.client.update(&e, &parent, meta);
            tree.stale(&parent.node.id);
            r.map(|_| ()).map_err(|e| tree.fail(e))
        })
    }

    fn get_quota(&self) -> FsFuture<'_, (u64, Option<u64>)> {
        let tree = self.0.clone();
        blocking(move || Ok(tree.quota()))
    }
}

impl CloudDav {
    /// Delete moves to the trash, a folder with everything in it.
    fn remove<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        let tree = self.0.clone();
        let path = path.clone();
        blocking(move || {
            tree.writable()?;
            let e = tree.resolve(&path)?;
            if e.node.id == tree.root.node.id {
                return Err(FsError::Forbidden);
            }
            let r = tree.client.trash(&e);
            if let Some(p) = &e.node.parent_id {
                tree.stale(p);
            }
            r.map_err(|e| tree.fail(e))
        })
    }
}

/// A running bridge: its address, and the secret its paths start with.
pub struct Bridge {
    pub addr: SocketAddr,
    pub secret: String,
}

impl Bridge {
    pub fn url(&self) -> String {
        format!("http://{}/{}/", self.addr, self.secret)
    }
}

fn plain(status: StatusCode) -> Response<Body> {
    let mut res = Response::new(Body::from(status.canonical_reason().unwrap_or("")));
    *res.status_mut() = status;
    res
}

/// The `Host` header must name this listener on the loopback address;
/// anything else is a page on some other name reaching us by DNS rebinding.
fn host_ok(req: &Request<Incoming>, port: u16) -> bool {
    let Some(host) = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
    else {
        return false;
    };
    let allowed = [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    allowed.iter().any(|a| host.eq_ignore_ascii_case(a))
}

/// A folder is moved to the trash whole, as one item. (Left to the WebDAV
/// handler, it would delete everything in it one by one first.) `None` for
/// anything that isn't a folder.
async fn delete_folder(
    tree: &Arc<Tree>,
    prefix: &str,
    uri: &hyper::http::Uri,
) -> Option<Response<Body>> {
    let mut path = DavPath::new(uri.path()).ok()?;
    path.set_prefix(prefix.trim_end_matches('/')).ok()?;
    let tree = tree.clone();
    let r = blocking(move || {
        let e = tree.resolve(&path)?;
        if !e.is_folder() {
            return Ok(false);
        }
        tree.writable()?;
        if e.node.id == tree.root.node.id {
            return Err(FsError::Forbidden);
        }
        let r = tree.client.trash(&e);
        if let Some(p) = &e.node.parent_id {
            tree.stale(p);
        }
        r.map(|_| true).map_err(|e| tree.fail(e))
    })
    .await;
    match r {
        Ok(false) => None,
        Ok(true) => Some(plain(StatusCode::NO_CONTENT)),
        Err(FsError::NotFound) => Some(plain(StatusCode::NOT_FOUND)),
        Err(FsError::Forbidden) => Some(plain(StatusCode::FORBIDDEN)),
        Err(_) => Some(plain(StatusCode::INTERNAL_SERVER_ERROR)),
    }
}

async fn handle(
    dav: DavHandler,
    tree: Arc<Tree>,
    prefix: Arc<str>,
    port: u16,
    req: Request<Incoming>,
) -> Response<Body> {
    if !host_ok(&req, port) {
        return plain(StatusCode::FORBIDDEN);
    }
    let path = req.uri().path();
    let inside = path == &prefix[..prefix.len() - 1] || path.starts_with(&*prefix);
    if !inside {
        // Windows asks the server's root what it supports before mounting a
        // folder under it; say that and nothing else.
        if req.method() == Method::OPTIONS {
            let mut res = plain(StatusCode::OK);
            res.headers_mut()
                .insert("DAV", header::HeaderValue::from_static("1, 2"));
            res.headers_mut()
                .insert("MS-Author-Via", header::HeaderValue::from_static("DAV"));
            return res;
        }
        return plain(StatusCode::NOT_FOUND);
    }
    if req.method() == Method::DELETE
        && let Some(res) = delete_folder(&tree, &prefix, req.uri()).await
    {
        return res;
    }
    let res = dav.handle(req).await;
    // Other clients may have changed things; a request that failed on a
    // stale listing shouldn't fail again for the next five seconds.
    if res.status() == StatusCode::NOT_FOUND || res.status() == StatusCode::CONFLICT {
        tree.dirs.lock().unwrap().clear();
    }
    res
}

/// Serve `root` over WebDAV on `listener` until the process ends.
pub fn serve(
    client: Client,
    root: Entry,
    listener: TcpListener,
    secret: String,
    opts: ServeOptions,
) -> io::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .build()?;
    rt.block_on(run(client, root, listener, secret, opts))
}

/// Start serving on a background thread; for tests.
pub fn spawn(
    client: Client,
    root: Entry,
    listener: TcpListener,
    opts: ServeOptions,
) -> io::Result<Bridge> {
    let bridge = Bridge {
        addr: listener.local_addr()?,
        secret: new_secret(),
    };
    let secret = bridge.secret.clone();
    std::thread::spawn(move || serve(client, root, listener, secret, opts));
    Ok(bridge)
}

async fn run(
    client: Client,
    root: Entry,
    listener: TcpListener,
    secret: String,
    opts: ServeOptions,
) -> io::Result<()> {
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let read_only = opts.read_only || client.scope == AppScope::Read;
    let tree = Arc::new(Tree {
        client,
        root,
        read_only,
        temp_dir: opts.temp_dir,
        dirs: Mutex::new(HashMap::new()),
        quota: Mutex::new(None),
    });
    let prefix: Arc<str> = format!("/{secret}/").into();
    let mut builder = DavHandler::builder()
        .filesystem(Box::new(CloudDav(tree.clone())))
        .locksystem(FakeLs::new())
        .strip_prefix(format!("/{secret}"));
    if read_only {
        builder = builder.methods(DavMethodSet::WEBDAV_RO);
    }
    let dav = builder.build_handler();
    loop {
        let (stream, _) = listener.accept().await?;
        let (dav, tree, prefix) = (dav.clone(), tree.clone(), prefix.clone());
        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let (dav, tree, prefix) = (dav.clone(), tree.clone(), prefix.clone());
                async move { Ok::<_, Infallible>(handle(dav, tree, prefix, port, req).await) }
            });
            let _ = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}
