//! The video downloader's way out: a small HTTP proxy on 127.0.0.1 that
//! yt-dlp is pointed at (`--proxy`), which only connects to public
//! addresses.
//!
//! `check_link` looks at the link before yt-dlp runs, but yt-dlp then
//! resolves names itself (a name can answer differently the second time),
//! follows redirects and fetches media URLs from the site's answers. Here
//! every connection is checked where it's made: the proxy resolves the host,
//! refuses it if any address is private or local, and connects to the
//! addresses it checked, so whatever is sent afterwards goes there.
//!
//! It takes `CONNECT host:port` (HTTPS) and absolute-form plain HTTP
//! requests, needs a password only the server knows (so other programs on
//! the machine can't use it), and logs nothing.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::downloader::{is_public, split_host_port};

/// Longest request head accepted.
const MAX_HEAD: usize = 16 * 1024;
/// How long the head and the connection to the site may take.
const SETUP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Egress {
    addr: SocketAddr,
    secret: String,
}

impl Egress {
    /// Start listening on a free port on 127.0.0.1. With `public_only`
    /// off (tests only) local addresses are let through too.
    pub async fn start(public_only: bool) -> std::io::Result<Egress> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let secret: String = thencloud_crypto::random_bytes(16)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let expected = format!(
            "Basic {}",
            base64_std(format!("thencloud:{secret}").as_bytes())
        );
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let expected = expected.clone();
                tokio::spawn(async move {
                    let _ = handle(client, &expected, public_only).await;
                });
            }
        });
        Ok(Egress { addr, secret })
    }

    /// What to give yt-dlp's `--proxy`.
    pub fn url(&self) -> String {
        format!("http://thencloud:{}@{}", self.secret, self.addr)
    }
}

/// Why a connection wasn't made, as the HTTP status yt-dlp gets.
enum Refused {
    BadRequest,
    Auth,
    Private,
    Unreachable,
}

impl Refused {
    fn response(&self) -> &'static [u8] {
        match self {
            Refused::BadRequest => b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n",
            Refused::Auth => {
                b"HTTP/1.1 407 Proxy Authentication Required\r\n\
                  Proxy-Authenticate: Basic realm=\"thencloud\"\r\nConnection: close\r\n\r\n"
            }
            Refused::Private => b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n",
            Refused::Unreachable => b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n",
        }
    }
}

async fn handle(mut client: TcpStream, expected: &str, public_only: bool) -> std::io::Result<()> {
    let setup =
        tokio::time::timeout(SETUP_TIMEOUT, setup(&mut client, expected, public_only)).await;
    let (mut upstream, forward) = match setup {
        Ok(Ok(v)) => v,
        Ok(Err(r)) => return client.write_all(r.response()).await,
        Err(_) => return Ok(()),
    };
    match forward {
        // HTTPS: say the tunnel is up, then pass bytes both ways.
        None => {
            client
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await?
        }
        // Plain HTTP: the rewritten request (and any body that came with it).
        Some(bytes) => upstream.write_all(&bytes).await?,
    }
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

/// Read the request head, check the password and the destination, and
/// connect. Returns the connection and, for plain HTTP, the bytes to send
/// first.
async fn setup(
    client: &mut TcpStream,
    expected: &str,
    public_only: bool,
) -> Result<(TcpStream, Option<Vec<u8>>), Refused> {
    let mut buf = Vec::with_capacity(1024);
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(Refused::BadRequest);
        }
        let mut chunk = [0u8; 2048];
        let n = client
            .read(&mut chunk)
            .await
            .map_err(|_| Refused::BadRequest)?;
        if n == 0 {
            return Err(Refused::BadRequest);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..end]).map_err(|_| Refused::BadRequest)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        return Err(Refused::BadRequest);
    };
    let headers: Vec<&str> = lines.filter(|l| !l.is_empty()).collect();
    let named = |l: &str, name: &str| {
        l.split_once(':')
            .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case(name))
    };
    let authorized = headers.iter().any(|l| {
        named(l, "proxy-authorization")
            && l.split_once(':').is_some_and(|(_, v)| v.trim() == expected)
    });
    if !authorized {
        return Err(Refused::Auth);
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_host_port(target).ok_or(Refused::BadRequest)?;
        let upstream = connect(&host, port, public_only).await?;
        return Ok((upstream, None));
    }
    let rest = target.strip_prefix("http://").ok_or(Refused::BadRequest)?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(Refused::BadRequest);
    }
    let (host, port) = split_host_port(authority).ok_or(Refused::BadRequest)?;
    // split_host_port assumes 443 when there's no port; plain HTTP is 80.
    let explicit = match authority.strip_prefix('[') {
        Some(rest) => rest.contains("]:"),
        None => authority.contains(':'),
    };
    let port = if explicit { port } else { 80 };
    let upstream = connect(&host, port, public_only).await?;
    // The same request in origin form, without what was meant for us.
    let mut out = format!("{method} {path} {version}\r\n").into_bytes();
    for l in headers {
        if !named(l, "proxy-authorization") && !named(l, "proxy-connection") {
            out.extend_from_slice(l.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&buf[end..]);
    Ok((upstream, Some(out)))
}

/// Resolve `host` and connect to it, unless any of its addresses is private
/// or local.
async fn connect(host: &str, port: u16, public_only: bool) -> Result<TcpStream, Refused> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| Refused::Unreachable)?
        .collect();
    if addrs.is_empty() {
        return Err(Refused::Unreachable);
    }
    if public_only && addrs.iter().any(|a| !is_public(&a.ip())) {
        return Err(Refused::Private);
    }
    TcpStream::connect(&addrs[..])
        .await
        .map_err(|_| Refused::Unreachable)
}

