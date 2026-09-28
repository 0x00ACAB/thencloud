//! The CLI's backup reader on hostile input: nothing may panic or allocate
//! without bound, and a changed backup must never read back as a whole.
#![no_main]

use std::io::Read;
use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use thencloud_cli::backup::{BackupEntry, BackupReader, BackupWriter, Record};
use thencloud_crypto::Key;

static KEY: LazyLock<Key> = LazyLock::new(|| Key::from_slice(&[9; 32]).unwrap());
static GOOD: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut w = BackupWriter::new(Vec::new(), KEY.clone()).unwrap();
    for (i, name) in ["a.txt", "b.txt"].iter().enumerate() {
        w.entry(&BackupEntry {
            path: vec!["Docs".into(), (*name).into()],
            folder: false,
            size: 5,
            mtime: i as i64,
            mime: None,
        })
        .unwrap();
        w.data(b"hello").unwrap();
    }
    w.finish().unwrap()
});

/// Read a whole backup the way `restore` does. True if it read cleanly.
fn read_all(bytes: &[u8]) -> bool {
    let Ok(mut r) = BackupReader::new(bytes, KEY.clone()) else {
        return false;
    };
    loop {
        match r.next_record() {
            Ok(None) => return true,
            Ok(Some(Record::Entry(e))) if !e.folder => {
                let mut sink = Vec::new();
                if r.file_data(e.size.min(1 << 20)).read_to_end(&mut sink).is_err() {
                    return false;
                }
            }
            Ok(Some(_)) => {}
            Err(_) => return false,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    read_all(data);
    // The good backup with bytes flipped where `data` says.
    let mut bad = GOOD.clone();
    for pair in data.chunks_exact(3) {
        let at = u16::from_le_bytes([pair[0], pair[1]]) as usize % bad.len();
        bad[at] ^= pair[2];
    }
    if bad != *GOOD {
        assert!(!read_all(&bad), "a changed backup read back cleanly");
    }
});
