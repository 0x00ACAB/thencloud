//! `thencloud import-nextcloud`: copy a Nextcloud account's files in. The
//! files are read over Nextcloud's WebDAV, encrypted here and uploaded,
//! folder by folder, keeping names and modification times. Nothing is
//! written to disk on the way: each file streams from one server into the
//! other through the cipher.
//!
//! Running it again skips files already there with the same size and time,
//! so an import that stopped halfway can simply be started again.

use std::io::{self, Read};
use std::time::{Duration, UNIX_EPOCH};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use xmltree::{Element, XMLNode};

use crate::{Client, Entry, Error, Result, SyncStats};

/// The most a single PROPFIND answer may be (a folder of about 100,000
/// items); more is refused rather than read into memory.
const MAX_LISTING: u64 = 64 * 1024 * 1024;

/// What stays unescaped in a path segment we send.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// One file or folder in a Nextcloud listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub folder: bool,
    pub size: u64,
    /// Milliseconds since the epoch (Nextcloud keeps whole seconds).
    pub mtime: i64,
}

/// A Nextcloud account, reached over WebDAV with an app password.
pub struct Nextcloud {
    agent: ureq::Agent,
    /// Scheme, host and port, e.g. `https://cloud.example.com`.
    origin: String,
    /// The path of the user's files, e.g. `/nextcloud/remote.php/dav/files/alice/`.
    files: String,
    auth: String,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .allow_non_standard_methods(true)
        .timeout_global(Some(Duration::from_secs(3600)))
        .build()
        .into()
}

fn basic(user: &str, password: &str) -> String {
    use base64::Engine;
    let b = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
    format!("Basic {b}")
}

/// Split `https://host/sub` into `https://host` and `/sub/`.
fn split_url(server: &str) -> Result<(String, String)> {
    let server = server.trim_end_matches('/');
    let rest = server
        .strip_prefix("https://")
        .map(|r| ("https://", r))
        .or_else(|| server.strip_prefix("http://").map(|r| ("http://", r)))
        .ok_or_else(|| Error::Usage(format!("{server}: give the address with https://")))?;
    let (host, path) = match rest.1.find('/') {
        Some(i) => (&rest.1[..i], &rest.1[i..]),
        None => (rest.1, ""),
    };
    if host.is_empty() {
        return Err(Error::Usage(format!("{server}: no host name")));
    }
    Ok((format!("{}{host}", rest.0), format!("{path}/")))
}

/// Percent-encode each segment of a relative path.
fn encode_path(rel: &str) -> String {
    rel.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| format!("{}/", utf8_percent_encode(s, SEGMENT)))
        .collect()
}

/// A decoded href as a path: the scheme and host dropped if it has them.
fn href_path(href: &str) -> Option<String> {
    let path = if let Some(rest) = href
        .strip_prefix("https://")
        .or_else(|| href.strip_prefix("http://"))
    {
        &rest[rest.find('/')?..]
    } else {
        href
    };
    percent_decode_str(path)
        .decode_utf8()
        .ok()
        .map(|p| p.into_owned())
}

fn dav<'a>(e: &'a Element, name: &str) -> Option<&'a Element> {
    e.children.iter().find_map(|n| match n {
        XMLNode::Element(c) if c.name == name && c.namespace.as_deref() == Some("DAV:") => Some(c),
        _ => None,
    })
}

fn dav_all<'a>(e: &'a Element, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
    e.children.iter().filter_map(move |n| match n {
        XMLNode::Element(c) if c.name == name && c.namespace.as_deref() == Some("DAV:") => Some(c),
        _ => None,
    })
}

fn text(e: Option<&Element>) -> Option<String> {
    e.and_then(|e| e.get_text()).map(|t| t.trim().to_string())
}

/// The children of `dir` (a decoded path ending in `/`) in a PROPFIND
/// answer with Depth 1. The answer comes from another server, so anything
/// that isn't plainly a direct child with a usable name is left out.
pub fn parse_listing(xml: &[u8], dir: &str) -> Result<Vec<Item>> {
    let root = Element::parse(xml)
        .map_err(|e| Error::Usage(format!("Nextcloud sent a listing that isn't XML: {e}")))?;
    if root.name != "multistatus" || root.namespace.as_deref() != Some("DAV:") {
        return Err(Error::Usage(
            "Nextcloud sent something other than a WebDAV listing".into(),
        ));
    }
    let mut out = Vec::new();
    for resp in dav_all(&root, "response") {
        let Some(path) = text(dav(resp, "href")).and_then(|h| href_path(&h)) else {
            continue;
        };
        let Some(rest) = path.strip_prefix(dir) else {
            continue;
        };
        let name = rest.strip_suffix('/').unwrap_or(rest);
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            continue;
        }
        if name.chars().any(|c| c.is_control()) {
            continue;
        }
        let Some(prop) = dav_all(resp, "propstat")
            .filter(|ps| text(dav(ps, "status")).is_some_and(|s| s.contains(" 200 ")))
            .find_map(|ps| dav(ps, "prop"))
        else {
            continue;
        };
        let folder = dav(prop, "resourcetype").is_some_and(|r| dav(r, "collection").is_some());
        let size = if folder {
            0
        } else {
            match text(dav(prop, "getcontentlength")).and_then(|s| s.parse::<u64>().ok()) {
                Some(s) => s,
                None => continue,
            }
        };
        let mtime = text(dav(prop, "getlastmodified"))
            .and_then(|s| httpdate::parse_http_date(&s).ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64);
        out.push(Item {
            name: name.to_string(),
            folder,
            size,
            mtime,
        });
    }
    Ok(out)
}

