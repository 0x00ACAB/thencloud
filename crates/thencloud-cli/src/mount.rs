//! `thencloud mount`: a folder as a Linux filesystem, through FUSE. Like the
//! rest of the client, everything is decrypted here and the server sees
//! only the requests the web client would make.
//!
//! Reads fetch and decrypt one 4 MiB chunk at a time (a few are kept in
//! memory). Writes go to an unlinked temporary file and are uploaded as a
//! new version when the file is closed or synced.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fuser::{
    BackgroundSession, Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags,
    Generation, INodeNo, InitFlags, KernelConfig, LockOwner, MountOption, OpenAccMode, OpenFlags,
    RenameFlags, ReplyAttr, ReplyCreate, ReplyData, ReplyDirectory, ReplyEmpty, ReplyEntry,
    ReplyOpen, ReplyStatfs, ReplyWrite, Request, Session, SessionACL, TimeOrNow, WriteFlags,
};
use thencloud_crypto::api::{AppScope, Node};
use thencloud_crypto::{CHUNK_SIZE, Key, Metadata};

use crate::{Client, Entry, Error, now_ms};

/// How long the kernel may cache names and attributes.
const TTL: Duration = Duration::from_secs(1);
/// How long a folder listing is reused before asking the server again.
const LIST_TTL: Duration = Duration::from_secs(5);
const CACHED_CHUNKS: usize = 16;
/// Renaming a file over another keeps the target's history by uploading the
/// content as a new version of it, up to this size (editors save this way).
const REPLACE_MAX: u64 = 64 * 1024 * 1024;
const ROOT: u64 = 1;

pub struct MountOptions {
    pub read_only: bool,
    pub allow_other: bool,
    /// Where written files are staged before upload (unlinked right away).
    pub temp_dir: PathBuf,
}

struct Inode {
    parent: u64,
    meta: Metadata,
    folder: bool,
    /// The node on the server and its key; `None` for a new file that
    /// hasn't been uploaded yet.
    remote: Option<(Node, Key)>,
    /// Removed while still open: never uploaded again.
    unlinked: bool,
}

impl Inode {
    fn entry(&self) -> Option<Entry> {
        self.remote.as_ref().map(|(node, key)| Entry {
            node: node.clone(),
            key: key.clone(),
            meta: self.meta.clone(),
        })
    }
}

/// Local contents of a file being written.
struct Buffer {
    file: Arc<File>,
    dirty: bool,
}

#[derive(Default)]
struct State {
    inodes: HashMap<u64, Inode>,
    by_id: HashMap<String, u64>,
    dirs: HashMap<u64, (Instant, Vec<u64>)>,
    buffers: HashMap<u64, Buffer>,
    /// Open file handles to their inode.
    handles: HashMap<u64, u64>,
    /// Nodes this mount created, safe to delete for good when replaced.
    created: HashSet<String>,
    next_ino: u64,
    next_fh: u64,
    quota: Option<(Instant, i64, i64)>,
}

impl State {
    fn inode(&self, ino: u64) -> Result<&Inode, Errno> {
        self.inodes.get(&ino).ok_or(Errno::ENOENT)
    }

    fn inode_mut(&mut self, ino: u64) -> Result<&mut Inode, Errno> {
        self.inodes.get_mut(&ino).ok_or(Errno::ENOENT)
    }

    fn remote(&self, ino: u64) -> Result<Entry, Errno> {
        self.inode(ino)?.entry().ok_or(Errno::ENOENT)
    }

    fn add(&mut self, inode: Inode) -> u64 {
        self.next_ino += 1;
        if let Some((node, _)) = &inode.remote {
            self.by_id.insert(node.id.clone(), self.next_ino);
        }
        self.inodes.insert(self.next_ino, inode);
        self.next_ino
    }

    /// Record a listed entry, reusing its inode number if it has one.
    fn upsert(&mut self, parent: u64, e: Entry) -> u64 {
        if let Some(&ino) = self.by_id.get(&e.node.id)
            && let Some(i) = self.inodes.get_mut(&ino)
        {
            i.parent = parent;
            i.folder = e.is_folder();
            // Keep local changes that aren't uploaded yet.
            if !self.buffers.get(&ino).is_some_and(|b| b.dirty) {
                i.meta = e.meta;
                i.remote = Some((e.node, e.key));
            }
            return ino;
        }
        self.add(Inode {
            parent,
            folder: e.is_folder(),
            meta: e.meta,
            remote: Some((e.node, e.key)),
            unlinked: false,
        })
    }

