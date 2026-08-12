use uuid::Uuid;

pub const V1_ROOT: &str = "v1";
pub const HEADS_PREFIX: &str = "v1/heads/";
pub const CHECKPOINT_HEAD_KEY: &str = "v1/checkpoint.bin";

const DIGEST_HEX_LEN: usize = 64;
const SEQUENCE_WIDTH: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceCategory {
    Image,
    File,
    Preview,
    Icon,
}

impl ResourceCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::File => "file",
            Self::Preview => "preview",
            Self::Icon => "icon",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "image" => Ok(Self::Image),
            "file" => Ok(Self::File),
            "preview" => Ok(Self::Preview),
            "icon" => Ok(Self::Icon),
            _ => Err("resource key has an unknown category".to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSegmentKey {
    pub device_id: String,
    pub epoch: String,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCheckpointKey {
    pub generation: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedResourceKey {
    pub category: ResourceCategory,
    pub sha256: String,
    pub extension: String,
}

fn validate_uuid(value: &str, label: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("invalid {label} UUID"))?;
    if parsed.to_string() != value {
        return Err(format!("{label} UUID must use canonical lowercase form"));
    }
    Ok(())
}

fn validate_digest(digest: &str) -> Result<(), String> {
    if digest.len() != DIGEST_HEX_LEN
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("SHA-256 digest must be 64 lowercase hexadecimal characters".to_string());
    }
    Ok(())
}

fn validate_extension(extension: &str) -> Result<(), String> {
    if extension.is_empty()
        || extension.len() > 16
        || !extension
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(
            "resource extension must be 1-16 lowercase alphanumeric characters".to_string(),
        );
    }
    Ok(())
}

pub fn head_object_key(device_id: &str) -> Result<String, String> {
    validate_uuid(device_id, "device")?;
    Ok(format!("{HEADS_PREFIX}{device_id}.bin"))
}

pub fn parse_head_key(key: &str) -> Result<String, String> {
    let device_id = key
        .strip_prefix(HEADS_PREFIX)
        .and_then(|value| value.strip_suffix(".bin"))
        .ok_or_else(|| "head key has an invalid layout".to_string())?;
    validate_uuid(device_id, "device")?;
    if head_object_key(device_id)? != key {
        return Err("head key is not canonical".to_string());
    }
    Ok(device_id.to_string())
}

pub fn snapshot_object_key(device_id: &str, epoch: &str, sha256: &str) -> Result<String, String> {
    validate_uuid(device_id, "device")?;
    validate_uuid(epoch, "epoch")?;
    validate_digest(sha256)?;
    Ok(format!(
        "{V1_ROOT}/snapshots/{device_id}/{epoch}/{sha256}.pack"
    ))
}

pub fn segment_prefix(device_id: &str, epoch: &str) -> Result<String, String> {
    validate_uuid(device_id, "device")?;
    validate_uuid(epoch, "epoch")?;
    Ok(format!("{V1_ROOT}/segments/{device_id}/{epoch}/"))
}

pub fn segment_object_key(
    device_id: &str,
    epoch: &str,
    first_sequence: u64,
    last_sequence: u64,
    sha256: &str,
) -> Result<String, String> {
    if first_sequence == 0 || first_sequence > last_sequence {
        return Err("segment sequence range is invalid".to_string());
    }
    validate_digest(sha256)?;
    Ok(format!(
        "{}{first_sequence:0SEQUENCE_WIDTH$}-{last_sequence:0SEQUENCE_WIDTH$}-{sha256}.pack",
        segment_prefix(device_id, epoch)?
    ))
}

pub fn parse_segment_key(key: &str) -> Result<ParsedSegmentKey, String> {
    if key.contains('\\') || key.starts_with('/') || key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("segment key contains unsafe characters".to_string());
    }
    let components: Vec<&str> = key.split('/').collect();
    if components.len() != 5
        || components[0] != V1_ROOT
        || components[1] != "segments"
        || components.iter().any(|component| component.is_empty())
    {
        return Err("segment key has an invalid layout".to_string());
    }
    let device_id = components[2];
    let epoch = components[3];
    validate_uuid(device_id, "device")?;
    validate_uuid(epoch, "epoch")?;

    parse_segment_filename(device_id, epoch, components[4])
}

