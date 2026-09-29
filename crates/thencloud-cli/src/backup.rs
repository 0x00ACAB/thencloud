//! Encrypted backups (`thencloud backup` and `restore`).
//!
//! A backup is one file, readable only with its backup key: a random key
//! made for each backup and shown once, like a recovery key. It holds your
//! files decrypted and sealed again under that key, so it can be restored
//! into any account on any server, where everything is encrypted afresh.
//!
//! ```text
//! file   = "thncbk01" || backup id (16 random bytes) || record*
//! record = length (u32, big-endian) || seal_backup_record(key, id, index, plain)
//! plain  = 'E' || JSON entry   a file or folder: {"path": [..names], "folder", "size", "mtime", "mime"?}
//!        | 'D' || bytes        the next bytes of the last file entry (at most 4 MiB)
//!        | 'Z'                 the end; nothing may follow
//! ```
//!
//! A file entry is followed by exactly `size` bytes of data records.
//! Records are numbered from 0 and the number is bound into each one, so a
//! backup cut short (no end record), reordered or spliced fails to open.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};
use thencloud_crypto::{self as c, CHUNK_SIZE, Key};

use crate::{Error, Result};

const ENTRY: u8 = b'E';
const DATA: u8 = b'D';
const END: u8 = b'Z';
/// The largest record: a data piece, its type byte, and what sealing adds.
const MAX_RECORD: usize = CHUNK_SIZE + 1 + c::SEALED_OVERHEAD;

/// A file or folder in a backup. `path` runs from the folder backed up
/// (not included) to this item's own name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackupEntry {
    pub path: Vec<String>,
    pub folder: bool,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mtime: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
}

pub struct BackupWriter<W: Write> {
    out: W,
    key: Key,
    id: Vec<u8>,
    index: u64,
}

impl<W: Write> BackupWriter<W> {
    pub fn new(mut out: W, key: Key) -> Result<Self> {
        let id = c::random_bytes(c::BACKUP_ID_LEN);
        out.write_all(c::BACKUP_MAGIC)?;
        out.write_all(&id)?;
        Ok(BackupWriter {
            out,
            key,
            id,
            index: 0,
        })
    }

    fn record(&mut self, kind: u8, payload: &[u8]) -> Result<()> {
        let mut plain = Vec::with_capacity(1 + payload.len());
        plain.push(kind);
        plain.extend_from_slice(payload);
        let sealed = c::seal_backup_record(&self.key, &self.id, self.index, &plain);
        self.index += 1;
        self.out.write_all(&(sealed.len() as u32).to_be_bytes())?;
        self.out.write_all(&sealed)?;
        Ok(())
    }

    pub fn entry(&mut self, e: &BackupEntry) -> Result<()> {
        self.record(ENTRY, &serde_json::to_vec(e).expect("serialisable"))
    }

    /// The next bytes of the last file entry, in pieces of at most 4 MiB.
    pub fn data(&mut self, bytes: &[u8]) -> Result<()> {
        for piece in bytes.chunks(CHUNK_SIZE) {
            self.record(DATA, piece)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<W> {
        self.record(END, &[])?;
        self.out.flush()?;
        Ok(self.out)
    }
}

pub enum Record {
    Entry(BackupEntry),
    Data(Vec<u8>),
}

pub struct BackupReader<R: Read> {
    inp: R,
    key: Key,
    id: Vec<u8>,
    index: u64,
    done: bool,
}

fn damaged(what: &str) -> Error {
    Error::Usage(format!("this backup is damaged: {what}"))
}

impl<R: Read> BackupReader<R> {
    pub fn new(mut inp: R, key: Key) -> Result<Self> {
        let mut head = [0u8; 8 + c::BACKUP_ID_LEN];
        inp.read_exact(&mut head)
            .map_err(|_| Error::Usage("that isn't a thencloud backup".into()))?;
        if &head[..8] != c::BACKUP_MAGIC {
            return Err(Error::Usage("that isn't a thencloud backup".into()));
        }
        Ok(BackupReader {
            inp,
            key,
            id: head[8..].to_vec(),
            index: 0,
            done: false,
        })
    }

    /// The next entry or piece of data, or `None` after the end record.
    pub fn next_record(&mut self) -> Result<Option<Record>> {
        if self.done {
            return Ok(None);
        }
        let mut len = [0u8; 4];
        self.inp
            .read_exact(&mut len)
            .map_err(|_| damaged("it's cut short"))?;
        let len = u32::from_be_bytes(len) as usize;
        if len > MAX_RECORD {
            return Err(damaged("a record is too long"));
        }
        let mut sealed = vec![0u8; len];
        self.inp
            .read_exact(&mut sealed)
            .map_err(|_| damaged("it's cut short"))?;
        let plain = c::open_backup_record(&self.key, &self.id, self.index, &sealed).map_err(
            |_| match self.index {
                // The first record not opening is almost always the wrong key.
                0 => Error::Usage("that backup key doesn't open this backup".into()),
                _ => damaged("a record doesn't open where it is"),
            },
        )?;
        self.index += 1;
        match plain.split_first() {
            Some((&ENTRY, json)) => Ok(Some(Record::Entry(
                serde_json::from_slice(json).map_err(|_| damaged("an entry doesn't read"))?,
            ))),
            Some((&DATA, bytes)) => Ok(Some(Record::Data(bytes.to_vec()))),
            Some((&END, [])) => {
                let mut extra = [0u8; 1];
                if self.inp.read(&mut extra)? != 0 {
                    return Err(damaged("something follows its end"));
                }
                self.done = true;
                Ok(None)
            }
            _ => Err(damaged("a record is of no known kind")),
        }
    }

    /// A reader over the `size` bytes of data following a file entry.
    pub fn file_data(&mut self, size: u64) -> FileData<'_, R> {
        FileData {
            backup: self,
            left: size,
            buf: Vec::new(),
            at: 0,
        }
    }
}

pub struct FileData<'a, R: Read> {
    backup: &'a mut BackupReader<R>,
    left: u64,
    buf: Vec<u8>,
    at: usize,
}

