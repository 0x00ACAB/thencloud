//! Checking a Sigstore bundle, offline, for `thencloud verify-web`.
//!
//! Releases are signed in the release workflow with `cosign sign-blob`:
//! GitHub's OIDC token gets a short-lived certificate from Sigstore's Fulcio
//! naming the exact workflow and tag, the signature is logged in Rekor, and
//! everything needed to check it later goes in a `.sigstore.json` bundle.
//! This checks such a bundle against the trust root built in here
//! (`sigstore_trusted_root.json`, from sigstore/root-signing), without
//! contacting anyone:
//!
//! 1. the certificate chains to a Fulcio root and was valid when Rekor
//!    logged the signature;
//! 2. it was issued to the expected identity (the release workflow at the
//!    release's tag) by the expected OIDC issuer, for code signing;
//! 3. the signature over the file verifies with the certificate's key;
//! 4. Rekor's signed entry timestamp verifies with a trusted Rekor key, and
//!    the logged entry is this signature, this certificate and this file.
//!
//! Only message-signature bundles logged in Rekor v1 (`hashedrekord` with a
//! signed entry timestamp) are read; anything else is refused with a reason.
//! The bundle comes from a server or a download, so it's parsed as untrusted
//! input (fuzzed: `sigstore-bundle`).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::signature::{self, UnparsedPublicKey, VerificationAlgorithm};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use x509_cert::Certificate;
use x509_cert::der::asn1::Utf8StringRef;
use x509_cert::der::oid::ObjectIdentifier;
use x509_cert::der::{Decode, Encode};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{BasicConstraints, ExtendedKeyUsage, SubjectAltName};

use crate::{Error, Result};

/// The OIDC issuer of GitHub Actions' tokens.
pub const GITHUB_ACTIONS: &str = "https://token.actions.githubusercontent.com";

const TRUSTED_ROOT: &str = include_str!("sigstore_trusted_root.json");

const OID_EC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const OID_P256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const OID_P384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");
const OID_ECDSA_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.2");
const OID_ECDSA_SHA384: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.4.3.3");
const OID_CODE_SIGNING: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.5.5.7.3.3");
/// Fulcio's OIDC issuer extension: the newer one (a DER UTF8String), then
/// the original (the raw string).
const OID_ISSUER_V2: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.57264.1.8");
const OID_ISSUER_V1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.6.1.4.1.57264.1.1");

/// Who must have signed: the certificate's subject alternative name (a
/// workflow URI, or an email address) and the OIDC issuer that vouched.
pub struct Identity<'a> {
    pub name: &'a str,
    pub issuer: &'a str,
}

/// What a good bundle says about the signature.
#[derive(Debug)]
pub struct Verified {
    /// When Rekor logged it (seconds since the epoch).
    pub integrated_time: i64,
    pub log_index: i64,
}

fn bad(what: impl Into<String>) -> Error {
    Error::Usage(format!(
        "the Sigstore bundle doesn't check out: {}",
        what.into()
    ))
}

