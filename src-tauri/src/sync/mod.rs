pub mod backup;
pub mod crypto;
pub mod proto;
pub mod resources;
pub mod s3;
pub mod webdav;

pub use backup::{
    count_unsynced, create_baseline_backup, create_oplog_backup, mark_oplog_synced, purge_oplog,
    read_baseline_items, read_manifest_from_backup, BackupManifest, ResourceEntry,
};
pub use proto::{
    deserialize_oplog, deserialize_oplog_with_resources, serialize_oplog,
    serialize_oplog_with_resources, OplogResource,
};
pub use resources::{collect_entry_resources, materialize_resources, rewrite_to_local};
pub use s3::{
    delete_from_s3, download_from_s3, list_s3_objects, test_s3_connection, upload_to_s3, S3Entry,
    S3TestResult,
};
pub use webdav::{
    delete_from_webdav, download_from_webdav, list_webdav_files, test_webdav_connection,
    upload_to_webdav, WebDavEntry, WebDavTestResult,
};