impl<R: Read> Read for FileData<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.at == self.buf.len() {
            if self.left == 0 {
                return Ok(0);
            }
            let bytes = match self.backup.next_record() {
                Ok(Some(Record::Data(b))) if !b.is_empty() && b.len() as u64 <= self.left => b,
                Ok(_) => return Err(io::Error::other(damaged("a file's data is missing"))),
                Err(e) => return Err(io::Error::other(e)),
            };
            self.left -= bytes.len() as u64;
            self.buf = bytes;
            self.at = 0;
        }
        let n = out.len().min(self.buf.len() - self.at);
        out[..n].copy_from_slice(&self.buf[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(key: &Key) -> Vec<u8> {
        let mut w = BackupWriter::new(Vec::new(), key.clone()).unwrap();
        w.entry(&BackupEntry {
            path: vec!["Docs".into()],
            folder: true,
            size: 0,
            mtime: 1,
            mime: None,
        })
        .unwrap();
        w.entry(&BackupEntry {
            path: vec!["Docs".into(), "a.txt".into()],
            folder: false,
            size: 5,
            mtime: 2,
            mime: None,
        })
        .unwrap();
        w.data(b"hello").unwrap();
        w.finish().unwrap()
    }

    fn read_all(bytes: &[u8], key: &Key) -> Result<Vec<(BackupEntry, Vec<u8>)>> {
        let mut r = BackupReader::new(bytes, key.clone())?;
        let mut out = vec![];
        while let Some(rec) = r.next_record()? {
            let Record::Entry(e) = rec else {
                return Err(damaged("data without an entry"));
            };
            let mut data = vec![];
            if !e.folder {
                r.file_data(e.size).read_to_end(&mut data)?;
            }
            out.push((e, data));
        }
        Ok(out)
    }

    #[test]
    fn round_trip() {
        let key = Key::generate();
        let got = read_all(&sample(&key), &key).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1].0.path, ["Docs", "a.txt"]);
        assert_eq!(got[1].1, b"hello");
    }

    #[test]
    fn damage_is_caught() {
        let key = Key::generate();
        let good = sample(&key);
        assert!(read_all(&good, &Key::generate()).is_err(), "wrong key");
        for cut in [10, 30, good.len() - 1] {
            assert!(read_all(&good[..cut], &key).is_err(), "cut at {cut}");
        }
        let mut extra = good.clone();
        extra.push(0);
        assert!(read_all(&extra, &key).is_err(), "trailing bytes");
        let mut flipped = good.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        assert!(read_all(&flipped, &key).is_err(), "flipped bit");
        // Records from another backup with the same key don't fit.
        let other = sample(&key);
        let mut spliced = good[..24].to_vec();
        spliced.extend_from_slice(&other[24..]);
        assert!(read_all(&spliced, &key).is_err(), "spliced");
    }
}
