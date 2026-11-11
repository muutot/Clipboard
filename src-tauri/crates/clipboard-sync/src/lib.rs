//! S3-first synchronization primitives shared independently of the desktop runtime.

pub mod cancellation;
pub mod s3;
mod transport;
pub mod v1;

pub use s3::{S3TestResult, test_s3_connection};