// ------------------------------------------------------------ trust root

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrustedRoot {
    tlogs: Vec<Tlog>,
    certificate_authorities: Vec<CertificateAuthority>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tlog {
    public_key: PublicKey,
    log_id: LogId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicKey {
    raw_bytes: String,
    key_details: String,
    valid_for: ValidFor,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LogId {
    key_id: String,
}

#[derive(Deserialize)]
struct ValidFor {
    start: String,
    #[serde(default)]
    end: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CertificateAuthority {
    cert_chain: CertChain,
    valid_for: ValidFor,
}

#[derive(Deserialize)]
struct CertChain {
    certificates: Vec<RawBytes>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBytes {
    raw_bytes: String,
}

/// "2022-04-13T20:06:15Z" (with or without fractions) to seconds since the epoch.
fn rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || !s.ends_with('Z') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

fn within(v: &ValidFor, t: i64) -> bool {
    let start = rfc3339(&v.start).unwrap_or(i64::MAX);
    let end = v.end.as_deref().and_then(rfc3339).unwrap_or(i64::MAX);
    start <= t && t <= end
}

fn trusted_root() -> TrustedRoot {
    serde_json::from_str(TRUSTED_ROOT).expect("the built-in Sigstore trust root parses")
}

// ---------------------------------------------------------------- bundle

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    media_type: String,
    verification_material: VerificationMaterial,
    #[serde(default)]
    message_signature: Option<MessageSignature>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerificationMaterial {
    /// v0.3 bundles.
    #[serde(default)]
    certificate: Option<RawBytes>,
    /// v0.1 and v0.2 bundles: the leaf first.
    #[serde(default)]
    x509_certificate_chain: Option<CertChain>,
    #[serde(default)]
    tlog_entries: Vec<TlogEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TlogEntry {
    log_index: String,
    log_id: LogId,
    kind_version: KindVersion,
    integrated_time: String,
    #[serde(default)]
    inclusion_promise: Option<InclusionPromise>,
    canonicalized_body: String,
}

#[derive(Deserialize)]
struct KindVersion {
    kind: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InclusionPromise {
    signed_entry_timestamp: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessageSignature {
    message_digest: MessageDigest,
    signature: String,
}

#[derive(Deserialize)]
struct MessageDigest {
    algorithm: String,
    digest: String,
}

/// The logged entry: a hashedrekord.
#[derive(Deserialize)]
struct Rekord {
    kind: String,
    spec: RekordSpec,
}

#[derive(Deserialize)]
struct RekordSpec {
    data: RekordData,
    signature: RekordSignature,
}

#[derive(Deserialize)]
struct RekordData {
    hash: RekordHash,
}

#[derive(Deserialize)]
struct RekordHash {
    algorithm: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RekordSignature {
    content: String,
    public_key: RekordKey,
}

#[derive(Deserialize)]
struct RekordKey {
    content: String,
}

fn b64(s: &str, what: &str) -> Result<Vec<u8>> {
    STANDARD
        .decode(s.trim())
        .map_err(|_| bad(format!("{what} isn't valid base64")))
}

// ------------------------------------------------------------ signatures

/// The ring algorithm for an EC key (from its SPKI) and a signature algorithm.
fn ecdsa(
    spki: &x509_cert::spki::SubjectPublicKeyInfoOwned,
    sig_alg: ObjectIdentifier,
) -> Option<(&'static dyn VerificationAlgorithm, Vec<u8>)> {
    if spki.algorithm.oid != OID_EC {
        return None;
    }
    let curve: ObjectIdentifier = spki.algorithm.parameters.as_ref()?.decode_as().ok()?;
    let alg: &'static dyn VerificationAlgorithm = match (curve, sig_alg) {
        (c, s) if c == OID_P256 && s == OID_ECDSA_SHA256 => &signature::ECDSA_P256_SHA256_ASN1,
        (c, s) if c == OID_P256 && s == OID_ECDSA_SHA384 => &signature::ECDSA_P256_SHA384_ASN1,
        (c, s) if c == OID_P384 && s == OID_ECDSA_SHA384 => &signature::ECDSA_P384_SHA384_ASN1,
        (c, s) if c == OID_P384 && s == OID_ECDSA_SHA256 => &signature::ECDSA_P384_SHA256_ASN1,
        _ => return None,
    };
    Some((alg, spki.subject_public_key.raw_bytes().to_vec()))
}

fn verify_sig(
    spki: &x509_cert::spki::SubjectPublicKeyInfoOwned,
    sig_alg: ObjectIdentifier,
    message: &[u8],
    sig: &[u8],
) -> bool {
    match ecdsa(spki, sig_alg) {
        Some((alg, key)) => UnparsedPublicKey::new(alg, key)
            .verify(message, sig)
            .is_ok(),
        None => false,
    }
}

/// `child` was signed by `parent`'s key.
fn signed_by(child: &Certificate, parent: &Certificate) -> bool {
    let Ok(tbs) = child.tbs_certificate().to_der() else {
        return false;
    };
    let Some(sig) = child.signature().as_bytes() else {
        return false;
    };
    child.tbs_certificate().issuer() == parent.tbs_certificate().subject()
        && verify_sig(
            parent.tbs_certificate().subject_public_key_info(),
            child.signature_algorithm().oid,
            &tbs,
            sig,
        )
}

fn valid_at(c: &Certificate, t: i64) -> bool {
    let v = c.tbs_certificate().validity();
    let from = v.not_before.to_unix_duration().as_secs() as i64;
    let to = v.not_after.to_unix_duration().as_secs() as i64;
    from <= t && t <= to
}

fn is_ca(c: &Certificate) -> bool {
    matches!(
        c.tbs_certificate().get_extension::<BasicConstraints>(),
        Ok(Some((_, bc))) if bc.ca
    )
}

/// The leaf chains, at `t`, to one of the trusted Fulcio chains.
fn chains_to_fulcio(leaf: &Certificate, root: &TrustedRoot, t: i64) -> bool {
    root.certificate_authorities.iter().any(|ca| {
        if !within(&ca.valid_for, t) {
            return false;
        }
        let chain: Option<Vec<Certificate>> = ca
            .cert_chain
            .certificates
            .iter()
            .map(|c| {
                let der = STANDARD.decode(&c.raw_bytes).ok()?;
                Certificate::from_der(&der).ok()
            })
            .collect();
        let Some(chain) = chain else { return false };
        let Some(first) = chain.first() else {
            return false;
        };
        // leaf <- intermediate <- ... <- root (the last one is the anchor).
        signed_by(leaf, first)
            && chain.iter().all(|c| is_ca(c) && valid_at(c, t))
            && chain.windows(2).all(|w| signed_by(&w[0], &w[1]))
    })
}

// ---------------------------------------------------------- the identity

fn check_identity(leaf: &Certificate, who: &Identity) -> Result<()> {
    let tbs = leaf.tbs_certificate();
    let eku = tbs
        .get_extension::<ExtendedKeyUsage>()
        .map_err(|_| bad("its certificate's key usage can't be read"))?;
    if !eku.is_some_and(|(_, e)| e.0.contains(&OID_CODE_SIGNING)) {
        return Err(bad("its certificate isn't for code signing"));
    }
    let san = tbs
        .get_extension::<SubjectAltName>()
        .map_err(|_| bad("its certificate's names can't be read"))?
        .ok_or_else(|| bad("its certificate names no one"))?;
    let names: Vec<String> = san
        .1
        .0
        .iter()
        .filter_map(|n| match n {
            GeneralName::UniformResourceIdentifier(u) => Some(u.to_string()),
            GeneralName::Rfc822Name(e) => Some(e.to_string()),
            _ => None,
        })
        .collect();
    if !names.iter().any(|n| n == who.name) {
        return Err(bad(format!(
            "it was signed by {}, not {}",
            names.first().map_or("no one", String::as_str),
            who.name
        )));
    }
    let mut issuer = None;
    for ext in tbs.extensions().into_iter().flatten() {
        if ext.extn_id == OID_ISSUER_V2 {
            issuer = Utf8StringRef::from_der(ext.extn_value.as_bytes())
                .ok()
                .map(|s| s.as_str().to_string());
        } else if ext.extn_id == OID_ISSUER_V1 && issuer.is_none() {
            issuer = String::from_utf8(ext.extn_value.as_bytes().to_vec()).ok();
        }
    }
    match issuer {
        Some(i) if i == who.issuer => Ok(()),
        Some(i) => Err(bad(format!(
            "its identity was vouched for by {i}, not {}",
            who.issuer
        ))),
        None => Err(bad(
            "its certificate doesn't say who vouched for the identity",
        )),
    }
}

// ------------------------------------------------------------ the check

/// Check `bundle` (the `.sigstore.json` file) for `artifact`, signed by `who`.
pub fn verify(bundle: &[u8], artifact: &[u8], who: &Identity) -> Result<Verified> {
    let digest: [u8; 32] = Sha256::digest(artifact).into();
    let bundle: Bundle =
        serde_json::from_slice(bundle).map_err(|e| bad(format!("it isn't a bundle ({e})")))?;
    if !bundle
        .media_type
        .starts_with("application/vnd.dev.sigstore.bundle")
    {
        return Err(bad("it isn't a Sigstore bundle"));
    }
    let root = trusted_root();
    let vm = &bundle.verification_material;

    let leaf_der = match (&vm.certificate, &vm.x509_certificate_chain) {
        (Some(c), _) => b64(&c.raw_bytes, "the certificate")?,
        (None, Some(chain)) => b64(
            &chain
                .certificates
                .first()
                .ok_or_else(|| bad("it has no certificate"))?
                .raw_bytes,
            "the certificate",
        )?,
        (None, None) => return Err(bad("it has no certificate (a key-based signature?)")),
    };
    let leaf =
        Certificate::from_der(&leaf_der).map_err(|_| bad("its certificate can't be read"))?;

    let sig = bundle
        .message_signature
        .as_ref()
        .ok_or_else(|| bad("it signs an attestation, not a file"))?;
    if sig.message_digest.algorithm != "SHA2_256"
        || b64(&sig.message_digest.digest, "the digest")? != digest
    {
        return Err(bad("it's for a different file"));
    }
    let signature = b64(&sig.signature, "the signature")?;

    // Rekor v1: a hashedrekord with a signed entry timestamp.
    let [entry] = vm.tlog_entries.as_slice() else {
        return Err(bad("it should have exactly one transparency log entry"));
    };
    if entry.kind_version.kind != "hashedrekord" || entry.kind_version.version != "0.0.1" {
        return Err(bad(format!(
            "its log entry is a {} {}, which this can't check",
            entry.kind_version.kind, entry.kind_version.version
        )));
    }
    let promise = entry.inclusion_promise.as_ref().ok_or_else(|| {
        bad("its log entry has no signed entry timestamp (Rekor v2 bundles can't be checked yet)")
    })?;
    let integrated_time: i64 = entry
        .integrated_time
        .parse()
        .map_err(|_| bad("its log time can't be read"))?;
    let log_index: i64 = entry
        .log_index
        .parse()
        .map_err(|_| bad("its log index can't be read"))?;

    // 1. The certificate, as it was when the signature was logged.
    if !valid_at(&leaf, integrated_time) {
        return Err(bad(
            "its certificate wasn't valid when the signature was logged",
        ));
    }
    if !chains_to_fulcio(&leaf, &root, integrated_time) {
        return Err(bad("its certificate wasn't issued by Sigstore"));
    }
    // 2. Who it names.
    check_identity(&leaf, who)?;
    // 3. The signature over the file, with the certificate's key.
    if !verify_sig(
        leaf.tbs_certificate().subject_public_key_info(),
        OID_ECDSA_SHA256,
        artifact,
        &signature,
    ) {
        return Err(bad("the signature over the file doesn't verify"));
    }
    // 4. The log entry: Rekor's promise, and what was logged.
    let log_id = b64(&entry.log_id.key_id, "the log id")?;
    let tlog = root
        .tlogs
        .iter()
        .find(|t| STANDARD.decode(&t.log_id.key_id).ok().as_deref() == Some(&log_id[..]))
        .ok_or_else(|| bad("it was logged by a transparency log this doesn't trust"))?;
    if tlog.public_key.key_details != "PKIX_ECDSA_P256_SHA_256"
        || !within(&tlog.public_key.valid_for, integrated_time)
    {
        return Err(bad("its transparency log key doesn't cover it"));
    }
    let tlog_spki = x509_cert::spki::SubjectPublicKeyInfoOwned::from_der(&b64(
        &tlog.public_key.raw_bytes,
        "the log key",
    )?)
    .map_err(|_| bad("the log key can't be read"))?;
    // The promise signs the entry as canonical JSON (keys in order, no spaces).
    let set_payload = format!(
        r#"{{"body":"{}","integratedTime":{},"logID":"{}","logIndex":{}}}"#,
        entry.canonicalized_body,
        integrated_time,
        log_id
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        log_index
    );
    if !verify_sig(
        &tlog_spki,
        OID_ECDSA_SHA256,
        set_payload.as_bytes(),
        &b64(
            &promise.signed_entry_timestamp,
            "the signed entry timestamp",
        )?,
    ) {
        return Err(bad("the transparency log's signature on it doesn't verify"));
    }
    let body: Rekord = serde_json::from_slice(&b64(&entry.canonicalized_body, "the log entry")?)
        .map_err(|_| bad("its log entry can't be read"))?;
    let logged_cert = pem_der(&b64(
        &body.spec.signature.public_key.content,
        "the logged key",
    )?)
    .ok_or_else(|| bad("its log entry holds no certificate"))?;
    if body.kind != "hashedrekord"
        || body.spec.data.hash.algorithm != "sha256"
        || body.spec.data.hash.value != hex(&digest)
        || b64(&body.spec.signature.content, "the logged signature")? != signature
        || logged_cert != leaf_der
    {
        return Err(bad(
            "what was logged isn't this signature, certificate and file",
        ));
    }
    Ok(Verified {
        integrated_time,
        log_index,
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The DER inside a PEM certificate.
fn pem_der(pem: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(pem).ok()?;
    let body = text
        .trim()
        .strip_prefix("-----BEGIN CERTIFICATE-----")?
        .strip_suffix("-----END CERTIFICATE-----")?;
    let joined: String = body.split_whitespace().collect();
    STANDARD.decode(joined).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(rfc3339("2022-04-13T20:06:15Z"), Some(1_649_880_375));
        assert_eq!(rfc3339("2022-12-31T23:59:59.999Z"), Some(1_672_531_199));
        assert_eq!(rfc3339("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        for bad in [
            "",
            "2022-13-01T00:00:00Z",
            "2022-04-13 20:06:15Z",
            "yesterday",
            "2022-04-13T20:06:15",
        ] {
            assert_eq!(rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn built_in_root_parses() {
        let r = trusted_root();
        assert!(
            r.tlogs
                .iter()
                .any(|t| t.public_key.key_details == "PKIX_ECDSA_P256_SHA_256")
        );
        assert!(!r.certificate_authorities.is_empty());
    }
}
