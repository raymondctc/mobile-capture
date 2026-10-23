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
            .take(128 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut bytes)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if n > 128 * 1024 * 1024 {
            return Err("Network record exceeds enforced 128 MiB serialization bound".into());
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

pub(crate) fn retain_recent(events: &mut Vec<Event>) {
    sort(events);
    if events.len() <= 500 {
        return;
    }
    let mut omitted = [0usize; 3];
    for event in events.iter() {
        omitted[usize::from(source_order(&event.source))] += 1;
    }
    let mut slots = 500;
    while slots > 0 {
        for count in &mut omitted {
            if *count > 0 {
                *count -= 1;
                slots -= 1;
                if slots == 0 {
                    break;
                }
            }
        }
    }
    events.retain(|event| {
        let count = &mut omitted[usize::from(source_order(&event.source))];
        if *count == 0 {
            true
        } else {
            *count -= 1;
            false
        }
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
    const EVENT_BOUND: usize = 6 * 1024 - 96;
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
    let mut bodies = serde_json::Map::new();
    for key in ["request_body", "response_body"] {
        if let Some(body) = event.detail.get(key) {
            let mut fields = serde_json::Map::new();
            for name in [
                "encoding",
                "unavailable",
                "reason",
                "byte_basis",
                "retained_bytes",
            ] {
                if let Some(value) = body.get(name) {
                    let mut value = value.clone();
                    shorten(&mut value);
                    fields.insert(name.into(), value);
                }
            }
            if body["unavailable"] != true {
                if let Some(text) = body["text"].as_str() {
                    // A multiple of four keeps the preview independently decodable.
                    let prefix = &text.as_bytes()[..text.len().min(1368)];
                    if let Ok(mut bytes) = STANDARD.decode(prefix) {
                        bytes.truncate(1024);
                        fields.insert("text".into(), json!(STANDARD.encode(&bytes)));
                        fields.insert("preview".into(), json!(true));
                        fields.insert("preview_bytes".into(), json!(bytes.len()));
                        let retained = text.len() / 4 * 3
                            - text
                                .as_bytes()
                                .iter()
                                .rev()
                                .take_while(|&&b| b == b'=')
                                .take(2)
                                .count();
                        fields.insert("retained_bytes".into(), json!(retained));
                    }
                }
            }
            bodies.insert(key.into(), serde_json::Value::Object(fields));
        }
    }
    shorten(&mut event.detail);
    for (key, body) in bodies {
        event.detail[key] = body;
    }
    truncate(&mut event.id, 128);
    truncate(&mut event.source, 32);
    truncate(&mut event.raw_ref, 512);
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
    if serde_json::to_vec(event).map_or(true, |v| v.len() > EVENT_BOUND) {
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
    if serde_json::to_vec(event).map_or(true, |v| v.len() > EVENT_BOUND) {
        event.detail = json!({"snapshot_preview":true,"detail_preview_omitted":true});
        truncate(&mut event.summary, 256);
        truncate(&mut event.raw_ref, 128);
        truncate(&mut event.kind, 32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use base64::{engine::general_purpose::STANDARD, Engine};

    fn preview_event(source: &str, number: usize) -> Event {
        Event {
            id: format!("{source}-{number}"),
            timestamp: "2026-10-15T00:00:02.000000Z".into(),
            original_timestamp: None,
            received_at: None,
            source: source.into(),
            kind: source.into(),
            summary: "fixture event".into(),
            raw_ref: format!("{source}#L{number}"),
            detail: json!({}),
            clock_uncertainty_ms: None,
        }
    }

    #[test]
    fn recent_preview_fairly_shares_capacity_and_retains_each_sources_newest_events() {
        let mut events: Vec<_> = ["logs", "network", "marker"]
            .into_iter()
            .flat_map(|source| (1..=600).map(move |number| preview_event(source, number)))
            .collect();
        let mut reversed = events.clone();
        reversed.reverse();
        retain_recent(&mut events);
        retain_recent(&mut reversed);
        assert_eq!(events.len(), 500);
        assert_eq!(
            events.iter().map(|event| &event.id).collect::<Vec<_>>(),
            reversed.iter().map(|event| &event.id).collect::<Vec<_>>()
        );
        let counts = ["logs", "network", "marker"].map(|source| {
            let numbers: Vec<_> = events
                .iter()
                .filter(|event| event.source == source)
                .map(line)
                .collect();
            assert_eq!(numbers, (601 - numbers.len()..=600).collect::<Vec<_>>());
            numbers.len()
        });
        assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1);
    }

    #[test]
    fn recent_preview_redistributes_unused_capacity_to_busy_sources() {
        let mut events: Vec<_> = (1..=700)
            .map(|number| preview_event("logs", number))
            .collect();
        events.extend([preview_event("network", 1), preview_event("marker", 1)]);
        retain_recent(&mut events);
        assert_eq!(events.len(), 500);
        assert_eq!(events.first().unwrap().raw_ref, "logs#L203");
        assert_eq!(events[497].raw_ref, "logs#L700");
        assert_eq!(events[498].source, "network");
        assert_eq!(events[499].source, "marker");
    }

    #[test]
    fn recent_preview_below_capacity_preserves_all_events_and_chronological_tiebreaks() {
        let mut early = preview_event("marker", 3);
        early.timestamp = "2026-10-15T00:00:01.000000Z".into();
        let mut events = vec![
            preview_event("marker", 2),
            preview_event("network", 1),
            preview_event("logs", 2),
            preview_event("marker", 1),
            preview_event("logs", 1),
            preview_event("network", 2),
            early,
        ];
        retain_recent(&mut events);
        assert_eq!(
            events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>(),
            [
                "marker-3",
                "logs-1",
                "logs-2",
                "network-1",
                "network-2",
                "marker-1",
                "marker-2"
            ]
        );
    }

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
        let mut events = vec![event; 600];
        retain_recent(&mut events);
        assert_eq!(events.len(), 500);
        let reply = crate::network::Reply::Snapshot(Snapshot::Recording(SessionView {
            id: "fixture-session".into(),
            device: Device {
                serial: "fixture-stream".into(),
                name: "Fixture Android".into(),
                state: DeviceState::Ready,
            },
            directory: "fixture-session".into(),
            started_at: "2026-10-20T00:00:00Z".into(),
            limits: Limits::default(),
            fixture: true,
            bytes: 0,
            logs: 0,
            markers: 0,
            events,
            receipt: None,
            archive: None,
            warnings: vec![],
            network: NetworkView::Recording(CoverageView::default()),
            descriptor_version: 1,
            selection: CaptureSelection::Logs,
            log_scope: LogScope::default(),
            network_policy: NetworkPolicy::default(),
            context: CaptureContext::default(),
            log_attribution: None,
        }));
        assert!(serde_json::to_vec(&reply).unwrap().len() < 4 * 1024 * 1024);
    }
}