const PROPFIND_ITEMS: &str = r#"<?xml version="1.0"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/></d:prop></d:propfind>"#;

const PROPFIND_PRINCIPAL: &str = r#"<?xml version="1.0"?>
<d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;

impl Nextcloud {
    /// Sign in as `user` with a password (an app password from Nextcloud's
    /// Settings > Security is best) and find where their files are.
    pub fn connect(server: &str, user: &str, password: &str) -> Result<Nextcloud> {
        let (origin, base) = split_url(server)?;
        let mut nc = Nextcloud {
            agent: agent(),
            origin,
            files: String::new(),
            auth: basic(user, password),
        };
        let dav_root = format!("{base}remote.php/dav/");
        // The files live under the user's id, which may differ from the
        // name they sign in with; the principal says what it is.
        let body = nc.propfind(&dav_root, "0", PROPFIND_PRINCIPAL)?;
        let id = Element::parse(&body[..])
            .ok()
            .and_then(|root| {
                let resp = dav(&root, "response")?;
                let prop = dav_all(resp, "propstat").find_map(|ps| dav(ps, "prop"))?;
                let href = text(dav(dav(prop, "current-user-principal")?, "href"))?;
                let path = href_path(&href)?;
                let id = path.trim_end_matches('/').rsplit('/').next()?.to_string();
                (!id.is_empty() && id != "." && id != "..").then_some(id)
            })
            .unwrap_or_else(|| user.to_string());
        nc.files = format!("{dav_root}files/{}/", utf8_percent_encode(&id, SEGMENT));
        Ok(nc)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin)
    }

    fn propfind(&self, path: &str, depth: &str, body: &str) -> Result<Vec<u8>> {
        let req = ureq::http::Request::builder()
            .method("PROPFIND")
            .uri(self.url(path))
            .header("Authorization", &self.auth)
            .header("Depth", depth)
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(body.as_bytes().to_vec())
            .map_err(|e| Error::Http(e.to_string()))?;
        let mut res = self.agent.run(req)?;
        match res.status().as_u16() {
            207 => {}
            401 => {
                return Err(Error::Usage(
                    "Nextcloud didn't accept the user name and password".into(),
                ));
            }
            404 => return Err(Error::Usage(format!("no such folder on Nextcloud: {path}"))),
            s => {
                return Err(Error::Usage(format!(
                    "Nextcloud answered a listing with HTTP {s}"
                )));
            }
        }
        Ok(res
            .body_mut()
            .with_config()
            .limit(MAX_LISTING)
            .read_to_vec()?)
    }

    /// The items in a folder (`rel` is relative to the user's files).
    pub fn list(&self, rel: &str) -> Result<Vec<Item>> {
        let path = format!("{}{}", self.files, encode_path(rel));
        let body = self.propfind(&path, "1", PROPFIND_ITEMS)?;
        let dir = href_path(&path).unwrap_or(path);
        parse_listing(&body, &dir)
    }

    /// A file's contents, which must come to exactly `size` bytes.
    pub fn open(&self, rel: &str, size: u64) -> Result<impl Read + use<>> {
        let path = format!("{}{}", self.files, encode_path(rel));
        let path = path.trim_end_matches('/');
        let res = self
            .agent
            .get(self.url(path))
            .header("Authorization", &self.auth)
            .call()?;
        if res.status().as_u16() != 200 {
            return Err(Error::Usage(format!(
                "Nextcloud answered HTTP {} for {rel}",
                res.status().as_u16()
            )));
        }
        Ok(Exact {
            inner: res.into_body().into_reader(),
            left: size,
        })
    }
}

/// Reads exactly the size a file was listed with. Fewer bytes (a dropped
/// connection) or more (it changed meanwhile) is an error, so the upload
/// fails instead of being padded or cut short.
struct Exact<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Exact<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            let mut probe = [0u8; 1];
            return match self.inner.read(&mut probe)? {
                0 => Ok(0),
                _ => Err(io::Error::other("the file grew while it was being copied")),
            };
        }
        let want = buf.len().min(self.left as usize);
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the download ended early",
            ));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

impl Client {
    /// Copy `from` (a folder on Nextcloud, "" for all files) into `into`.
    /// Calls `report` with each path as it's uploaded.
    pub fn import_nextcloud(
        &self,
        nc: &Nextcloud,
        from: &str,
        into: &Entry,
        report: &mut dyn FnMut(&str),
    ) -> Result<SyncStats> {
        let mut stats = SyncStats::default();
        let from = from.trim_matches('/');
        self.import_into(nc, from, into, "", report, &mut stats)?;
        Ok(stats)
    }

