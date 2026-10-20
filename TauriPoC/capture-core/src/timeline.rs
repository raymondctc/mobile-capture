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

pub(crate) fn network_events(
    directory: &std::path::Path,
    started_at: &str,
    live: bool,
) -> crate::model::Result<Vec<Event>> {
    use std::io::{BufRead, Read};
    let file = match crate::files::open(directory, "network/events.jsonl") {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut reader = std::io::BufReader::new(file);
    let mut output = vec![];
    let mut line = 0;
    loop {
        let mut bytes = vec![];
        let n = std::io::Read::by_ref(&mut reader)
            .take(2 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if n > 2 * 1024 * 1024 {
            return Err("Network record exceeds enforced 2 MiB serialization bound".into());
        }
        line += 1;
        if live && !bytes.ends_with(b"\n") {
            break;
        }
        while matches!(bytes.last(), Some(b'\n' | b'\r')) {
            bytes.pop();
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let value:serde_json::Value=serde_json::from_slice(&bytes).unwrap_or(json!({"kind":"parse_error","summary":"Unparsed network event","detail":{"parse_error":"Invalid network JSON record"}}));
        let mut detail = value["detail"].as_object().cloned().unwrap_or_default();
        if value["id"].is_string() {
            detail.insert("source_event_id".into(), value["id"].clone());
        }
        if value["raw_ref"].is_string() {
            detail.insert("native_ref".into(), value["raw_ref"].clone());
        }
        let parsed = value["timestamp"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        let received = value["received_at"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        let uncertainty = value["clock_uncertainty_ms"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.);
        let (timestamp, basis, calibrated, uncertainty) = if let Some(time) = parsed {
            (
                time.to_utc().to_rfc3339_opts(SecondsFormat::Micros, true),
                "source_timestamp",
                uncertainty.is_some(),
                uncertainty,
            )
        } else if let Some(time) = received {
            (
                time.to_utc().to_rfc3339_opts(SecondsFormat::Micros, true),
                "received_at_fallback",
                false,
                None,
            )
        } else {
            (started_at.to_owned(), "session_start_fallback", false, None)
        };
        detail.insert("clock_calibrated".into(), json!(calibrated));
        detail.insert("timestamp_basis".into(), json!(basis));
        detail.insert("temporal_matching".into(), json!("inferred"));
        let mut identity = format!("network\0network/events.jsonl\0{line}\0").into_bytes();
        identity.extend_from_slice(&bytes);
        output.push(Event {
            id: format!("event-{}", &hash(&identity)[..24]),
            timestamp,
            original_timestamp: value
                .get("original_timestamp")
                .unwrap_or(&value["timestamp"])
                .as_str()
                .map(String::from),
            received_at: received
                .map(|time| time.to_utc().to_rfc3339_opts(SecondsFormat::Micros, true)),
            source: "network".into(),
            kind: value["kind"].as_str().unwrap_or("network").into(),
            summary: value["summary"].as_str().unwrap_or("Network event").into(),
            raw_ref: format!("network/events.jsonl#L{line}"),
            detail: serde_json::Value::Object(detail),
            clock_uncertainty_ms: uncertainty,
        });
    }
    Ok(output)
}

pub(crate) fn sort(events: &mut [Event]) {
    events.sort_by(|a, b| {
        (&a.timestamp, source_order(&a.source), line(a), &a.id).cmp(&(
            &b.timestamp,
            source_order(&b.source),
            line(b),
            &b.id,
        ))
    });
}
fn source_order(source: &str) -> u8 {
    match source {
        "logs" => 0,
        "network" => 1,
        _ => 2,
    }
}
fn truncate(text: &mut String, max: usize) {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
}
pub(crate) fn preview(event: &mut Event) {
    use base64::{engine::general_purpose::STANDARD, Engine};
    fn shorten(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) if s.len() > 256 => {
                truncate(s, 256);
            }
            serde_json::Value::Array(a) => {
                a.truncate(16);
                for v in a {
                    shorten(v);
                }
            }
            serde_json::Value::Object(o) => {
                for v in o.values_mut() {
                    shorten(v);
                }
            }
            _ => {}
        }
    }
    let original = event.detail.clone();
    shorten(&mut event.detail);
    for key in ["request_body", "response_body"] {
        if let Some(body) = original.get(key) {
            let mut preview = body.clone();
            if body["unavailable"] != true {
                if let Some(text) = body["text"].as_str() {
                    if let Ok(bytes) = STANDARD.decode(text) {
                        let n = bytes.len().min(1024);
                        preview["text"] = json!(STANDARD.encode(&bytes[..n]));
                        preview["preview"] = json!(true);
                        preview["preview_bytes"] = json!(n);
                        preview["retained_bytes"] = json!(bytes.len());
                    }
                }
            }
            event.detail[key] = preview;
        }
    }
    truncate(&mut event.summary, 1024);
    truncate(&mut event.timestamp, 64);
    if let Some(original) = &mut event.original_timestamp {
        truncate(original, 64);
    }
    if let Some(received) = &mut event.received_at {
        truncate(received, 64);
    }
    truncate(&mut event.kind, 128);
    event.detail["snapshot_preview"] = json!(true);
    if serde_json::to_vec(event).map_or(true, |v| v.len() > 6144) {
        let mut retained = serde_json::Map::new();
        retained.insert("snapshot_preview".into(), json!(true));
        retained.insert("detail_preview_omitted".into(), json!(true));
        for key in ["request_body", "response_body", "method", "url", "status"] {
            if let Some(value) = event.detail.get(key) {
                retained.insert(key.into(), value.clone());
            }
        }
        event.detail = serde_json::Value::Object(retained);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD, Engine};
    #[test]
    fn network_normalization_matches_the_schema_one_python_normalizer() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("network")).unwrap();
        let start = "2026-10-20T00:00:00.000000Z";
        let records = [
            json!({"id":"network:fixture:request","timestamp":"2026-10-20T01:02:03.123456Z","original_timestamp":"1792458123.123456","received_at":"2026-10-20T01:02:03.123457Z","source":"network","kind":"request","summary":"POST fixture","raw_ref":"network/capture.mitm#fixture","detail":{"request_body":{"encoding":"base64","text":"AP8=","bytes":2,"byte_basis":"raw_content"}},"clock_uncertainty_ms":1}),
            json!({"id":"network:fixture:response","timestamp":"invalid","received_at":"2026-10-20T01:02:03.223457Z","kind":"response","summary":"200 fixture","detail":{"response_body":{"encoding":null,"text":null,"bytes":null,"unavailable":true,"byte_basis":"raw_content"}},"clock_uncertainty_ms":1}),
        ];
        let bytes = records
            .iter()
            .map(|r| format!("{}\n", serde_json::to_string(r).unwrap()))
            .collect::<String>();
        std::fs::write(directory.path().join("network/events.jsonl"), bytes).unwrap();
        let expected = std::process::Command::new(if cfg!(windows) { "python" } else { "python3" })
            .args([
                "-c",
                r#"
import sys, json
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import evidence
root = Path(sys.argv[2])
raw_files = {"network/events.jsonl": (root / "network/events.jsonl").read_bytes()}
print(json.dumps(evidence._read_events(root, {"started_at": sys.argv[3]}, raw_files=raw_files)))
"#,
            ])
            .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../CaptureRuntime"))
            .arg(directory.path())
            .arg(start)
            .output()
            .unwrap();
        assert!(
            expected.status.success(),
            "{}",
            String::from_utf8_lossy(&expected.stderr)
        );
        let expected: serde_json::Value = serde_json::from_slice(&expected.stdout).unwrap();
        assert_eq!(
            serde_json::to_value(network_events(directory.path(), start, false).unwrap()).unwrap(),
            expected
        );
    }
    #[test]
    fn network_snapshot_previews_bound_strings_without_corrupting_base64() {
        let bytes = vec![255; 256 * 1024];
        let mut event = Event {
            id: "event-fixture".into(),
            timestamp: "2026-10-20T00:00:00Z".into(),
            original_timestamp: Some("9".repeat(1024 * 1024)),
            received_at: None,
            source: "network".into(),
            kind: "response".into(),
            summary: "日".repeat(1024 * 1024),
            raw_ref: "network/events.jsonl#L1".into(),
            detail: json!({"request_body":{"encoding":"base64","text":STANDARD.encode(&bytes),"bytes":bytes.len(),"byte_basis":"raw_content"},"response_body":{"encoding":null,"text":null,"bytes":null,"unavailable":true,"byte_basis":"raw_content"},"response_headers":[{"name":"long","value":"日".repeat(1024*1024)}]}),
            clock_uncertainty_ms: Some(1.),
        };
        preview(&mut event);
        assert!(serde_json::to_vec(&event).unwrap().len() <= 6144);
        let body = &event.detail["request_body"];
        assert_eq!(body["preview_bytes"], 1024);
        assert_eq!(body["retained_bytes"], bytes.len());
        assert_eq!(
            STANDARD.decode(body["text"].as_str().unwrap()).unwrap(),
            vec![255; 1024]
        );
        assert_eq!(event.detail["response_body"]["unavailable"], true);
        assert!(serde_json::to_vec(&vec![event; 500]).unwrap().len() < 4 * 1024 * 1024);
    }
}
