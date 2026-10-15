//! `thencloud import-takeout`: copy an extracted Google Takeout into a
//! folder, encrypting everything here. Google Photos puts each photo's date
//! in a JSON "sidecar" next to it (the file's own time is the export's), so
//! those are read for the file time and the Photos timeline (`taken`), and
//! not uploaded themselves. Folders are kept as they are. Safe to run
//! again: a file already there with the same size is skipped.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::{Client, Entry, Error, Result, SyncStats};

/// Takeout cuts sidecar names to this many characters, before ".json".
const MAX_SIDECAR_STEM: usize = 46;

/// The dates a sidecar gives a file: (taken, created), in milliseconds.
#[derive(Debug, Default, PartialEq)]
pub struct Dates {
    pub taken: Option<i64>,
    pub created: Option<i64>,
}

/// Read `photoTakenTime` and `creationTime` from a sidecar's JSON. Their
/// `timestamp` is seconds, as a string. Anything else gives no dates.
pub fn parse_sidecar(json: &[u8]) -> Dates {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(json) else {
        return Dates::default();
    };
    let ms = |key: &str| {
        let t = v.get(key)?.get("timestamp")?;
        let secs = t
            .as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| t.as_i64())?;
        // Before 1900 or after 2200 is a broken sidecar, not a date.
        (-2_208_988_800..7_258_118_400)
            .contains(&secs)
            .then_some(secs * 1000)
    };
    Dates {
        taken: ms("photoTakenTime"),
        created: ms("creationTime"),
    }
}

/// The names a sidecar for `file` can have, most likely first: Takeout
/// writes "x.jpg.json", newer exports "x.jpg.supplemental-metadata.json"
/// (both cut to 46 characters before ".json"), and for "x(1).jpg" the
/// number goes after the extension: "x.jpg(1).json".
pub fn sidecar_names(file: &str) -> Vec<String> {
    let cut = |s: &str| s.chars().take(MAX_SIDECAR_STEM).collect::<String>();
    let mut out = vec![
        format!("{}.json", cut(&format!("{file}.supplemental-metadata"))),
        format!("{}.json", cut(file)),
    ];
    if let Some((stem, ext)) = file.rsplit_once('.')
        && let Some(open) = stem.rfind('(')
        && stem.ends_with(')')
        && stem[open + 1..stem.len() - 1]
            .chars()
            .all(|c| c.is_ascii_digit())
    {
        let (base, n) = stem.split_at(open);
        out.push(format!(
            "{}{n}.json",
            cut(&format!("{base}.{ext}.supplemental-metadata"))
        ));
        out.push(format!("{}{n}.json", cut(&format!("{base}.{ext}"))));
    }
    out.dedup();
    out
}

impl Client {
    /// Copy the extracted Takeout at `dir` into `into`. `report` gets each
    /// file copied, as a path under `dir`.
    pub fn import_takeout(
        &self,
        dir: &Path,
        into: &Entry,
        report: &mut dyn FnMut(&str),
    ) -> Result<SyncStats> {
        if !dir.is_dir() {
            return Err(Error::Usage(format!("{} isn't a folder", dir.display())));
        }
        let mut stats = SyncStats::default();
        self.takeout_into(dir, into, "", report, &mut stats)?;
        Ok(stats)
    }

