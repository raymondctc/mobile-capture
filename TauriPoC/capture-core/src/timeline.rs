use crate::model::Event;
use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

static LOG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[ \t]*(\d{9,}(?:\.\d{1,9})?)\s+(\d+)\s+(\d+)\s+([VDIWEFAS])\s+(.*?):\s?(.*)$")
        .expect("static logcat grammar")
});
pub fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

pub fn events(logs: &[u8], markers: &[u8], started_at: &str) -> Vec<Event> {
    let mut output = Vec::new();
    for (source, path, bytes) in [
        ("logs", "logs/device.log", logs),
        ("marker", "timeline/markers.jsonl", markers),
    ] {
        for (index, raw) in records(bytes).into_iter().enumerate() {
            if raw.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let text = String::from_utf8_lossy(raw);
            let mut detail = json!({"clock_calibrated": false, "timestamp_basis": "session_start_fallback", "temporal_matching": "inferred"});
            let mut original = None;
            let mut timestamp = started_at.to_string();
            let (kind, summary, uncertainty) = if source == "logs" {
                if let Some(fields) = LOG.captures(&text) {
                    original = Some(fields[1].to_string());
                    let epoch = fields[1].parse::<f64>().ok().filter(|v| v.is_finite());
                    if let Some(time) = epoch.and_then(|value| {
                        DateTime::<Utc>::from_timestamp_micros((value * 1_000_000.).round() as i64)
                    }) {
                        timestamp = time.to_rfc3339_opts(SecondsFormat::Micros, true);
                        detail["timestamp_basis"] = json!("source_timestamp");
                    }
                    detail["format"] = json!("android_logcat_epoch");
                    detail["pid"] = json!(fields[2].parse::<u64>().unwrap_or(0));
                    detail["tid"] = json!(fields[3].parse::<u64>().unwrap_or(0));
                    detail["level"] = json!(&fields[4]);
                    detail["tag"] = json!(fields[5].trim());
                    detail["message"] = json!(&fields[6]);
                    (
                        "log".to_string(),
                        format!("{}: {}", fields[5].trim(), &fields[6]),
                        None,
                    )
                } else {
                    detail["format"] = json!("unparsed");
                    detail["parse_error"] = json!("No supported timestamped log format");
                    ("unparsed_log".into(), text.to_string(), None)
                }
            } else {
                match serde_json::from_slice::<serde_json::Value>(raw) {
                    Ok(value) if value.is_object() => {
                        if let Some(time) = value["timestamp"]
                            .as_str()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        {
                            timestamp = time.to_utc().to_rfc3339_opts(SecondsFormat::Micros, true);
                            original = value["timestamp"].as_str().map(String::from);
                            detail["clock_calibrated"] = json!(true);
                            detail["timestamp_basis"] = json!("source_timestamp");
                        }
                        (
                            "marker".into(),
                            value["text"].as_str().unwrap_or("Marker").into(),
                            Some(1.),
                        )
                    }
                    _ => {
                        detail["parse_error"] = json!("Invalid marker record");
                        ("parse_error".into(), "Unparsed marker event".into(), None)
                    }
                }
            };
            let number = index + 1;
            let mut identity = format!("{source}\0{path}\0{number}\0").into_bytes();
            identity.extend_from_slice(raw);
            output.push(Event {
                id: format!("event-{}", &hash(&identity)[..24]),
                timestamp,
                original_timestamp: original,
                received_at: None,
                source: source.into(),
                kind,
                summary,
                raw_ref: format!("{path}#L{number}"),
                detail,
                clock_uncertainty_ms: uncertainty,
            });
        }
    }
    output.sort_by(|a, b| {
        (
            &a.timestamp,
            if a.source == "logs" { 0 } else { 2 },
            line(a),
            &a.id,
        )
            .cmp(&(
                &b.timestamp,
                if b.source == "logs" { 0 } else { 2 },
                line(b),
                &b.id,
            ))
    });
    output
}
fn line(event: &Event) -> usize {
    event
        .raw_ref
        .rsplit("#L")
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn records(bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' || bytes[index] == b'\n' {
            lines.push(&bytes[start..index]);
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            start = index + 1;
        }
        index += 1;
    }
    if start < bytes.len() {
        lines.push(&bytes[start..]);
    }
    lines
}