fn parse_segment_filename(
    device_id: &str,
    epoch: &str,
    filename: &str,
) -> Result<ParsedSegmentKey, String> {
    let stem = filename
        .strip_suffix(".pack")
        .ok_or_else(|| "segment key must end in .pack".to_string())?;
    let mut parts = stem.split('-');
    let first = parts
        .next()
        .ok_or_else(|| "segment key is missing its first sequence".to_string())?;
    let last = parts
        .next()
        .ok_or_else(|| "segment key is missing its last sequence".to_string())?;
    let digest = parts
        .next()
        .ok_or_else(|| "segment key is missing its digest".to_string())?;
    if parts.next().is_some() || first.len() != SEQUENCE_WIDTH || last.len() != SEQUENCE_WIDTH {
        return Err("segment key sequence fields must be fixed-width".to_string());
    }
    if !first.bytes().all(|byte| byte.is_ascii_digit())
        || !last.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err("segment key sequence fields must be decimal".to_string());
    }
    let first_sequence = first
        .parse::<u64>()
        .map_err(|_| "segment first sequence is out of range".to_string())?;
    let last_sequence = last
        .parse::<u64>()
        .map_err(|_| "segment last sequence is out of range".to_string())?;
    if first_sequence == 0 || first_sequence > last_sequence {
        return Err("segment sequence range is invalid".to_string());
    }
    validate_digest(digest)?;
    Ok(ParsedSegmentKey {
        device_id: device_id.to_string(),
        epoch: epoch.to_string(),
        first_sequence,
        last_sequence,
        sha256: digest.to_string(),
    })
}

pub fn checkpoint_object_key(generation: u64, sha256: &str) -> Result<String, String> {
    if generation == 0 {
        return Err("checkpoint generation must be positive".to_string());
    }
    validate_digest(sha256)?;
    Ok(format!(
        "{V1_ROOT}/checkpoints/{generation:0SEQUENCE_WIDTH$}-{sha256}.pack"
    ))
}

pub fn parse_checkpoint_key(key: &str) -> Result<ParsedCheckpointKey, String> {
    let filename = key
        .strip_prefix("v1/checkpoints/")
        .ok_or_else(|| "checkpoint key has an invalid layout".to_string())?;
    let stem = filename
        .strip_suffix(".pack")
        .ok_or_else(|| "checkpoint key must end in .pack".to_string())?;
    let (generation, sha256) = stem
        .split_once('-')
        .ok_or_else(|| "checkpoint key is missing its digest".to_string())?;
    if generation.len() != SEQUENCE_WIDTH || !generation.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("checkpoint generation must be fixed-width decimal".to_string());
    }
    let generation = generation
        .parse::<u64>()
        .map_err(|_| "checkpoint generation is out of range".to_string())?;
    validate_digest(sha256)?;
    if checkpoint_object_key(generation, sha256)? != key {
        return Err("checkpoint key is not canonical".to_string());
    }
    Ok(ParsedCheckpointKey {
        generation,
        sha256: sha256.to_string(),
    })
}

pub fn resource_object_key(
    category: ResourceCategory,
    sha256: &str,
    extension: &str,
) -> Result<String, String> {
    validate_digest(sha256)?;
    validate_extension(extension)?;
    Ok(format!(
        "{V1_ROOT}/resources/{}/sha256-{sha256}.{extension}",
        category.as_str()
    ))
}

pub fn parse_resource_key(key: &str) -> Result<ParsedResourceKey, String> {
    if key.contains('\\') || key.starts_with('/') || key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("resource key contains unsafe characters".to_string());
    }
    let components: Vec<&str> = key.split('/').collect();
    if components.len() != 4
        || components[0] != V1_ROOT
        || components[1] != "resources"
        || components.iter().any(|component| component.is_empty())
    {
        return Err("resource key has an invalid layout".to_string());
    }
    let category = ResourceCategory::parse(components[2])?;
    let (stem, extension) = components[3]
        .rsplit_once('.')
        .ok_or_else(|| "resource key is missing its extension".to_string())?;
    let sha256 = stem
        .strip_prefix("sha256-")
        .ok_or_else(|| "resource key is missing its SHA-256 prefix".to_string())?;
    validate_digest(sha256)?;
    validate_extension(extension)?;
    Ok(ParsedResourceKey {
        category,
        sha256: sha256.to_string(),
        extension: extension.to_string(),
    })
}