    fn detach(&mut self, ino: u64) {
        if let Some(i) = self.inodes.get(&ino)
            && let Some((_, kids)) = self.dirs.get_mut(&i.parent)
        {
            kids.retain(|k| *k != ino);
        }
    }

    fn attach(&mut self, ino: u64, parent: u64) {
        if let Some((_, kids)) = self.dirs.get_mut(&parent) {
            kids.push(ino);
        }
    }

    /// Ask the server again on the next listing.
    fn stale(&mut self, dir: u64) {
        if let Some(d) = self.dirs.get_mut(&dir) {
            d.0 = d.0.checked_sub(LIST_TTL).unwrap_or(d.0);
        }
    }

    fn open_count(&self, ino: u64) -> usize {
        self.handles.values().filter(|&&i| i == ino).count()
    }
}

/// (version id, index, plaintext)
type CachedChunk = (String, u32, Arc<Vec<u8>>);

pub struct CloudFs {
    client: Client,
    st: Mutex<State>,
    chunks: Mutex<VecDeque<CachedChunk>>,
    opts: MountOptions,
    uid: u32,
    gid: u32,
}

fn errno(e: &Error) -> Errno {
    match e {
        Error::Api(404, ..) => Errno::ENOENT,
        Error::Api(401 | 403, ..) => Errno::EACCES,
        Error::Api(409, code, _) if code == "name_taken" => Errno::EEXIST,
        Error::Api(507, ..) => Errno::ENOSPC,
        _ => Errno::EIO,
    }
}

fn ms_time(ms: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64)
}

fn time_ms(t: TimeOrNow) -> i64 {
    match t {
        TimeOrNow::Now => now_ms(),
        TimeOrNow::SpecificTime(t) => t
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64),
    }
}

fn read_full_at(f: &File, buf: &mut [u8], offset: u64) -> io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match f.read_at(&mut buf[got..], offset + got as u64)? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

/// A reader over a shared file that keeps its own position.
struct At<'a>(&'a File, u64);

impl Read for At<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.0.read_at(buf, self.1)?;
        self.1 += n as u64;
        Ok(n)
    }
}

/// "notes (conflicted copy 2).md"
fn conflict_name(name: &str, n: u32) -> String {
    let tag = if n == 1 {
        "conflicted copy".to_string()
    } else {
        format!("conflicted copy {n}")
    };
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({tag}).{ext}"),
        _ => format!("{name} ({tag})"),
    }
}

