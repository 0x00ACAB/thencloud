//! The S3 side of blob storage: a client built from the command line, and
//! the handful of operations the blob store and backups need.
//!
//! Only ciphertext ever reaches S3 (the same bytes the local store would
//! hold), so the bucket learns nothing the filesystem wouldn't.
//!
//! Requests are path-style (`endpoint/bucket/key`), which is what MinIO,
//! Ceph, Garage and friends expect; AWS supports it too. Checksums are only
//! sent when the protocol requires them, because trailing-checksum uploads
//! are the one thing S3-compatible servers keep getting wrong.

use std::io;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::{
    BehaviorVersion, Credentials, Region, RequestChecksumCalculation, ResponseChecksumValidation,
};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_smithy_http_client::tls::Provider;
use aws_smithy_http_client::tls::rustls_provider::CryptoMode;

use crate::Config;

/// One bucket (plus a key prefix) the server talks to.
#[derive(Debug, Clone)]
pub struct S3Target {
    pub client: Client,
    pub bucket: String,
    /// Normalised: empty, or ends with '/'.
    pub prefix: String,
}

/// Build the blob-store target from the command line, or `None` when S3
/// isn't configured. The S3 options are all-or-nothing.
pub fn target_from_config(cfg: &Config) -> Result<Option<S3Target>, String> {
    let set = [
        ("--s3-endpoint", cfg.s3_endpoint.is_some()),
        ("--s3-bucket", cfg.s3_bucket.is_some()),
        ("--s3-access-key", cfg.s3_access_key.is_some()),
        ("--s3-secret-key", cfg.s3_secret_key.is_some()),
    ];
    let given: Vec<&str> = set
        .iter()
        .filter(|(_, on)| *on)
        .map(|(name, _)| *name)
        .collect();
    if given.is_empty() {
        return Ok(None);
    }
    if given.len() < set.len() {
        let missing: Vec<&str> = set
            .iter()
            .filter(|(_, on)| !*on)
            .map(|(name, _)| *name)
            .collect();
        return Err(format!(
            "S3 is half-configured: {} set, {} missing",
            given.join(", "),
            missing.join(", ")
        ));
    }
    Ok(Some(target(
        cfg.s3_endpoint.as_deref().expect("checked above"),
        &cfg.s3_region,
        cfg.s3_bucket.clone().expect("checked above"),
        cfg.s3_access_key.as_deref().expect("checked above"),
        cfg.s3_secret_key.as_deref().expect("checked above"),
        &cfg.s3_prefix,
    )))
}

/// A client for one bucket. Building it doesn't touch the network.
pub fn target(
    endpoint: &str,
    region: &str,
    bucket: String,
    access_key: &str,
    secret_key: &str,
    prefix: &str,
) -> S3Target {
    let https = aws_smithy_http_client::Builder::new()
        .tls_provider(Provider::Rustls(CryptoMode::Ring))
        .build_https();
    let conf = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .http_client(https)
        .endpoint_url(endpoint)
        .region(Region::new(region.to_string()))
        .credentials_provider(Credentials::new(
            access_key,
            secret_key,
            None,
            None,
            "thencloud",
        ))
        .force_path_style(true)
        .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
        .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
        .build();
    let mut prefix = prefix.to_string();
    if !prefix.is_empty() && !prefix.ends_with('/') {
        prefix.push('/');
    }
    S3Target {
        client: Client::from_conf(conf),
        bucket,
        prefix,
    }
}

/// Is this SDK error a missing object (404 / NoSuchKey)?
fn is_missing<E: std::fmt::Debug>(e: &SdkError<E>) -> bool {
    match e {
        SdkError::ServiceError(se) => se.raw().status().as_u16() == 404,
        SdkError::ResponseError(r) => r.raw().status().as_u16() == 404,
        _ => false,
    }
}

fn other<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}

impl S3Target {
    pub fn key(&self, rest: &str) -> String {
        format!("{}{}", self.prefix, rest)
    }

    pub async fn put(&self, key: &str, data: &[u8]) -> io::Result<()> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(data.to_vec().into())
            .send()
            .await
            .map_err(other)?;
        Ok(())
    }

    /// The object's bytes; `NotFound` when the key doesn't exist.
    pub async fn get(&self, key: &str) -> io::Result<Vec<u8>> {
        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => Ok(out
                .body
                .collect()
                .await
                .map_err(other)?
                .into_bytes()
                .to_vec()),
            Err(e) if is_missing(&e) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no such key: {key}"),
            )),
            Err(e) => Err(other(e)),
        }
    }

    /// The object's size; `NotFound` when the key doesn't exist.
    pub async fn size(&self, key: &str) -> io::Result<u64> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => Ok(out.content_length().unwrap_or(0).max(0) as u64),
            Err(e) if is_missing(&e) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("no such key: {key}"),
            )),
            Err(e) => Err(other(e)),
        }
    }

    pub async fn delete(&self, key: &str) -> io::Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(other)?;
        Ok(())
    }

    /// Delete keys in batches of 1000 (S3's limit). A missing key is not
    /// an error, matching the local store's behaviour.
    pub async fn delete_many(&self, keys: &[String]) -> io::Result<()> {
        for batch in keys.chunks(1000) {
            let mut delete = Delete::builder();
            for k in batch {
                delete = delete.objects(ObjectIdentifier::builder().key(k).build().map_err(other)?);
            }
            self.client
                .delete_objects()
                .bucket(&self.bucket)
                .delete(delete.build().map_err(other)?)
                .send()
                .await
                .map_err(other)?;
        }
        Ok(())
    }

    /// Every key under `prefix`, in order, following pagination.
    pub async fn list(&self, prefix: &str) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);
            if let Some(t) = token {
                req = req.continuation_token(t);
            }
            let page = req.send().await.map_err(other)?;
            for o in page.contents() {
                if let Some(k) = o.key() {
                    out.push(k.to_string());
                }
            }
            match (page.is_truncated(), page.next_continuation_token()) {
                (Some(true), Some(t)) => token = Some(t.to_string()),
                _ => break,
            }
        }
        Ok(out)
    }

    /// The "directories" under `prefix`: the common prefixes one level down.
    pub async fn list_dirs(&self, prefix: &str) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix)
                .delimiter("/");
            if let Some(t) = token {
                req = req.continuation_token(t);
            }
            let page = req.send().await.map_err(other)?;
            for p in page.common_prefixes() {
                if let Some(k) = p.prefix() {
                    out.push(k.to_string());
                }
            }
            match (page.is_truncated(), page.next_continuation_token()) {
                (Some(true), Some(t)) => token = Some(t.to_string()),
                _ => break,
            }
        }
        Ok(out)
    }

    /// Is there anything at all under `prefix`?
    pub async fn is_empty(&self, prefix: &str) -> io::Result<bool> {
        let out = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(prefix)
            .max_keys(1)
            .send()
            .await
            .map_err(other)?;
        Ok(out.contents().is_empty() && out.common_prefixes().is_empty())
    }
}
