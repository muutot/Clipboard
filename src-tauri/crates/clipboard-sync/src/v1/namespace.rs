//! Stable cryptographic namespace identity, independent of S3 login credentials.
//! Existing v1 data keeps its original derivation scope; only this small binding
//! is added, after authenticating the existing pointers, using create-if-absent.
use super::{
    decode_checkpoint_head, decode_device_head, parse_head_key, ObjectStore, PutCondition,
    SessionKey, CHECKPOINT_HEAD_KEY, HEADS_PREFIX,
};
use serde::{Deserialize, Serialize};

pub const NAMESPACE_KEY: &str = "v1/namespace.json";
const MAX_DESCRIPTOR_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Namespace {
    version: u8,
    pub scope: String,
    key_check: Option<String>,
}
impl Namespace {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_DESCRIPTOR_BYTES {
            return Err("sync namespace descriptor is too large".into());
        }
        let value: Self =
            serde_json::from_slice(bytes).map_err(|_| "invalid sync namespace descriptor")?;
        if value.version != 1
            || !is_digest(&value.scope)
            || value.key_check.as_ref().is_some_and(|v| !is_digest(v))
        {
            return Err("unsupported or invalid sync namespace descriptor".into());
        }
        Ok(value)
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("namespace contains only strings and a version")
    }
    pub fn session_key(&self, password: Option<&str>) -> Result<Option<SessionKey>, String> {
        let key = password
            .filter(|v| !v.is_empty())
            .map(|p| SessionKey::derive(p, &self.scope))
            .transpose()?;
        match (&key, &self.key_check) {
            (None, None) => {}
            (Some(key), Some(check)) => key.verify_namespace_check(&self.scope, check)?,
            _ => return Err(
                "sync namespace encryption mode differs; in-place password changes are unsupported"
                    .into(),
            ),
        }
        Ok(key)
    }
}
fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
}

/// Read/claim an immutable namespace binding. A legacy encrypted namespace must
/// first be upgraded using its original access key; subsequent credentials can
/// discover that same public derivation scope without knowing the old login.
pub fn resolve_namespace(
    store: &impl ObjectStore,
    stable_scope: &str,
    legacy_scope: &str,
    password: Option<&str>,
    known: Option<&Namespace>,
) -> Result<(Namespace, Option<SessionKey>), String> {
    if let Some(object) = store.get(NAMESPACE_KEY)? {
        let descriptor = Namespace::from_bytes(&object.bytes)?;
        ensure_unchanged(known, &descriptor)?;
        let key = descriptor.session_key(password)?;
        return Ok((descriptor, key));
    }
    if !is_digest(stable_scope) || !is_digest(legacy_scope) {
        return Err("invalid sync namespace scope".into());
    }
    let heads: Vec<_> = store
        .list(HEADS_PREFIX, None)?
        .into_iter()
        .filter(|v| parse_head_key(&v.key).is_ok())
        .collect();
    let checkpoint = store.get(CHECKPOINT_HEAD_KEY)?;
    let legacy = !heads.is_empty() || checkpoint.is_some();
    let scope =
        known
            .map(|v| v.scope.as_str())
            .unwrap_or(if legacy { legacy_scope } else { stable_scope });
    let key = if let Some(known) = known {
        known.session_key(password)?
    } else {
        password
            .filter(|v| !v.is_empty())
            .map(|p| SessionKey::derive(p, scope))
            .transpose()?
    };
    // Authenticate before writing the binding, even if the local head cache is
    // warm. This also prevents changed passwords from creating mixed ciphertext.
    for head in heads {
        let object = store
            .get(&head.key)?
            .ok_or("remote head disappeared during namespace upgrade")?;
        decode_device_head(&object.bytes, key.as_ref()).map_err(upgrade_error)?;
    }
    if let Some(object) = checkpoint {
        decode_checkpoint_head(&object.bytes, key.as_ref()).map_err(upgrade_error)?;
    }
    let candidate = Namespace {
        version: 1,
        scope: scope.into(),
        key_check: key.as_ref().map(|key| key.namespace_check(scope)),
    };
    store.put(NAMESPACE_KEY, candidate.to_bytes(), PutCondition::IfAbsent)?;
    // Adopt a concurrent winner only after validating its key/mode. A provider
    // that loses the write is an error, never permission to publish unbound data.
    let object = store
        .get(NAMESPACE_KEY)?
        .ok_or("sync namespace binding was not durable")?;
    let descriptor = Namespace::from_bytes(&object.bytes)?;
    ensure_unchanged(known, &descriptor)?;
    if descriptor == candidate {
        return Ok((descriptor, key));
    }
    let key = descriptor.session_key(password)?;
    Ok((descriptor, key))
}
fn ensure_unchanged(known: Option<&Namespace>, descriptor: &Namespace) -> Result<(), String> {
    if known.is_some_and(|known| known != descriptor) {
        return Err(
            "remote sync namespace binding changed; refusing to overwrite existing data".into(),
        );
    }
    Ok(())
}
fn upgrade_error(_: String) -> String {
    "cannot authenticate existing sync data: verify the password and upgrade once with the original S3 access key before rotating credentials".into()
}
