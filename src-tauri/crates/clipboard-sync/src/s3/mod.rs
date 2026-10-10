use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use crate::{cancellation, transport};
use base64::{engine::general_purpose::STANDARD, Engine};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, ETAG};
use reqwest::{Client, RequestBuilder};
use serde::Serialize;
use sha2::{Digest, Sha256};

mod client;
mod listing;
mod objects;
mod signing;

#[cfg(test)]
mod tests;

use client::*;
#[cfg(test)]
use listing::*;
#[cfg(test)]
use objects::*;
use signing::*;

pub use client::MAX_IN_MEMORY_PROTOCOL_OBJECT_BYTES;
pub(crate) use listing::list_s3_objects_after_with_metrics;
pub use listing::{list_s3_objects, list_s3_objects_after};
#[cfg(any(test, feature = "engine-test-support"))]
pub use objects::ensure_test_bucket;
pub use objects::{
    delete_from_s3, download_from_s3, get_s3_object, get_s3_object_to_file, head_s3_object,
    put_s3_file, put_s3_object, test_s3_connection, upload_to_s3,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Entry {
    /// Complete bucket-relative key used by nested v1 namespaces.
    #[serde(skip_serializing)]
    pub object_key: String,
    pub name: String,
    pub is_directory: bool,
    pub size_bytes: Option<u64>,
    pub modified_ms: Option<i64>,
    /// Raw ETag text from ListObjectsV2, including quotes when supplied.
    #[serde(skip_serializing)]
    pub etag: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct S3TestResult {
    pub success: bool,
    pub message: String,
    pub status_code: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3DownloadedObject {
    pub bytes: Vec<u8>,
    /// Raw HTTP ETag value, including quotes when supplied by the server.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3ObjectMetadata {
    pub size_bytes: Option<u64>,
    /// Raw HTTP ETag value, including quotes when supplied by the server.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3DownloadedFile {
    pub size_bytes: u64,
    pub sha256: String,
    /// Raw HTTP ETag value, including quotes when supplied by the server.
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum S3PutCondition {
    #[default]
    Unconditional,
    IfAbsent,
    /// Raw HTTP ETag value returned by a previous GET/PUT.
    IfMatch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3PutOutcome {
    Stored { etag: Option<String> },
    PreconditionFailed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct S3RequestMetricsSnapshot {
    pub put_requests: u64,
    pub get_requests: u64,
    pub head_requests: u64,
    pub list_requests: u64,
    pub delete_requests: u64,
    pub uploaded_bytes: u64,
    pub downloaded_bytes: u64,
    pub put_elapsed_ns: u64,
    pub get_elapsed_ns: u64,
    pub head_elapsed_ns: u64,
    pub list_elapsed_ns: u64,
    pub delete_elapsed_ns: u64,
}

#[derive(Debug, Default)]
struct S3RequestMetricsInner {
    put_requests: AtomicU64,
    get_requests: AtomicU64,
    head_requests: AtomicU64,
    list_requests: AtomicU64,
    delete_requests: AtomicU64,
    uploaded_bytes: AtomicU64,
    downloaded_bytes: AtomicU64,
    put_elapsed_ns: AtomicU64,
    get_elapsed_ns: AtomicU64,
    head_elapsed_ns: AtomicU64,
    list_elapsed_ns: AtomicU64,
    delete_elapsed_ns: AtomicU64,
}

/// Optional transport diagnostics for benchmarks and runtime observability.
/// Clones share the same atomics. A normal `S3ObjectStore` does not allocate or
/// update these counters unless metrics are explicitly attached.
#[derive(Debug, Clone, Default)]
pub struct S3RequestMetrics {
    inner: Arc<S3RequestMetricsInner>,
}

impl S3RequestMetrics {
    pub fn snapshot(&self) -> S3RequestMetricsSnapshot {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        S3RequestMetricsSnapshot {
            put_requests: load(&self.inner.put_requests),
            get_requests: load(&self.inner.get_requests),
            head_requests: load(&self.inner.head_requests),
            list_requests: load(&self.inner.list_requests),
            delete_requests: load(&self.inner.delete_requests),
            uploaded_bytes: load(&self.inner.uploaded_bytes),
            downloaded_bytes: load(&self.inner.downloaded_bytes),
            put_elapsed_ns: load(&self.inner.put_elapsed_ns),
            get_elapsed_ns: load(&self.inner.get_elapsed_ns),
            head_elapsed_ns: load(&self.inner.head_elapsed_ns),
            list_elapsed_ns: load(&self.inner.list_elapsed_ns),
            delete_elapsed_ns: load(&self.inner.delete_elapsed_ns),
        }
    }

    pub fn reset(&self) {
        for counter in [
            &self.inner.put_requests,
            &self.inner.get_requests,
            &self.inner.head_requests,
            &self.inner.list_requests,
            &self.inner.delete_requests,
            &self.inner.uploaded_bytes,
            &self.inner.downloaded_bytes,
            &self.inner.put_elapsed_ns,
            &self.inner.get_elapsed_ns,
            &self.inner.head_elapsed_ns,
            &self.inner.list_elapsed_ns,
            &self.inner.delete_elapsed_ns,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
    }

    pub(crate) fn record_list_page(&self, downloaded_bytes: u64) {
        self.inner.list_requests.fetch_add(1, Ordering::Relaxed);
        self.inner
            .downloaded_bytes
            .fetch_add(downloaded_bytes, Ordering::Relaxed);
    }

    pub(crate) fn record_list_elapsed(&self, elapsed: Duration) {
        add_duration(&self.inner.list_elapsed_ns, elapsed);
    }

    pub(crate) fn record_get(&self, downloaded_bytes: u64, elapsed: Duration) {
        self.inner.get_requests.fetch_add(1, Ordering::Relaxed);
        self.inner
            .downloaded_bytes
            .fetch_add(downloaded_bytes, Ordering::Relaxed);
        add_duration(&self.inner.get_elapsed_ns, elapsed);
    }

    pub(crate) fn record_head(&self, elapsed: Duration) {
        self.inner.head_requests.fetch_add(1, Ordering::Relaxed);
        add_duration(&self.inner.head_elapsed_ns, elapsed);
    }

    pub(crate) fn record_put(&self, uploaded_bytes: u64, elapsed: Duration) {
        self.inner.put_requests.fetch_add(1, Ordering::Relaxed);
        self.inner
            .uploaded_bytes
            .fetch_add(uploaded_bytes, Ordering::Relaxed);
        add_duration(&self.inner.put_elapsed_ns, elapsed);
    }

    pub(crate) fn record_delete(&self, elapsed: Duration) {
        self.inner.delete_requests.fetch_add(1, Ordering::Relaxed);
        add_duration(&self.inner.delete_elapsed_ns, elapsed);
    }
}

fn add_duration(counter: &AtomicU64, elapsed: Duration) {
    counter.fetch_add(
        elapsed.as_nanos().min(u64::MAX as u128) as u64,
        Ordering::Relaxed,
    );
}