/// Selects only objects emitted by the discarded sync implementation.
/// The configured remote prefix is stripped by the caller before this check.
pub fn obsolete_object_candidate(relative_key: &str) -> bool {
    if relative_key.is_empty()
        || relative_key.starts_with('/')
        || relative_key.contains('\\')
        || relative_key.bytes().any(|byte| byte.is_ascii_control())
        || relative_key == V1_ROOT
        || relative_key.starts_with("v1/")
    {
        return false;
    }
    if let Some(rest) = relative_key.strip_prefix("resources/") {
        return !rest.is_empty()
            && rest
                .split('/')
                .all(|component| !component.is_empty() && component != "." && component != "..");
    }
    !relative_key.contains('/')
        && (relative_key.starts_with("baseline-") || relative_key.starts_with("oplog-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: &str = "c527a31e-7f42-43cf-bf73-6e5fbed4be18";
    const EPOCH: &str = "e04623ec-6109-4275-a748-8743f3076b7d";
    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn constructs_and_parses_fixed_width_segment_keys() {
        let key = segment_object_key(DEVICE, EPOCH, 7, 91, DIGEST).unwrap();
        assert_eq!(
            key,
            format!(
                "v1/segments/{DEVICE}/{EPOCH}/00000000000000000007-00000000000000000091-{DIGEST}.pack"
            )
        );
        let parsed = parse_segment_key(&key).unwrap();
        assert_eq!(parsed.first_sequence, 7);
        assert_eq!(parsed.last_sequence, 91);
        assert_eq!(parsed.sha256, DIGEST);
    }

    #[test]
    fn object_layout_rejects_noncanonical_identifiers_and_digests() {
        assert!(head_object_key(&DEVICE.to_uppercase()).is_err());
        assert!(snapshot_object_key(DEVICE, "not-an-epoch", DIGEST).is_err());
        assert!(segment_object_key(DEVICE, EPOCH, 0, 1, DIGEST).is_err());
        assert!(segment_object_key(DEVICE, EPOCH, 2, 1, DIGEST).is_err());
        assert!(checkpoint_object_key(0, DIGEST).is_err());
        assert!(
            resource_object_key(ResourceCategory::Image, &DIGEST.to_uppercase(), "png").is_err()
        );
        assert!(resource_object_key(ResourceCategory::Image, DIGEST, "tar.gz").is_err());
    }

    #[test]
    fn head_keys_round_trip_from_v1() {
        let key = head_object_key(DEVICE).unwrap();
        assert_eq!(parse_head_key(&key).unwrap(), DEVICE);
        assert!(parse_head_key("v3/heads/device.bin").is_err());
        assert!(parse_head_key(&format!("v1/heads/{}.BIN", DEVICE)).is_err());
    }

    #[test]
    fn checkpoint_keys_round_trip_with_fixed_width_generation() {
        let key = checkpoint_object_key(42, DIGEST).unwrap();
        let parsed = parse_checkpoint_key(&key).unwrap();
        assert_eq!(parsed.generation, 42);
        assert_eq!(parsed.sha256, DIGEST);
        assert!(parse_checkpoint_key(&format!("v1/checkpoints/42-{DIGEST}.pack")).is_err());
        assert!(
            parse_checkpoint_key(&format!("v1/checkpoints/{:020}-{}.pack", 0, DIGEST)).is_err()
        );
    }

    #[test]
    fn resource_keys_round_trip_and_reject_noncanonical_paths() {
        let key = resource_object_key(ResourceCategory::File, DIGEST, "bin").unwrap();
        assert_eq!(
            parse_resource_key(&key).unwrap(),
            ParsedResourceKey {
                category: ResourceCategory::File,
                sha256: DIGEST.to_string(),
                extension: "bin".to_string(),
            }
        );
        assert!(parse_resource_key("v1/resources/unknown/sha256-a.bin").is_err());
        assert!(parse_resource_key(&format!("v1/resources/file/../sha256-{DIGEST}.bin")).is_err());
        assert!(parse_resource_key(&format!("v1/resources/file/sha256-{}.BIN", DIGEST)).is_err());
    }

    #[test]
    fn obsolete_cleanup_never_selects_v1_or_parent_paths() {
        assert!(obsolete_object_candidate("baseline-device.zip"));
        assert!(obsolete_object_candidate("oplog-device-s1-e2-hash"));
        assert!(obsolete_object_candidate("resources/image/a.png"));
        assert!(!obsolete_object_candidate("resources/../v1/heads/a.bin"));
        assert!(!obsolete_object_candidate("nested/baseline-device.zip"));
        assert!(!obsolete_object_candidate("v1/heads/device.bin"));
        assert!(!obsolete_object_candidate("v1/resources/image/a.png"));
        assert!(!obsolete_object_candidate("resources/"));
        assert!(!obsolete_object_candidate("../baseline-device.zip"));
    }
}
