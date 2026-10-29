//! Persistent database claims for roots that may survive a data migration.
//!
//! A single database cannot decide whether another database's files are orphaned.
//! Claims are additive; cleanup stays off for shared roots instead of pruning a
//! potentially active or recoverable database merely because it looks old.

use std::{collections::BTreeSet, fs, io::Read, path::Path};

use serde::{Deserialize, Serialize};

use super::types::RESOURCE_ROOT_DATABASES;
use crate::storage::StorageError;

const MAX_CLAIMS_BYTES: u64 = 65_536;
const MAX_DATABASES: usize = 64;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DatabaseClaims {
    version: u8,
    databases: BTreeSet<String>,
}

fn read_claims(root: &Path) -> Result<Option<DatabaseClaims>, StorageError> {
    let file = match fs::File::open(root.join(RESOURCE_ROOT_DATABASES)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CLAIMS_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CLAIMS_BYTES {
        return Err(std::io::Error::other("resource database claims exceed the size limit").into());
    }
    let claims: DatabaseClaims = serde_json::from_slice(&bytes)?;
    if claims.version != 1
        || claims.databases.len() > MAX_DATABASES
        || claims.databases.iter().any(String::is_empty)
    {
        return Err(std::io::Error::other("invalid resource database claims").into());
    }
    Ok(Some(claims))
}

/// Caller holds the persistent resource-root lock. Registration must succeed
/// before an initializer returns roots where the new database can publish.
pub(super) fn register_database(root: &Path, identity: String) -> Result<bool, StorageError> {
    let mut claims = read_claims(root)?.unwrap_or(DatabaseClaims {
        version: 1,
        databases: BTreeSet::new(),
    });
    if claims.databases.insert(identity) {
        if claims.databases.len() > MAX_DATABASES {
            return Err(
                std::io::Error::other("too many databases share this resource root").into(),
            );
        }
        let bytes = serde_json::to_vec_pretty(&claims)?;
        if bytes.len() as u64 > MAX_CLAIMS_BYTES {
            return Err(
                std::io::Error::other("resource database claims exceed the size limit").into(),
            );
        }
        super::store::write_resource_root_metadata(root, RESOURCE_ROOT_DATABASES, &bytes)?;
    }
    let exclusive = claims.databases.len() == 1;
    if !exclusive {
        crate::log_warn!(
            "[cleanup] multiple databases use {}; orphan deletion is disabled for this root",
            root.display()
        );
    }
    Ok(exclusive)
}

/// Missing, malformed or unreadable claims cannot authorize an unlink.
pub(super) fn database_is_exclusive(root: &Path, identity: &str) -> bool {
    read_claims(root)
        .ok()
        .flatten()
        .is_some_and(|claims| claims.databases.len() == 1 && claims.databases.contains(identity))
}
