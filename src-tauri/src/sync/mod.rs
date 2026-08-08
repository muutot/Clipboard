pub mod backup;
pub mod crypto;
pub mod resources;
pub mod s3;
pub mod webdav;
pub mod wire;

/// Stable per-device identifier used for oplog file names and conflict
/// resolution. Falls back to the machine hostname, then a generic placeholder.
/// Keep the fallback in one place so every sync/backup surface agrees.
pub fn device_id() -> String {
    if let Ok(hostname) = std::env::var("COMPUTERNAME") {
        return hostname.to_lowercase();
    }
    if let Ok(hostname) = std::env::var("HOSTNAME") {
        return hostname.to_lowercase();
    }
    "unknown".to_string()
}

pub use backup::{
    count_unsynced, create_baseline_backup, create_oplog_backup, mark_oplog_synced, purge_oplog,
    read_baseline_items, read_baseline_with_resources, read_manifest_from_backup,
    write_baseline_zip, BackupManifest, ResourceEntry,
};
pub use resources::{
    collect_entry_resources, collect_item_resources, materialize_resources,
    rewrite_item_paths_to_local, rewrite_to_local,
};
pub use s3::{
    delete_from_s3, download_from_s3, list_s3_objects, test_s3_connection, upload_to_s3, S3Entry,
    S3TestResult,
};
pub use webdav::{
    delete_from_webdav, download_from_webdav, list_webdav_files, test_webdav_connection,
    upload_to_webdav, WebDavEntry, WebDavTestResult,
};
pub use wire::{
    deserialize_baseline_with_resources, deserialize_oplog, deserialize_oplog_with_resources,
    merge_baselines, serialize_baseline_with_resources, serialize_oplog,
    serialize_oplog_with_resources, OplogResource,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_falls_back_to_unknown_without_hostname_env() {
        unsafe {
            std::env::remove_var("COMPUTERNAME");
            std::env::remove_var("HOSTNAME");
        }
        assert_eq!(device_id(), "unknown");
    }
}