    fn import_into(
        &self,
        nc: &Nextcloud,
        rel: &str,
        into: &Entry,
        prefix: &str,
        report: &mut dyn FnMut(&str),
        stats: &mut SyncStats,
    ) -> Result<()> {
        let kids = self.list(into)?;
        let mut items = nc.list(rel)?;
        items.sort_by(|a, b| a.name.cmp(&b.name));
        for item in items {
            let shown = format!("{prefix}{}", item.name);
            let src = if rel.is_empty() {
                item.name.clone()
            } else {
                format!("{rel}/{}", item.name)
            };
            let there = kids
                .iter()
                .find(|k| k.meta.name.to_lowercase() == item.name.to_lowercase());
            if item.folder {
                let folder = match there {
                    Some(f) if f.is_folder() => f.clone(),
                    Some(_) => {
                        return Err(Error::Usage(format!(
                            "{shown} is a file here but a folder on Nextcloud"
                        )));
                    }
                    None => {
                        let f = self.mkdir(into, &item.name)?;
                        let mut meta = f.meta.clone();
                        meta.mtime = item.mtime;
                        self.update(&f, into, meta)?
                    }
                };
                self.import_into(nc, &src, &folder, &format!("{shown}/"), report, stats)?;
                continue;
            }
            match there {
                Some(f) if f.is_folder() => {
                    return Err(Error::Usage(format!(
                        "{shown} is a folder here but a file on Nextcloud"
                    )));
                }
                Some(f) if f.meta.size == item.size && f.meta.mtime / 1000 == item.mtime / 1000 => {
                    stats.unchanged += 1;
                }
                other => {
                    report(&shown);
                    let mut body = nc.open(&src, item.size)?;
                    self.upload_from(&mut body, item.size, item.mtime, into, &item.name, other)?;
                    stats.transferred += 1;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:oc="http://owncloud.org/ns">
 <d:response><d:href>/nc/remote.php/dav/files/alice/Photos/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype>
   <d:getlastmodified>Tue, 01 Sep 2026 10:00:00 GMT</d:getlastmodified></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/nc/remote.php/dav/files/alice/Photos/Beach%20day.jpg</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>1234</d:getcontentlength>
   <d:getlastmodified>Wed, 02 Sep 2026 11:30:05 GMT</d:getlastmodified></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat>
  <d:propstat><d:prop><oc:size/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>
 <d:response><d:href>https://cloud.example.com/nc/remote.php/dav/files/alice/Photos/2026/</d:href>
  <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype>
   <d:getlastmodified>Wed, 02 Sep 2026 11:30:05 GMT</d:getlastmodified></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/nc/remote.php/dav/files/alice/Photos/2026/deeper.jpg</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>1</d:getcontentlength></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/nc/remote.php/dav/files/bob/secret.txt</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>1</d:getcontentlength></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
 <d:response><d:href>/nc/remote.php/dav/files/alice/Photos/..</d:href>
  <d:propstat><d:prop><d:resourcetype/><d:getcontentlength>1</d:getcontentlength></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;

    #[test]
    fn listing_keeps_only_direct_children() {
        let items =
            parse_listing(LISTING.as_bytes(), "/nc/remote.php/dav/files/alice/Photos/").unwrap();
        assert_eq!(
            items,
            vec![
                Item {
                    name: "Beach day.jpg".into(),
                    folder: false,
                    size: 1234,
                    mtime: 1_788_348_605_000,
                },
                Item {
                    name: "2026".into(),
                    folder: true,
                    size: 0,
                    mtime: 1_788_348_605_000,
                },
            ]
        );
    }

    #[test]
    fn listing_that_isnt_webdav_is_refused() {
        assert!(parse_listing(b"<html>hi</html>", "/").is_err());
        assert!(parse_listing(b"not xml", "/").is_err());
    }

    #[test]
    fn urls() {
        assert_eq!(
            split_url("https://example.com/nextcloud/").unwrap(),
            ("https://example.com".into(), "/nextcloud/".into())
        );
        assert_eq!(
            split_url("http://127.0.0.1:8080").unwrap(),
            ("http://127.0.0.1:8080".into(), "/".into())
        );
        assert!(split_url("example.com").is_err());
        assert_eq!(encode_path("a b/c#d/"), "a%20b/c%23d/");
        assert_eq!(encode_path(""), "");
    }

    #[test]
    fn exact_reader_refuses_short_and_long() {
        let mut out = Vec::new();
        let r = Exact {
            inner: &b"hello"[..],
            left: 5,
        }
        .read_to_end(&mut out);
        assert_eq!(r.unwrap(), 5);
        assert!(
            Exact {
                inner: &b"hell"[..],
                left: 5
            }
            .read_to_end(&mut Vec::new())
            .is_err()
        );
        assert!(
            Exact {
                inner: &b"hello!"[..],
                left: 5
            }
            .read_to_end(&mut Vec::new())
            .is_err()
        );
    }
}