impl CloudFs {
    pub fn new(client: Client, root: Entry, mut opts: MountOptions) -> CloudFs {
        opts.read_only |= client.scope == AppScope::Read;
        let mut st = State {
            next_ino: ROOT,
            next_fh: 0,
            ..Default::default()
        };
        st.by_id.insert(root.node.id.clone(), ROOT);
        st.inodes.insert(
            ROOT,
            Inode {
                parent: ROOT,
                folder: true,
                meta: root.meta,
                remote: Some((root.node, root.key)),
                unlinked: false,
            },
        );
        CloudFs {
            client,
            st: Mutex::new(st),
            chunks: Mutex::new(VecDeque::new()),
            opts,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fail(&self, e: Error) -> Errno {
        if !matches!(e, Error::Api(404 | 409, ..)) {
            eprintln!("thencloud: {e}");
        }
        errno(&e)
    }

    fn writable(&self) -> Result<(), Errno> {
        if self.opts.read_only {
            Err(Errno::EROFS)
        } else {
            Ok(())
        }
    }

    fn attr(&self, st: &State, ino: u64) -> Result<FileAttr, Errno> {
        let i = st.inode(ino)?;
        let size = match st.buffers.get(&ino) {
            Some(b) => b.file.metadata().map_err(|_| Errno::EIO)?.len(),
            None => i.meta.size,
        };
        let mtime = ms_time(i.meta.mtime);
        let (ctime, crtime) = i.remote.as_ref().map_or((mtime, mtime), |(n, _)| {
            (
                UNIX_EPOCH + Duration::from_secs(n.updated_at.max(0) as u64),
                UNIX_EPOCH + Duration::from_secs(n.created_at.max(0) as u64),
            )
        });
        let ro = self.opts.read_only;
        Ok(FileAttr {
            ino: INodeNo(ino),
            size,
            blocks: size.div_ceil(512),
            atime: mtime,
            mtime,
            ctime,
            crtime,
            kind: if i.folder {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            perm: match (i.folder, ro) {
                (true, false) => 0o755,
                (true, true) => 0o555,
                (false, false) => 0o644,
                (false, true) => 0o444,
            },
            nlink: if i.folder { 2 } else { 1 },
            uid: self.uid,
            gid: self.gid,
            rdev: 0,
            blksize: 128 * 1024,
            flags: 0,
        })
    }

    /// A folder's children, from the server if the cached listing is stale.
    fn children(&self, ino: u64, fresh: bool) -> Result<Vec<u64>, Errno> {
        let folder = {
            let st = self.lock();
            if let Some((t, kids)) = st.dirs.get(&ino)
                && !fresh
                && t.elapsed() < LIST_TTL
            {
                return Ok(kids.clone());
            }
            let i = st.inode(ino)?;
            if !i.folder {
                return Err(Errno::ENOTDIR);
            }
            i.entry().ok_or(Errno::ENOENT)?
        };
        let list = self.client.list(&folder).map_err(|e| self.fail(e))?;
        let mut st = self.lock();
        let mut kids: Vec<u64> = list.into_iter().map(|e| st.upsert(ino, e)).collect();
        // New files that aren't on the server yet.
        if let Some((_, old)) = st.dirs.get(&ino) {
            kids.extend(old.iter().filter(|k| {
                st.inodes
                    .get(k)
                    .is_some_and(|i| i.remote.is_none() && !i.unlinked)
            }));
        }
        st.dirs.insert(ino, (Instant::now(), kids.clone()));
        Ok(kids)
    }

    fn find(&self, parent: u64, name: &str) -> Result<Option<u64>, Errno> {
        let kids = self.children(parent, false)?;
        let st = self.lock();
        Ok(kids
            .into_iter()
            .find(|k| st.inodes.get(k).is_some_and(|i| i.meta.name == name)))
    }

    /// The server keeps names unique ignoring case.
    fn clash(&self, parent: u64, name: &str) -> Result<Option<u64>, Errno> {
        let kids = self.children(parent, false)?;
        let st = self.lock();
        let lower = name.to_lowercase();
        Ok(kids.into_iter().find(|k| {
            st.inodes
                .get(k)
                .is_some_and(|i| i.meta.name.to_lowercase() == lower)
        }))
    }

    fn name(name: &OsStr) -> Result<&str, Errno> {
        let n = name.to_str().ok_or(Errno::EINVAL)?;
        if n.len() > 255 {
            Err(Errno::ENAMETOOLONG)
        } else {
            Ok(n)
        }
    }

    fn chunk(&self, e: &Entry, index: u32) -> Result<Arc<Vec<u8>>, Errno> {
        let vid = e
            .node
            .version
            .as_ref()
            .map(|v| v.id.clone())
            .ok_or(Errno::EIO)?;
        if let Some((_, _, c)) = self
            .chunks
            .lock()
            .unwrap()
            .iter()
            .find(|(v, i, _)| *v == vid && *i == index)
        {
            return Ok(c.clone());
        }
        let data = Arc::new(self.client.chunk(e, index).map_err(|e| self.fail(e))?);
        let mut cache = self.chunks.lock().unwrap();
        if cache.len() >= CACHED_CHUNKS {
            cache.pop_front();
        }
        cache.push_back((vid, index, data.clone()));
        Ok(data)
    }

    fn read_remote(&self, e: &Entry, offset: u64, size: u32) -> Result<Vec<u8>, Errno> {
        let end = (offset + size as u64).min(e.meta.size);
        let mut out = Vec::with_capacity(end.saturating_sub(offset) as usize);
        let mut pos = offset;
        while pos < end {
            let idx = pos / CHUNK_SIZE as u64;
            let chunk = self.chunk(e, idx as u32)?;
            let start = (pos - idx * CHUNK_SIZE as u64) as usize;
            let take = ((end - pos) as usize).min(chunk.len().saturating_sub(start));
            if take == 0 {
                return Err(Errno::EIO);
            }
            out.extend_from_slice(&chunk[start..start + take]);
            pos += take as u64;
        }
        Ok(out)
    }

    /// Make sure `ino` has a local copy to write to, with its current
    /// contents unless it's about to be truncated.
    fn buffer(&self, ino: u64, truncate: bool) -> Result<Arc<File>, Errno> {
        let remote = {
            let st = self.lock();
            if let Some(b) = st.buffers.get(&ino) {
                return Ok(b.file.clone());
            }
            let i = st.inode(ino)?;
            if i.folder {
                return Err(Errno::EISDIR);
            }
            i.entry().filter(|e| !truncate && e.meta.size > 0)
        };
        let mut file =
            tempfile::tempfile_in(&self.opts.temp_dir).map_err(|e| self.fail(e.into()))?;
        if let Some(e) = &remote {
            self.client
                .download(e, &mut file)
                .map_err(|e| self.fail(e))?;
        }
        let mut st = self.lock();
        let b = st.buffers.entry(ino).or_insert(Buffer {
            file: Arc::new(file),
            dirty: truncate,
        });
        Ok(b.file.clone())
    }

    /// Upload a file's local changes, if it has any.
    fn upload(&self, ino: u64) -> Result<(), Errno> {
        let (file, meta, parent, existing) = {
            let mut st = self.lock();
            let Some(b) = st.buffers.get_mut(&ino).filter(|b| b.dirty) else {
                return Ok(());
            };
            b.dirty = false;
            let file = b.file.clone();
            let i = st.inode(ino)?;
            if i.unlinked {
                return Ok(());
            }
            (file, i.meta.clone(), st.remote(i.parent)?, i.entry())
        };
        let size = file.metadata().map_err(|e| self.fail(e.into()))?.len();
        let send = |name: &str, existing: Option<&Entry>| {
            self.client
                .upload_from(&mut At(&file, 0), size, meta.mtime, &parent, name, existing)
        };
        let mut res = send(&meta.name, existing.as_ref());
        // Changed elsewhere meanwhile, or the name was taken: keep both.
        let mut n = 0;
        while let Err(Error::Api(409, code, _)) = &res
            && (code == "conflict" || code == "name_taken")
            && n < 20
        {
            n += 1;
            res = send(&conflict_name(&meta.name, n), None);
        }
        let mut st = self.lock();
        match res {
            Ok(e) if n == 0 => {
                st.created
                    .extend(existing.is_none().then(|| e.node.id.clone()));
                st.by_id.insert(e.node.id.clone(), ino);
                let i = st.inode_mut(ino)?;
                i.meta = e.meta;
                i.remote = Some((e.node, e.key));
                Ok(())
            }
            Ok(e) => {
                eprintln!(
                    "thencloud: {} changed elsewhere; saved yours as {}",
                    meta.name, e.meta.name
                );
                st.created.insert(e.node.id.clone());
                let parent = st.inode(ino)?.parent;
                let copy = st.upsert(parent, e);
                st.attach(copy, parent);
                st.stale(parent);
                // The original shows the server's version again.
                st.buffers.remove(&ino);
                if existing.is_none() {
                    st.detach(ino);
                    st.inode_mut(ino)?.unlinked = true;
                }
                Ok(())
            }
            Err(e) => {
                if let Some(b) = st.buffers.get_mut(&ino) {
                    b.dirty = true;
                }
                drop(st);
                Err(self.fail(e))
            }
        }
    }

    fn remove(&self, parent: u64, name: &str, dir: bool) -> Result<(), Errno> {
        self.writable()?;
        let ino = self.find(parent, name)?.ok_or(Errno::ENOENT)?;
        let folder = self.lock().inode(ino)?.folder;
        match (folder, dir) {
            (true, false) => return Err(Errno::EISDIR),
            (false, true) => return Err(Errno::ENOTDIR),
            (true, true) if !self.children(ino, true)?.is_empty() => return Err(Errno::ENOTEMPTY),
            _ => {}
        }
        self.drop_node(ino)
    }

    /// Move to the trash (or forget a file that was never uploaded).
    fn drop_node(&self, ino: u64) -> Result<(), Errno> {
        let entry = self.lock().inode(ino)?.entry();
        if let Some(e) = &entry {
            self.client.trash(e).map_err(|e| self.fail(e))?;
        }
        let mut st = self.lock();
        st.detach(ino);
        st.buffers.remove(&ino);
        if let Some(e) = entry {
            st.by_id.remove(&e.node.id);
        }
        let i = st.inode_mut(ino)?;
        i.unlinked = true;
        i.remote = None;
        Ok(())
    }

    fn rename_node(
        &self,
        parent: u64,
        name: &str,
        newparent: u64,
        newname: &str,
        noreplace: bool,
    ) -> Result<(), Errno> {
        self.writable()?;
        let src = self.find(parent, name)?.ok_or(Errno::ENOENT)?;
        if parent == newparent && name == newname {
            return Ok(());
        }
        {
            // Not into itself.
            let st = self.lock();
            let mut at = newparent;
            loop {
                if at == src {
                    return Err(Errno::EINVAL);
                }
                if at == ROOT {
                    break;
                }
                at = st.inode(at)?.parent;
            }
        }
        if let Some(t) = self.clash(newparent, newname)?.filter(|t| *t != src) {
            let (src_folder, src_size) = {
                let st = self.lock();
                let i = st.inode(src)?;
                (i.folder, self.attr(&st, src)?.size)
            };
            let (t_folder, t_name) = {
                let st = self.lock();
                let i = st.inode(t)?;
                (i.folder, i.meta.name.clone())
            };
            if noreplace || t_name != newname {
                return Err(Errno::EEXIST);
            }
            match (src_folder, t_folder) {
                (true, false) => return Err(Errno::ENOTDIR),
                (false, true) => return Err(Errno::EISDIR),
                (true, true) if !self.children(t, true)?.is_empty() => {
                    return Err(Errno::ENOTEMPTY);
                }
                (false, false)
                    if src_size <= REPLACE_MAX && self.lock().inode(t)?.remote.is_some() =>
                {
                    return self.replace(src, t);
                }
                _ => {}
            }
            self.drop_node(t)?;
        }
        let (entry, target) = {
            let st = self.lock();
            (st.inode(src)?.entry(), st.remote(newparent)?)
        };
        if let Some(e) = entry {
            let meta = Metadata {
                name: newname.into(),
                ..e.meta.clone()
            };
            let moved = self
                .client
                .update(&e, &target, meta)
                .map_err(|e| self.fail(e))?;
            let mut st = self.lock();
            let i = st.inode_mut(src)?;
            i.meta = moved.meta;
            i.remote = Some((moved.node, moved.key));
        } else {
            self.lock().inode_mut(src)?.meta.name = newname.into();
        }
        let mut st = self.lock();
        st.detach(src);
        st.inode_mut(src)?.parent = newparent;
        st.attach(src, newparent);
        Ok(())
    }

    /// Rename file `src` over file `dst` by making its content a new version
    /// of `dst`, so `dst` keeps its history and shares.
    fn replace(&self, src: u64, dst: u64) -> Result<(), Errno> {
        let file = self.buffer(src, false)?;
        let (source, target, parent, mtime, fresh) = {
            let st = self.lock();
            let s = st.inode(src)?;
            let d = st.inode(dst)?;
            let source = s.entry();
            let fresh = source
                .as_ref()
                .is_some_and(|e| st.created.contains(&e.node.id));
            (
                source,
                d.entry().ok_or(Errno::ENOENT)?,
                st.remote(d.parent)?,
                s.meta.mtime,
                fresh,
            )
        };
        let size = file.metadata().map_err(|e| self.fail(e.into()))?.len();
        let updated = self
            .client
            .upload_from(
                &mut At(&file, 0),
                size,
                mtime,
                &parent,
                &target.meta.name,
                Some(&target),
            )
            .map_err(|e| self.fail(e))?;
        if let Some(s) = &source {
            self.client.trash(s).map_err(|e| self.fail(e))?;
            // A temporary file this mount made has no history worth keeping.
            if fresh {
                let _ = self.client.purge(&s.node.id);
            }
        }
        let mut st = self.lock();
        if let Some(s) = &source {
            st.by_id.remove(&s.node.id);
        }
        st.detach(src);
        st.detach(dst);
        st.buffers.remove(&src);
        st.buffers.remove(&dst);
        let d = st.inode_mut(dst)?;
        d.unlinked = true;
        d.remote = None;
        let newparent = d.parent;
        st.by_id.insert(updated.node.id.clone(), src);
        let s = st.inode_mut(src)?;
        s.parent = newparent;
        s.meta = updated.meta;
        s.remote = Some((updated.node, updated.key));
        st.attach(src, newparent);
        Ok(())
    }

    fn new_entry(&self, st: &State, ino: u64) -> Result<(FileAttr, Generation), Errno> {
        Ok((self.attr(st, ino)?, Generation(0)))
    }
}

impl Filesystem for CloudFs {
    fn init(&mut self, _req: &Request, config: &mut KernelConfig) -> io::Result<()> {
        // Opening with O_TRUNC comes as one request, not a truncate first,
        // which would upload an empty version.
        let _ = config.add_capabilities(InitFlags::FUSE_ATOMIC_O_TRUNC);
        Ok(())
    }

    fn destroy(&mut self) {
        let dirty: Vec<u64> = self
            .lock()
            .buffers
            .iter()
            .filter(|(_, b)| b.dirty)
            .map(|(i, _)| *i)
            .collect();
        for ino in dirty {
            let _ = self.upload(ino);
        }
        let _ = self.client.logout();
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let res = Self::name(name)
            .and_then(|n| self.find(parent.0, n))
            .and_then(|ino| {
                let st = self.lock();
                self.new_entry(&st, ino.ok_or(Errno::ENOENT)?)
            });
        match res {
            Ok((attr, g)) => reply.entry(&TTL, &attr, g),
            Err(e) => reply.error(e),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let st = self.lock();
        match self.attr(&st, ino.0) {
            Ok(a) => reply.attr(&TTL, &a),
            Err(e) => reply.error(e),
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<fuser::BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let ino = ino.0;
        // Modes and owners aren't stored; accept them so `cp -p` works.
        let res = (|| {
            if size.is_none() && mtime.is_none() {
                return Ok(());
            }
            self.writable()?;
            if let Some(size) = size {
                let file = self.buffer(ino, size == 0)?;
                file.set_len(size).map_err(|e| self.fail(e.into()))?;
                let mut st = self.lock();
                if let Some(b) = st.buffers.get_mut(&ino) {
                    b.dirty = true;
                }
                st.inode_mut(ino)?.meta.mtime = now_ms();
            }
            if let Some(t) = mtime {
                let (entry, parent, buffered) = {
                    let mut st = self.lock();
                    let buffered = st.buffers.get(&ino).is_some_and(|b| b.dirty);
                    let i = st.inode_mut(ino)?;
                    i.meta.mtime = time_ms(t);
                    let parent = i.parent;
                    (i.entry(), st.remote(parent), buffered)
                };
                // Otherwise it goes up with the next upload.
                if let (Some(e), Ok(p), false) = (entry, parent, buffered) {
                    let e = self
                        .client
                        .update(&e, &p, e.meta.clone())
                        .map_err(|e| self.fail(e))?;
                    self.lock().inode_mut(ino)?.remote = Some((e.node, e.key));
                }
            }
            if fh.is_none() && self.lock().open_count(ino) == 0 {
                self.upload(ino)?;
                self.lock().buffers.remove(&ino);
            }
            Ok(())
        })();
        let st = self.lock();
        match res.and_then(|_| self.attr(&st, ino)) {
            Ok(a) => reply.attr(&TTL, &a),
            Err(e) => reply.error(e),
        }
    }

    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let res = (|| {
            self.writable()?;
            let name = Self::name(name)?;
            if self.clash(parent.0, name)?.is_some() {
                return Err(Errno::EEXIST);
            }
            let p = self.lock().remote(parent.0)?;
            let e = self.client.mkdir(&p, name).map_err(|e| self.fail(e))?;
            let mut st = self.lock();
            let ino = st.upsert(parent.0, e);
            st.attach(ino, parent.0);
            self.new_entry(&st, ino)
        })();
        match res {
            Ok((attr, g)) => reply.entry(&TTL, &attr, g),
            Err(e) => reply.error(e),
        }
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        match Self::name(name).and_then(|n| self.remove(parent.0, n, false)) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        match Self::name(name).and_then(|n| self.remove(parent.0, n, true)) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn rename(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        newparent: INodeNo,
        newname: &OsStr,
        flags: RenameFlags,
        reply: ReplyEmpty,
    ) {
        let res = (|| {
            if flags.contains(RenameFlags::RENAME_EXCHANGE) {
                return Err(Errno::EINVAL);
            }
            let noreplace = flags.contains(RenameFlags::RENAME_NOREPLACE);
            self.rename_node(
                parent.0,
                Self::name(name)?,
                newparent.0,
                Self::name(newname)?,
                noreplace,
            )
        })();
        match res {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        let res = (|| {
            let write = flags.acc_mode() != OpenAccMode::O_RDONLY;
            if write {
                self.writable()?;
            }
            if self.lock().inode(ino.0)?.folder {
                return Err(Errno::EISDIR);
            }
            if write && flags.0 & libc::O_TRUNC != 0 {
                self.buffer(ino.0, true)?
                    .set_len(0)
                    .map_err(|e| self.fail(e.into()))?;
                let mut st = self.lock();
                if let Some(b) = st.buffers.get_mut(&ino.0) {
                    b.dirty = true;
                }
                st.inode_mut(ino.0)?.meta.mtime = now_ms();
            }
            let mut st = self.lock();
            st.next_fh += 1;
            let fh = st.next_fh;
            st.handles.insert(fh, ino.0);
            Ok(fh)
        })();
        match res {
            Ok(fh) => reply.opened(FileHandle(fh), FopenFlags::empty()),
            Err(e) => reply.error(e),
        }
    }

    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let res = (|| {
            self.writable()?;
            let name = Self::name(name)?;
            if self.clash(parent.0, name)?.is_some() {
                return Err(Errno::EEXIST);
            }
            let file =
                tempfile::tempfile_in(&self.opts.temp_dir).map_err(|e| self.fail(e.into()))?;
            let mut st = self.lock();
            if !st.inode(parent.0)?.folder {
                return Err(Errno::ENOTDIR);
            }
            let ino = st.add(Inode {
                parent: parent.0,
                meta: Metadata {
                    name: name.into(),
                    mime: None,
                    size: 0,
                    mtime: now_ms(),
                },
                folder: false,
                remote: None,
                unlinked: false,
            });
            st.attach(ino, parent.0);
            st.buffers.insert(
                ino,
                Buffer {
                    file: Arc::new(file),
                    dirty: true,
                },
            );
            st.next_fh += 1;
            let fh = st.next_fh;
            st.handles.insert(fh, ino);
            let (attr, g) = self.new_entry(&st, ino)?;
            Ok((attr, g, fh))
        })();
        match res {
            Ok((attr, g, fh)) => reply.created(&TTL, &attr, g, FileHandle(fh), FopenFlags::empty()),
            Err(e) => reply.error(e),
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let res = (|| {
            let (buffer, entry) = {
                let st = self.lock();
                (
                    st.buffers.get(&ino.0).map(|b| b.file.clone()),
                    st.inode(ino.0)?.entry(),
                )
            };
            if let Some(f) = buffer {
                let mut buf = vec![0; size as usize];
                let n = read_full_at(&f, &mut buf, offset).map_err(|e| self.fail(e.into()))?;
                buf.truncate(n);
                return Ok(buf);
            }
            match entry {
                Some(e) => self.read_remote(&e, offset, size),
                None => Ok(Vec::new()),
            }
        })();
        match res {
            Ok(data) => reply.data(&data),
            Err(e) => reply.error(e),
        }
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let res = (|| {
            self.writable()?;
            let file = self.buffer(ino.0, false)?;
            file.write_all_at(data, offset)
                .map_err(|e| self.fail(e.into()))?;
            let mut st = self.lock();
            if let Some(b) = st.buffers.get_mut(&ino.0) {
                b.dirty = true;
            }
            st.inode_mut(ino.0)?.meta.mtime = now_ms();
            Ok(data.len() as u32)
        })();
        match res {
            Ok(n) => reply.written(n),
            Err(e) => reply.error(e),
        }
    }

    /// Called on every close(2): upload here so a failure reaches the caller.
    fn flush(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        match self.upload(ino.0) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn fsync(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _datasync: bool,
        reply: ReplyEmpty,
    ) {
        match self.upload(ino.0) {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn release(
        &self,
        _req: &Request,
        ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let res = self.upload(ino.0);
        let mut st = self.lock();
        st.handles.remove(&fh.0);
        if st.open_count(ino.0) == 0 && !st.buffers.get(&ino.0).is_some_and(|b| b.dirty) {
            st.buffers.remove(&ino.0);
        }
        match res {
            Ok(()) => reply.ok(),
            Err(e) => reply.error(e),
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let res = (|| {
            let kids = self.children(ino.0, offset == 0)?;
            let st = self.lock();
            let parent = st.inode(ino.0)?.parent;
            let mut out = vec![
                (ino.0, FileType::Directory, ".".to_string()),
                (parent, FileType::Directory, "..".to_string()),
            ];
            for k in kids {
                if let Some(i) = st.inodes.get(&k) {
                    let kind = if i.folder {
                        FileType::Directory
                    } else {
                        FileType::RegularFile
                    };
                    out.push((k, kind, i.meta.name.clone()));
                }
            }
            Ok(out)
        })();
        match res {
            Ok(entries) => {
                for (n, (ino, kind, name)) in entries.into_iter().enumerate().skip(offset as usize)
                {
                    if reply.add(INodeNo(ino), n as u64 + 1, kind, name) {
                        break;
                    }
                }
                reply.ok()
            }
            Err(e) => reply.error(e),
        }
    }

    fn statfs(&self, _req: &Request, _ino: INodeNo, reply: ReplyStatfs) {
        let cached = self
            .lock()
            .quota
            .filter(|(t, ..)| t.elapsed() < Duration::from_secs(10));
        let (quota, used) = match cached {
            Some((_, q, u)) => (q, u),
            None => match self.client.fetch_me() {
                Ok(me) => {
                    self.lock().quota = Some((Instant::now(), me.quota_bytes, me.used_bytes));
                    (me.quota_bytes, me.used_bytes)
                }
                Err(_) => (self.client.me.quota_bytes, self.client.me.used_bytes),
            },
        };
        const BS: u64 = 4096;
        let total = quota.max(0) as u64 / BS;
        let free = (quota - used).max(0) as u64 / BS;
        reply.statfs(total, free, free, 0, 0, BS as u32, 255, BS as u32);
    }
}

fn session(
    client: Client,
    root: Entry,
    mountpoint: &Path,
    opts: MountOptions,
) -> io::Result<Session<CloudFs>> {
    let read_only = opts.read_only || client.scope == AppScope::Read;
    let mut config = Config::default();
    config.mount_options = vec![
        MountOption::FSName("thencloud".into()),
        MountOption::Subtype("thencloud".into()),
        MountOption::DefaultPermissions,
        MountOption::NoDev,
        MountOption::NoSuid,
        MountOption::NoAtime,
        if read_only {
            MountOption::RO
        } else {
            MountOption::RW
        },
    ];
    config.acl = if opts.allow_other {
        SessionACL::All
    } else {
        SessionACL::Owner
    };
    config.n_threads = Some(8);
    Session::new(CloudFs::new(client, root, opts), mountpoint, &config)
}

/// Mount `root` at `mountpoint` and serve it until it's unmounted
/// (`fusermount3 -u`, or Ctrl+C here).
pub fn mount(client: Client, root: Entry, mountpoint: &Path, opts: MountOptions) -> io::Result<()> {
    let mut session = session(client, root, mountpoint, opts)?;
    let mut unmounter = session.unmount_callable();
    unmount_on_signal(move || {
        let _ = unmounter.unmount();
    });
    session.run()
}

/// Mount in the background; dropping the handle unmounts.
pub fn spawn(
    client: Client,
    root: Entry,
    mountpoint: &Path,
    opts: MountOptions,
) -> io::Result<BackgroundSession> {
    session(client, root, mountpoint, opts)?.spawn()
}

/// Unmount cleanly on Ctrl+C, SIGTERM or SIGHUP instead of leaving a dead
/// mount behind.
fn unmount_on_signal(f: impl FnOnce() + Send + 'static) {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            libc::sigaddset(&mut set, s);
        }
        // Blocked here and in every thread started after this, so only the
        // waiting thread sees them.
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        std::thread::spawn(move || {
            let mut sig = 0;
            libc::sigwait(&set, &mut sig);
            f();
        });
    }
}
