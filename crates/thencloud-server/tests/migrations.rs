//! Migrations are applied to databases that hold people's files, and sqlx
//! refuses to start when one it already applied has changed since. So a
//! migration, once committed, is never edited or removed: a change to the
//! schema is a new migration. This checks the files against
//! `migrations.sha256`, where a new migration's line is added with
//!
//!     (cd crates/thencloud-server/migrations && sha256sum NNNN_name.sql) >> crates/thencloud-server/migrations.sha256

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn migrations_are_never_edited() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pinned: BTreeMap<String, String> = std::fs::read_to_string(root.join("migrations.sha256"))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (hash, name) = l.split_once("  ").expect("lines are `<sha256>  <file>`");
            (name.to_owned(), hash.to_owned())
        })
        .collect();
    let mut found = BTreeMap::new();
    for entry in std::fs::read_dir(root.join("migrations")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let hash = Sha256::digest(std::fs::read(&path).unwrap());
        found.insert(
            name,
            hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        );
    }
    for (name, hash) in &pinned {
        match found.get(name) {
            None => panic!("{name} was removed; a database that applied it would no longer start"),
            Some(h) if h != hash => panic!(
                "{name} changed after it was committed; databases that applied it would no longer start. \
                 Put the change in a new migration instead."
            ),
            Some(_) => {}
        }
    }
    let unpinned: Vec<&String> = found.keys().filter(|n| !pinned.contains_key(*n)).collect();
    assert!(
        unpinned.is_empty(),
        "not in migrations.sha256 yet: {unpinned:?}; once final, add each with \
         `(cd crates/thencloud-server/migrations && sha256sum <file>) >> crates/thencloud-server/migrations.sha256`"
    );
}
