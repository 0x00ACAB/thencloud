//! Sigstore bundles checked against real releases' bundles, and tampered
//! copies of them that must not pass.

use serde_json::Value;
use thencloud_cli::sigstore::{GITHUB_ACTIONS, Identity, verify};

const GR_FILE: &[u8] = include_bytes!("sigstore/goreleaser-v2.18.2-checksums.txt");
const GR_BUNDLE: &[u8] = include_bytes!("sigstore/goreleaser-v2.18.2-checksums.txt.sigstore.json");
const CO_FILE: &[u8] = include_bytes!("sigstore/cosign-v3.1.3-checksums.txt");
const CO_BUNDLE: &[u8] = include_bytes!("sigstore/cosign-v3.1.3-checksums.txt.sigstore.json");

const GR: Identity = Identity {
    name: "https://github.com/goreleaser/goreleaser/.github/workflows/release.yml@refs/tags/v2.18.2",
    issuer: GITHUB_ACTIONS,
};
const CO: Identity = Identity {
    name: "keyless@projectsigstore.iam.gserviceaccount.com",
    issuer: "https://accounts.google.com",
};

fn edit(bundle: &[u8], f: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut v: Value = serde_json::from_slice(bundle).unwrap();
    f(&mut v);
    serde_json::to_vec(&v).unwrap()
}

fn err(r: thencloud_cli::Result<thencloud_cli::sigstore::Verified>) -> String {
    match r {
        Ok(v) => panic!("should not pass: {v:?}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn real_bundles_pass() {
    let v = verify(GR_BUNDLE, GR_FILE, &GR).unwrap();
    assert_eq!(v.integrated_time, 1_789_615_987);
    verify(CO_BUNDLE, CO_FILE, &CO).unwrap();
}

#[test]
fn another_file_tag_or_issuer_fails() {
    let mut file = GR_FILE.to_vec();
    file[0] ^= 1;
    assert!(err(verify(GR_BUNDLE, &file, &GR)).contains("different file"));
    let other_tag = Identity {
        name: "https://github.com/goreleaser/goreleaser/.github/workflows/release.yml@refs/tags/v2.18.3",
        issuer: GITHUB_ACTIONS,
    };
    assert!(err(verify(GR_BUNDLE, GR_FILE, &other_tag)).contains("signed by"));
    let other_issuer = Identity {
        name: GR.name,
        issuer: "https://accounts.google.com",
    };
    assert!(err(verify(GR_BUNDLE, GR_FILE, &other_issuer)).contains("vouched"));
    // Another project's release bundle for the same identity check.
    assert!(err(verify(CO_BUNDLE, CO_FILE, &GR)).contains("signed by"));
}

#[test]
fn tampered_bundles_fail() {
    // The file's signature.
    let b = edit(GR_BUNDLE, |v| {
        let s = v["messageSignature"]["signature"]
            .as_str()
            .unwrap()
            .to_string();
        let mut raw = base64_decode(&s);
        let n = raw.len();
        raw[n - 1] ^= 1;
        v["messageSignature"]["signature"] = base64_encode(&raw).into();
    });
    assert!(err(verify(&b, GR_FILE, &GR)).contains("doesn't verify"));
    // Rekor's promise.
    let b = edit(GR_BUNDLE, |v| {
        let e = &mut v["verificationMaterial"]["tlogEntries"][0];
        let t: i64 = e["integratedTime"].as_str().unwrap().parse().unwrap();
        e["integratedTime"] = (t + 1).to_string().into();
    });
    assert!(err(verify(&b, GR_FILE, &GR)).contains("transparency log's signature"));
    // Someone else's certificate.
    let co: Value = serde_json::from_slice(CO_BUNDLE).unwrap();
    let b = edit(GR_BUNDLE, |v| {
        v["verificationMaterial"]["certificate"] =
            co["verificationMaterial"]["certificate"].clone();
    });
    assert!(verify(&b, GR_FILE, &GR).is_err());
    // A log entry for another signature.
    let b = edit(GR_BUNDLE, |v| {
        v["verificationMaterial"]["tlogEntries"] =
            co["verificationMaterial"]["tlogEntries"].clone();
    });
    assert!(verify(&b, GR_FILE, &GR).is_err());
    // No signed entry timestamp (as in Rekor v2 bundles).
    let b = edit(GR_BUNDLE, |v| {
        v["verificationMaterial"]["tlogEntries"][0]
            .as_object_mut()
            .unwrap()
            .remove("inclusionPromise");
    });
    assert!(err(verify(&b, GR_FILE, &GR)).contains("Rekor v2"));
    // A logged time the certificate wasn't valid at, even re-signed, fails
    // at the certificate check first; with the promise unchanged it fails.
    let b = edit(GR_BUNDLE, |v| {
        v["verificationMaterial"]["tlogEntries"][0]["integratedTime"] = "1789000000".into();
    });
    assert!(verify(&b, GR_FILE, &GR).is_err());
    // A certificate Fulcio didn't sign: its signature, one bit off.
    let b = edit(GR_BUNDLE, |v| {
        let c = &mut v["verificationMaterial"]["certificate"]["rawBytes"];
        let mut der = base64_decode(c.as_str().unwrap());
        let n = der.len();
        der[n - 2] ^= 1;
        *c = base64_encode(&der).into();
    });
    assert!(err(verify(&b, GR_FILE, &GR)).contains("issued by Sigstore"));
    // An entry from a log that isn't trusted.
    let b = edit(GR_BUNDLE, |v| {
        v["verificationMaterial"]["tlogEntries"][0]["logId"]["keyId"] =
            base64_encode(&[7u8; 32]).into();
    });
    assert!(err(verify(&b, GR_FILE, &GR)).contains("doesn't trust"));
    // Not JSON, not a bundle.
    assert!(verify(b"", GR_FILE, &GR).is_err());
    assert!(verify(br#"{"mediaType":"text/plain"}"#, GR_FILE, &GR).is_err());
}

fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}

fn base64_encode(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}
