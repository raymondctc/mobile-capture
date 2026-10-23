use crate::{model::Event, timeline};
use chrono::{DateTime, SecondsFormat};
use icu_casemap::CaseMapper;
use serde_json::{json, Value};

pub(super) fn event(source: &str, path: &str, line: u64, raw: &[u8], started: &str) -> Event {
    let mut event = if source == "network" {
        network(raw, started)
    } else if source == "logs" {
        timeline::events(raw, &[], started).remove(0)
    } else {
        timeline::events(&[], raw, started).remove(0)
    };
    let mut identity = format!("{source}\0{path}\0{line}\0").into_bytes();
    identity.extend_from_slice(raw);
    event.id = format!("event-{}", &timeline::hash(&identity)[..24]);
    event.raw_ref = format!("{path}#L{line}");
    event
}

fn network(raw: &[u8], started: &str) -> Event {
    let value: Value = serde_json::from_slice(raw).unwrap_or(json!({
        "kind":"parse_error", "summary":"Unparsed network event",
        "detail":{"parse_error":"Invalid network JSON record"}
    }));
    let mut detail = value["detail"].as_object().cloned().unwrap_or_default();
    for (from, to) in [("id", "source_event_id"), ("raw_ref", "native_ref")] {
        if value[from].is_string() {
            detail.insert(to.into(), value[from].clone());
        }
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
        (started.to_owned(), "session_start_fallback", false, None)
    };
    detail.insert("clock_calibrated".into(), json!(calibrated));
    detail.insert("timestamp_basis".into(), json!(basis));
    detail.insert("temporal_matching".into(), json!("inferred"));
    Event {
        id: String::new(),
        timestamp,
        original_timestamp: value
            .get("original_timestamp")
            .unwrap_or(&value["timestamp"])
            .as_str()
            .map(String::from),
        received_at: received.map(|t| t.to_utc().to_rfc3339_opts(SecondsFormat::Micros, true)),
        source: "network".into(),
        kind: value["kind"].as_str().unwrap_or("network").into(),
        summary: value["summary"].as_str().unwrap_or("Network event").into(),
        raw_ref: String::new(),
        detail: Value::Object(detail),
        clock_uncertainty_ms: uncertainty,
    }
}

pub(super) fn fold(text: &str) -> String {
    CaseMapper::new().fold_string(text).into_owned()
}

pub(super) fn searchable(event: &Event, raw: &[u8], metadata: &Value) -> String {
    let mut full = event.clone();
    let mut bodies = Vec::new();
    for side in [super::BodySide::Request, super::BodySide::Response] {
        if full.detail.get(side.key()).is_some() {
            let (descriptor, bytes) = super::bodies::view(event, side, true);
            if descriptor.available && descriptor.text {
                bodies.push(String::from_utf8(bytes).expect("text body was validated"));
            }
            if let Some(object) = full.detail[side.key()].as_object_mut() {
                object.remove("text");
            }
        }
    }
    let mut text = serde_json::to_string(&full).unwrap_or_default();
    if event.source != "network" {
        text.push('\n');
        text.push_str(&String::from_utf8_lossy(raw));
    }
    text.push('\n');
    text.push_str(&metadata.to_string());
    for body in bodies {
        text.push('\n');
        text.push_str(&body);
    }
    fold(&text)
}
