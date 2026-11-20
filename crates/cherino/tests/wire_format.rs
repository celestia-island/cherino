//! Pins the JSON spelling of the two timestamp fields that cross cherino's
//! public API.
//!
//! `ContainerInfo::created_at` and `ContainerBinding::bound_at` used to be
//! `DateTime<Utc>` from the previous library and are now `jiff::Timestamp`. Both
//! serialize a UTC instant as RFC 3339 with a `Z` suffix, so the wire form is
//! unchanged for readers; what differs is fraction padding (the old library pads to
//! 0/3/6/9 digits, `jiff` trims trailing zeros). The assertions below pin both
//! the shape clients already read and the trimming rule, so a future `jiff`
//! bump cannot silently change the payload.

use cherino::binding::{BindingId, ContainerBinding};
use cherino::types::{ContainerInfo, ContainerStatus};
use jiff::Timestamp;
use std::collections::HashMap;

fn container_info(created_at: Option<Timestamp>) -> ContainerInfo {
    ContainerInfo {
        id: "abc123".to_string(),
        name: "web".to_string(),
        image: "nginx:1".to_string(),
        status: ContainerStatus::Running,
        created_at,
        ports: Vec::new(),
        env: HashMap::new(),
        volumes: Vec::new(),
        ip_address: None,
        labels: HashMap::new(),
    }
}

fn parse(raw: &str) -> Timestamp {
    raw.parse().expect("fixture timestamp is valid RFC 3339")
}

#[test]
fn created_at_serializes_as_rfc3339_utc() {
    let json = serde_json::to_string(&container_info(Some(parse("2026-11-20T14:30:05.123Z"))))
        .expect("ContainerInfo serializes");
    assert_eq!(
        json,
        r#"{"id":"abc123","name":"web","image":"nginx:1","status":"running","created_at":"2026-11-20T14:30:05.123Z","ports":[],"env":{},"volumes":[],"labels":{}}"#
    );
}

/// The one shape where the two libraries differ at the serde level: a fraction
/// whose trailing digits are zeros. The old library padded it to 3/6/9 digits;
/// jiff trims.
#[test]
fn created_at_serializes_trailing_zero_fractions_trimmed() {
    let json = serde_json::to_string(&container_info(Some(parse("2026-11-20T14:30:05.120Z"))))
        .expect("ContainerInfo serializes");
    assert!(
        json.contains(r#""created_at":"2026-11-20T14:30:05.12Z""#),
        "expected a trimmed fraction, got {json}"
    );
    let json = serde_json::to_string(&container_info(Some(parse("2026-11-20T14:30:05.500Z"))))
        .expect("ContainerInfo serializes");
    assert!(
        json.contains(r#""created_at":"2026-11-20T14:30:05.5Z""#),
        "expected a trimmed fraction, got {json}"
    );
}

#[test]
fn absent_created_at_is_omitted() {
    let json = serde_json::to_string(&container_info(None)).expect("ContainerInfo serializes");
    assert_eq!(
        json,
        r#"{"id":"abc123","name":"web","image":"nginx:1","status":"running","ports":[],"env":{},"volumes":[],"labels":{}}"#
    );
}

#[test]
fn pre_migration_timestamps_still_deserialize() {
    // serde wrote `Z` for `DateTime<Utc>` too, but the pre-migration build also
    // rendered these instants with `to_rfc3339()` (`+00:00`) into string fields,
    // so payloads carrying the offset spelling must keep loading.
    let legacy = r#"{"id":"abc123","name":"web","image":"nginx:1","status":"running","created_at":"2026-11-20T14:30:05.123+00:00","ports":[],"env":{},"volumes":[],"labels":{}}"#;
    let parsed: ContainerInfo = serde_json::from_str(legacy).expect("legacy payload parses");
    assert_eq!(parsed.created_at, Some(parse("2026-11-20T14:30:05.123Z")));
}

#[test]
fn subsecond_fraction_trailing_zeros_are_trimmed() {
    // The old library wrote `.120000000` / `.120`; jiff writes `.12`. Same instant.
    assert_eq!(
        parse("2026-11-20T14:30:05.120Z").to_string(),
        "2026-11-20T14:30:05.12Z"
    );
    assert_eq!(
        parse("2026-11-20T14:30:05.123400Z").to_string(),
        "2026-11-20T14:30:05.1234Z"
    );
    assert_eq!(
        parse("2026-11-20T14:30:05.123456780Z").to_string(),
        "2026-11-20T14:30:05.12345678Z"
    );
}

#[test]
fn bound_at_serializes_as_rfc3339_utc() {
    let binding = ContainerBinding {
        container_uuid: uuid::Uuid::nil(),
        binding_id: BindingId::new("github", "234"),
        bound_at: parse("2026-11-20T14:30:05Z"),
        metadata: serde_json::Value::Null,
    };
    assert_eq!(
        serde_json::to_string(&binding).expect("ContainerBinding serializes"),
        r#"{"container_uuid":"00000000-0000-0000-0000-000000000000","binding_id":{"platform":"github","external_id":"234","floor":null},"bound_at":"2026-11-20T14:30:05Z","metadata":null}"#
    );
}