/// Standard base64 with padding, for the proxy's Basic credentials.
fn base64_std(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn ask(proxy: &Egress, head: &str) -> (String, TcpStream) {
        let mut s = TcpStream::connect(proxy.addr).await.unwrap();
        s.write_all(head.as_bytes()).await.unwrap();
        let mut got = Vec::new();
        let mut b = [0u8; 1];
        while !got.ends_with(b"\r\n\r\n") {
            if s.read(&mut b).await.unwrap() == 0 {
                break;
            }
            got.push(b[0]);
        }
        (String::from_utf8(got).unwrap(), s)
    }

    fn auth(p: &Egress) -> String {
        format!(
            "Proxy-Authorization: Basic {}\r\n",
            base64_std(format!("thencloud:{}", p.secret).as_bytes())
        )
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_std(i.as_bytes()), o);
        }
    }

    #[tokio::test]
    async fn needs_its_password() {
        let p = Egress::start(true).await.unwrap();
        let (r, _) = ask(
            &p,
            "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n",
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 407"), "{r}");
        let (r, _) = ask(
            &p,
            "CONNECT example.com:443 HTTP/1.1\r\nProxy-Authorization: Basic dGhlbmNsb3VkOm5vcGU=\r\n\r\n",
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 407"), "{r}");
    }

    #[tokio::test]
    async fn refuses_private_and_local_addresses() {
        let p = Egress::start(true).await.unwrap();
        let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = local.local_addr().unwrap().port();
        for target in [
            format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n"),
            format!("CONNECT localhost:{port} HTTP/1.1\r\n"),
            format!("CONNECT [::1]:{port} HTTP/1.1\r\n"),
            "CONNECT 169.254.169.254:80 HTTP/1.1\r\n".into(),
            format!("GET http://127.0.0.1:{port}/latest/meta-data HTTP/1.1\r\n"),
        ] {
            let (r, _) = ask(&p, &format!("{target}{}\r\n", auth(&p))).await;
            assert!(r.starts_with("HTTP/1.1 403"), "{target}: {r}");
        }
    }

    #[tokio::test]
    async fn tunnels_and_forwards() {
        // Local addresses allowed, as in tests, to have something to reach.
        let p = Egress::start(false).await.unwrap();
        let site = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = site.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut heads = Vec::new();
            for _ in 0..2 {
                let (mut s, _) = site.accept().await.unwrap();
                let mut got = Vec::new();
                let mut b = [0u8; 1];
                while !got.ends_with(b"\r\n\r\n") {
                    s.read_exact(&mut b).await.unwrap();
                    got.push(b[0]);
                }
                heads.push(String::from_utf8(got).unwrap());
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi")
                    .await
                    .unwrap();
            }
            heads
        });
        // HTTPS-style: a tunnel, then whatever the client sends.
        let (r, mut s) = ask(
            &p,
            &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n{}\r\n", auth(&p)),
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 200"), "{r}");
        s.write_all(b"GET /a HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        let mut body = vec![0u8; 40];
        let n = s.read(&mut body).await.unwrap();
        assert!(String::from_utf8_lossy(&body[..n]).ends_with("hi"));
        // Plain HTTP: the request goes on in origin form, without our password.
        let (r, _) = ask(
            &p,
            &format!(
                "GET http://127.0.0.1:{port}/b?c=1 HTTP/1.1\r\nHost: 127.0.0.1\r\n{}Proxy-Connection: keep-alive\r\n\r\n",
                auth(&p)
            ),
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 200 OK"), "{r}");
        let heads = server.await.unwrap();
        assert!(
            heads[1].starts_with("GET /b?c=1 HTTP/1.1\r\n"),
            "{}",
            heads[1]
        );
        assert!(!heads[1].to_lowercase().contains("proxy-"), "{}", heads[1]);
    }
}