    fn takeout_into(
        &self,
        dir: &Path,
        into: &Entry,
        prefix: &str,
        report: &mut dyn FnMut(&str),
        stats: &mut SyncStats,
    ) -> Result<()> {
        let mut names: Vec<String> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        let here: HashSet<&str> = names.iter().map(String::as_str).collect();

        // Sidecars of files in this folder, and album descriptions: read,
        // never uploaded.
        let mut sidecars: HashSet<String> = HashSet::new();
        for n in &names {
            if n.ends_with(".json") || dir.join(n).is_dir() {
                continue;
            }
            if let Some(s) = sidecar_names(n)
                .into_iter()
                .find(|s| here.contains(s.as_str()))
            {
                sidecars.insert(s);
            }
        }
        if here.contains("metadata.json") && names.iter().any(|n| !n.ends_with(".json")) {
            sidecars.insert("metadata.json".into());
        }

        let kids = self.list(into)?;
        for name in &names {
            let path = dir.join(name);
            let shown = format!("{prefix}{name}");
            let there = kids
                .iter()
                .find(|k| k.meta.name.to_lowercase() == name.to_lowercase());
            if path.is_dir() {
                let folder = match there {
                    Some(f) if f.is_folder() => f.clone(),
                    Some(_) => {
                        return Err(Error::Usage(format!(
                            "{shown} is a file here but a folder in the Takeout"
                        )));
                    }
                    None => self.mkdir(into, name)?,
                };
                self.takeout_into(&path, &folder, &format!("{shown}/"), report, stats)?;
                continue;
            }
            if sidecars.contains(name) || !path.is_file() {
                continue;
            }
            let size = fs::metadata(&path)?.len();
            match there {
                Some(f) if f.is_folder() => {
                    return Err(Error::Usage(format!(
                        "{shown} is a folder here but a file in the Takeout"
                    )));
                }
                Some(f) if f.meta.size == size => stats.unchanged += 1,
                other => {
                    let dates = sidecar_names(name)
                        .into_iter()
                        .find(|s| here.contains(s.as_str()))
                        .and_then(|s| fs::read(dir.join(s)).ok())
                        .map(|b| parse_sidecar(&b))
                        .unwrap_or_default();
                    let mtime = match dates.taken.or(dates.created) {
                        Some(t) => t,
                        None => fs::metadata(&path)?
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_millis() as i64),
                    };
                    report(&shown);
                    let mut f = fs::File::open(&path)?;
                    self.upload_taken(&mut f, size, mtime, dates.taken, into, name, other)?;
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

    #[test]
    fn sidecar_dates() {
        let d = parse_sidecar(br#"{"title":"IMG_1.jpg","creationTime":{"timestamp":"1700000000"},"photoTakenTime":{"timestamp":"1600000000","formatted":"..."}}"#);
        assert_eq!(
            d,
            Dates {
                taken: Some(1_600_000_000_000),
                created: Some(1_700_000_000_000)
            }
        );
        assert_eq!(
            parse_sidecar(br#"{"photoTakenTime":{"timestamp":1600000000}}"#).taken,
            Some(1_600_000_000_000)
        );
        assert_eq!(parse_sidecar(b"not json"), Dates::default());
        assert_eq!(
            parse_sidecar(br#"{"photoTakenTime":{"timestamp":"99999999999999"}}"#).taken,
            None
        );
        assert_eq!(
            parse_sidecar(br#"{"photoTakenTime":"soon"}"#),
            Dates::default()
        );
    }

    #[test]
    fn sidecar_name_forms() {
        let n = sidecar_names("IMG_1234.jpg");
        assert!(n.contains(&"IMG_1234.jpg.json".to_string()));
        assert!(n.contains(&"IMG_1234.jpg.supplemental-metadata.json".to_string()));
        // Numbered copies put the number after the extension.
        assert!(sidecar_names("IMG_1234(1).jpg").contains(&"IMG_1234.jpg(1).json".to_string()));
        assert!(
            sidecar_names("IMG_1234(1).jpg")
                .contains(&"IMG_1234.jpg.supplemental-metadata(1).json".to_string())
        );
        // Long names are cut to 46 characters before ".json".
        let long = "PXL_20260101_123456789.PORTRAIT.ORIGINAL.MP.jpg";
        assert!(sidecar_names(long).contains(&format!(
            "{}.json",
            &format!("{long}.supplemental-metadata")[..46]
        )));
        // Names with no number in parentheses, or odd characters, don't panic.
        assert_eq!(sidecar_names("notes (draft).txt").len(), 2);
        sidecar_names("ąę(é).ż");
        sidecar_names("");
    }
}
