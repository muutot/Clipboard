use std::{collections::BTreeSet, path::Path};

use crate::sync::s3::{
    delete_from_s3, get_s3_object, get_s3_object_to_file, head_s3_object, list_s3_objects_after,
    put_s3_file, put_s3_object, S3PutCondition, S3PutOutcome,
};

use super::layout::obsolete_object_candidate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectInfo {
    /// Key relative to the configured sync prefix.
    pub key: String,
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
    /// Raw ETag returned by object listing, including quotes when supplied.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedObject {
    pub bytes: Vec<u8>,
    /// Raw HTTP ETag value, including quotes when supplied by the store.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMetadata {
    pub size_bytes: Option<u64>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedFile {
    pub size_bytes: u64,
    pub sha256: String,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PutCondition {
    #[default]
    Unconditional,
    IfAbsent,
    /// Raw ETag returned by a previous read/write.
    IfMatch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutOutcome {
    Stored { etag: Option<String> },
    PreconditionFailed,
}

pub trait ObjectStore {
    /// Lists keys relative to this store's configured remote scope.
    fn list(&self, prefix: &str, start_after: Option<&str>) -> Result<Vec<ObjectInfo>, String>;
    fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String>;
    fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String>;
    fn get_to_file(
        &self,
        key: &str,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<Option<DownloadedFile>, String>;
    fn put(&self, key: &str, bytes: Vec<u8>, condition: PutCondition)
        -> Result<PutOutcome, String>;
    fn put_file(
        &self,
        key: &str,
        path: &Path,
        sha256: &str,
        size_bytes: u64,
        condition: PutCondition,
    ) -> Result<PutOutcome, String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// S3 adapter whose public keys are always relative to one configured sync
/// prefix. Credentials remain private and are never exposed through Debug.
pub struct S3ObjectStore {
    endpoint: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    remote_prefix: String,
}

impl S3ObjectStore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint: impl Into<String>,
        region: impl Into<String>,
        bucket: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        remote_prefix: impl AsRef<str>,
    ) -> Result<Self, String> {
        let endpoint = endpoint.into();
        let region = region.into();
        let bucket = bucket.into();
        let access_key = access_key.into();
        let secret_key = secret_key.into();
        if endpoint.trim().is_empty() {
            return Err("S3 endpoint is empty".to_string());
        }
        if region.trim().is_empty() {
            return Err("S3 region is empty".to_string());
        }
        if bucket.trim().is_empty() {
            return Err("S3 bucket is empty".to_string());
        }
        if access_key.is_empty() || secret_key.is_empty() {
            return Err("S3 credentials are incomplete".to_string());
        }
        Ok(Self {
            endpoint,
            region,
            bucket,
            access_key,
            secret_key,
            remote_prefix: normalize_remote_prefix(remote_prefix.as_ref())?,
        })
    }

    pub fn remote_prefix(&self) -> &str {
        &self.remote_prefix
    }

    fn full_object_key(&self, relative_key: &str) -> Result<String, String> {
        validate_object_key(relative_key)?;
        Ok(self.join_relative(relative_key))
    }

    fn full_list_prefix(&self, relative_prefix: &str) -> Result<String, String> {
        validate_list_prefix(relative_prefix)?;
        if relative_prefix.is_empty() {
            Ok(if self.remote_prefix.is_empty() {
                String::new()
            } else {
                format!("{}/", self.remote_prefix)
            })
        } else {
            Ok(self.join_relative(relative_prefix))
        }
    }

    fn join_relative(&self, relative: &str) -> String {
        if self.remote_prefix.is_empty() {
            relative.to_string()
        } else {
            format!("{}/{relative}", self.remote_prefix)
        }
    }

    fn relative_object_key(&self, full_key: &str) -> Result<String, String> {
        if self.remote_prefix.is_empty() {
            return Ok(full_key.to_string());
        }
        let scope_prefix = format!("{}/", self.remote_prefix);
        full_key
            .strip_prefix(&scope_prefix)
            .filter(|relative| !relative.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| {
                format!(
                    "S3 returned object outside configured sync prefix {:?}",
                    self.remote_prefix
                )
            })
    }
}

impl ObjectStore for S3ObjectStore {
    fn list(&self, prefix: &str, start_after: Option<&str>) -> Result<Vec<ObjectInfo>, String> {
        let full_prefix = self.full_list_prefix(prefix)?;
        let full_start_after = start_after
            .map(|cursor| {
                validate_object_key(cursor)?;
                if !prefix.is_empty() && !cursor.starts_with(prefix) {
                    return Err("S3 start-after key is outside the requested prefix".to_string());
                }
                Ok(self.join_relative(cursor))
            })
            .transpose()?;
        list_s3_objects_after(
            &self.endpoint,
            &self.region,
            &self.bucket,
            (!full_prefix.is_empty()).then_some(full_prefix.as_str()),
            full_start_after.as_deref(),
            &self.access_key,
            &self.secret_key,
        )?
        .into_iter()
        .map(|entry| {
            Ok(ObjectInfo {
                key: self.relative_object_key(&entry.object_key)?,
                size_bytes: entry.size_bytes,
                modified_ms: entry.modified_ms,
                etag: entry.etag,
            })
        })
        .collect()
    }

    fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
        let full_key = self.full_object_key(key)?;
        get_s3_object(
            &self.endpoint,
            &self.region,
            &self.bucket,
            &full_key,
            &self.access_key,
            &self.secret_key,
        )
        .map(|object| {
            object.map(|object| DownloadedObject {
                bytes: object.bytes,
                etag: object.etag,
            })
        })
    }

    fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
        let full_key = self.full_object_key(key)?;
        head_s3_object(
            &self.endpoint,
            &self.region,
            &self.bucket,
            &full_key,
            &self.access_key,
            &self.secret_key,
        )
        .map(|metadata| {
            metadata.map(|metadata| ObjectMetadata {
                size_bytes: metadata.size_bytes,
                etag: metadata.etag,
            })
        })
    }

    fn get_to_file(
        &self,
        key: &str,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<Option<DownloadedFile>, String> {
        let full_key = self.full_object_key(key)?;
        get_s3_object_to_file(
            &self.endpoint,
            &self.region,
            &self.bucket,
            &full_key,
            destination,
            max_bytes,
            &self.access_key,
            &self.secret_key,
        )
        .map(|download| {
            download.map(|download| DownloadedFile {
                size_bytes: download.size_bytes,
                sha256: download.sha256,
                etag: download.etag,
            })
        })
    }

    fn put(
        &self,
        key: &str,
        bytes: Vec<u8>,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        let full_key = self.full_object_key(key)?;
        let condition = match condition {
            PutCondition::Unconditional => S3PutCondition::Unconditional,
            PutCondition::IfAbsent => S3PutCondition::IfAbsent,
            PutCondition::IfMatch(etag) => S3PutCondition::IfMatch(etag),
        };
        put_s3_object(
            &self.endpoint,
            &self.region,
            &self.bucket,
            &full_key,
            bytes,
            &self.access_key,
            &self.secret_key,
            condition,
        )
        .map(|outcome| match outcome {
            S3PutOutcome::Stored { etag } => PutOutcome::Stored { etag },
            S3PutOutcome::PreconditionFailed => PutOutcome::PreconditionFailed,
        })
    }

    fn put_file(
        &self,
        key: &str,
        path: &Path,
        sha256: &str,
        size_bytes: u64,
        condition: PutCondition,
    ) -> Result<PutOutcome, String> {
        let full_key = self.full_object_key(key)?;
        let condition = match condition {
            PutCondition::Unconditional => S3PutCondition::Unconditional,
            PutCondition::IfAbsent => S3PutCondition::IfAbsent,
            PutCondition::IfMatch(etag) => S3PutCondition::IfMatch(etag),
        };
        put_s3_file(
            &self.endpoint,
            &self.region,
            &self.bucket,
            &full_key,
            path,
            sha256,
            size_bytes,
            &self.access_key,
            &self.secret_key,
            condition,
        )
        .map(|outcome| match outcome {
            S3PutOutcome::Stored { etag } => PutOutcome::Stored { etag },
            S3PutOutcome::PreconditionFailed => PutOutcome::PreconditionFailed,
        })
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        let full_key = self.full_object_key(key)?;
        delete_from_s3(
            &self.endpoint,
            &self.region,
            &self.bucket,
            &full_key,
            &self.access_key,
            &self.secret_key,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObsoleteCleanupReport {
    pub scanned_objects: u64,
    pub deleted_objects: u64,
}

/// Deletes only objects selected by the strict obsolete-layout predicate. The
/// caller marks local cleanup state only after this function returns success.
pub fn cleanup_obsolete_objects(store: &impl ObjectStore) -> Result<ObsoleteCleanupReport, String> {
    let listed = store.list("", None)?;
    let scanned_objects = listed.len() as u64;
    let candidates: BTreeSet<String> = listed
        .into_iter()
        .map(|entry| entry.key)
        .filter(|key| obsolete_object_candidate(key))
        .collect();

    for key in &candidates {
        store
            .delete(key)
            .map_err(|error| format!("failed to delete obsolete sync object {key:?}: {error}"))?;
    }
    Ok(ObsoleteCleanupReport {
        scanned_objects,
        deleted_objects: candidates.len() as u64,
    })
}

fn normalize_remote_prefix(value: &str) -> Result<String, String> {
    let prefix = value.trim_matches('/');
    if prefix.is_empty() {
        return Ok(String::new());
    }
    validate_key_components(prefix, "S3 remote prefix")?;
    Ok(prefix.to_string())
}

fn validate_object_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.starts_with('/') || key.ends_with('/') {
        return Err("S3 object key must be a non-empty relative file key".to_string());
    }
    validate_key_components(key, "S3 object key")
}

fn validate_list_prefix(prefix: &str) -> Result<(), String> {
    if prefix.is_empty() {
        return Ok(());
    }
    if prefix.starts_with('/') {
        return Err("S3 list prefix must be relative".to_string());
    }
    let without_trailing_slash = prefix.strip_suffix('/').unwrap_or(prefix);
    if without_trailing_slash.is_empty() {
        return Err("S3 list prefix cannot target the parent namespace".to_string());
    }
    validate_key_components(without_trailing_slash, "S3 list prefix")
}

fn validate_key_components(value: &str, label: &str) -> Result<(), String> {
    if value.contains('\\') || value.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(format!("{label} contains unsafe characters"));
    }
    if value
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(format!("{label} contains an unsafe path component"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use sha2::{Digest, Sha256};

    use super::*;

    #[derive(Default)]
    struct MemoryObjectStore {
        objects: RefCell<BTreeMap<String, Vec<u8>>>,
        deleted: RefCell<Vec<String>>,
        fail_delete: RefCell<Option<String>>,
    }

    impl MemoryObjectStore {
        fn with_keys(keys: &[&str]) -> Self {
            Self {
                objects: RefCell::new(
                    keys.iter()
                        .map(|key| ((*key).to_string(), key.as_bytes().to_vec()))
                        .collect(),
                ),
                ..Self::default()
            }
        }

        fn etag(bytes: &[u8]) -> String {
            format!("\"{}\"", hex::encode(Sha256::digest(bytes)))
        }
    }

    impl ObjectStore for MemoryObjectStore {
        fn list(&self, prefix: &str, start_after: Option<&str>) -> Result<Vec<ObjectInfo>, String> {
            Ok(self
                .objects
                .borrow()
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .filter(|(key, _)| start_after.is_none_or(|cursor| key.as_str() > cursor))
                .map(|(key, bytes)| ObjectInfo {
                    key: key.clone(),
                    size_bytes: Some(bytes.len() as u64),
                    modified_ms: None,
                    etag: Some(Self::etag(bytes)),
                })
                .collect())
        }

        fn get(&self, key: &str) -> Result<Option<DownloadedObject>, String> {
            Ok(self
                .objects
                .borrow()
                .get(key)
                .cloned()
                .map(|bytes| DownloadedObject {
                    etag: Some(Self::etag(&bytes)),
                    bytes,
                }))
        }

        fn head(&self, key: &str) -> Result<Option<ObjectMetadata>, String> {
            Ok(self.objects.borrow().get(key).map(|bytes| ObjectMetadata {
                size_bytes: Some(bytes.len() as u64),
                etag: Some(Self::etag(bytes)),
            }))
        }

        fn get_to_file(
            &self,
            key: &str,
            destination: &Path,
            max_bytes: u64,
        ) -> Result<Option<DownloadedFile>, String> {
            let Some(bytes) = self.objects.borrow().get(key).cloned() else {
                return Ok(None);
            };
            if bytes.len() as u64 > max_bytes {
                return Err("memory object exceeds download limit".to_string());
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|error| error.to_string())?;
            use std::io::Write;
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            Ok(Some(DownloadedFile {
                size_bytes: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
                etag: Some(Self::etag(&bytes)),
            }))
        }

        fn put(
            &self,
            key: &str,
            bytes: Vec<u8>,
            condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            let mut objects = self.objects.borrow_mut();
            let existing = objects.get(key);
            let allowed = match condition {
                PutCondition::Unconditional => true,
                PutCondition::IfAbsent => existing.is_none(),
                PutCondition::IfMatch(expected) => {
                    existing.is_some_and(|bytes| Self::etag(bytes) == expected)
                }
            };
            if !allowed {
                return Ok(PutOutcome::PreconditionFailed);
            }
            let etag = Self::etag(&bytes);
            objects.insert(key.to_string(), bytes);
            Ok(PutOutcome::Stored { etag: Some(etag) })
        }

        fn put_file(
            &self,
            key: &str,
            path: &Path,
            sha256: &str,
            size_bytes: u64,
            condition: PutCondition,
        ) -> Result<PutOutcome, String> {
            let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
            if bytes.len() as u64 != size_bytes || hex::encode(Sha256::digest(&bytes)) != sha256 {
                return Err("memory file fingerprint mismatch".to_string());
            }
            self.put(key, bytes, condition)
        }

        fn delete(&self, key: &str) -> Result<(), String> {
            if self.fail_delete.borrow().as_deref() == Some(key) {
                return Err("injected delete failure".to_string());
            }
            self.objects.borrow_mut().remove(key);
            self.deleted.borrow_mut().push(key.to_string());
            Ok(())
        }
    }

    #[test]
    fn s3_store_joins_only_valid_keys_below_its_scope() {
        let store = S3ObjectStore::new(
            "http://127.0.0.1:9000",
            "us-east-1",
            "clipboard",
            "access",
            "secret",
            "/team/clipboard/",
        )
        .unwrap();

        assert_eq!(store.remote_prefix(), "team/clipboard");
        assert_eq!(
            store.full_object_key("v1/heads/device.bin").unwrap(),
            "team/clipboard/v1/heads/device.bin"
        );
        assert_eq!(
            store.full_list_prefix("v1/segments/").unwrap(),
            "team/clipboard/v1/segments/"
        );
        assert!(store.full_object_key("../v1/checkpoint.bin").is_err());
        assert!(store.full_object_key("/v1/checkpoint.bin").is_err());
        assert!(store.full_list_prefix("v1//heads/").is_err());
        assert!(S3ObjectStore::new(
            "http://127.0.0.1:9000",
            "us-east-1",
            "clipboard",
            "access",
            "secret",
            "team/../other",
        )
        .is_err());
    }

    #[test]
    fn cleanup_deletes_only_exact_obsolete_layout_objects() {
        let store = MemoryObjectStore::with_keys(&[
            "baseline-device.zip",
            "oplog-device-s1-e2-hash",
            "resources/image/a.png",
            "resources/file/nested/a.bin",
            "resources/../v1/heads/device.bin",
            "nested/baseline-device.zip",
            "v1/heads/device.bin",
            "v1/resources/image/a.png",
            "unrelated.txt",
        ]);

        let report = cleanup_obsolete_objects(&store).unwrap();

        assert_eq!(report.scanned_objects, 9);
        assert_eq!(report.deleted_objects, 4);
        assert_eq!(
            *store.deleted.borrow(),
            vec![
                "baseline-device.zip",
                "oplog-device-s1-e2-hash",
                "resources/file/nested/a.bin",
                "resources/image/a.png",
            ]
        );
        let remaining = store.objects.borrow();
        assert!(remaining.contains_key("v1/heads/device.bin"));
        assert!(remaining.contains_key("v1/resources/image/a.png"));
        assert!(remaining.contains_key("nested/baseline-device.zip"));
        assert!(remaining.contains_key("resources/../v1/heads/device.bin"));
    }

    #[test]
    fn cleanup_failure_is_explicit_and_retryable() {
        let store = MemoryObjectStore::with_keys(&[
            "baseline-device.zip",
            "resources/image/a.png",
            "v1/heads/device.bin",
        ]);
        *store.fail_delete.borrow_mut() = Some("resources/image/a.png".to_string());

        let error = cleanup_obsolete_objects(&store).unwrap_err();

        assert!(error.contains("resources/image/a.png"));
        assert!(store.objects.borrow().contains_key("resources/image/a.png"));
        assert!(store.objects.borrow().contains_key("v1/heads/device.bin"));
        assert!(!store.objects.borrow().contains_key("baseline-device.zip"));
        *store.fail_delete.borrow_mut() = None;
        let retried = cleanup_obsolete_objects(&store).unwrap();
        assert_eq!(retried.deleted_objects, 1);
    }

    #[test]
    fn memory_store_models_conditional_pointer_updates() {
        let store = MemoryObjectStore::default();
        let created = store
            .put("v1/checkpoint.bin", b"one".to_vec(), PutCondition::IfAbsent)
            .unwrap();
        let PutOutcome::Stored { etag } = created else {
            panic!("initial conditional write was rejected");
        };
        let etag = etag.unwrap();
        assert_eq!(
            store
                .put("v1/checkpoint.bin", b"two".to_vec(), PutCondition::IfAbsent)
                .unwrap(),
            PutOutcome::PreconditionFailed
        );
        assert!(matches!(
            store
                .put(
                    "v1/checkpoint.bin",
                    b"two".to_vec(),
                    PutCondition::IfMatch(etag.clone())
                )
                .unwrap(),
            PutOutcome::Stored { .. }
        ));
        assert_eq!(
            store
                .put(
                    "v1/checkpoint.bin",
                    b"stale".to_vec(),
                    PutCondition::IfMatch(etag)
                )
                .unwrap(),
            PutOutcome::PreconditionFailed
        );
    }
}
