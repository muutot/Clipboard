//! S3-first synchronization primitives shared independently of the desktop runtime.

pub mod s3;
pub mod v1;

pub use s3::{test_s3_connection, S3TestResult};
