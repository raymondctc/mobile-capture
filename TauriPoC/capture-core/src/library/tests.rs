use super::*;
use serde_json::json;
use std::{collections::BTreeMap, io::Write};

const STARTED: &str = "2026-10-22T20:00:00.000000Z";

struct Fixture {
    _temp: tempfile::TempDir,
    evidence: PathBuf,
    cache: PathBuf,
}

impl Fixture {
    fn new(legacy: bool) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let evidence = temp.path().join("session");
        let cache = temp.path().join("cache");
        for child in ["logs", "network", "timeline", "runtime"] {
            fs::create_dir_all(evidence.join(child)).unwrap();
        }
        let metadata = if legacy {
            json!({"id":"legacy-poc", "started_at":STARTED,"coordinator":"tauri-poc"})
        } else {
            json!({"version":1,"id":"public-session","device":{"serial":"phone","name":"Android","state":"ready"},
                "started_at":STARTED,"selection":{"mode":"logs"},"context":{"title":"Library STRASSE fixture","notes":"inspect safely","steps":[]}})
        };
        fs::write(
            evidence.join(if legacy {
                "session.json"
            } else {
                "capture.json"
            }),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        Self {
            _temp: temp,
            evidence,
            cache,
        }
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.evidence.join(path), bytes).unwrap();
    }
    fn append(&self, path: &str, bytes: &[u8]) {
        fs::OpenOptions::new()
            .append(true)
            .open(self.evidence.join(path))
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
    fn open(&self, live: bool) -> EvidenceLibrary {
        EvidenceLibrary::open(self.evidence.clone(), self.cache.clone(), live).unwrap()
    }
}

fn finish(library: &mut EvidenceLibrary) {
    for _ in 0..100 {
        if library.refresh().unwrap().complete {
            return;
        }
    }
    panic!("bounded ingestion did not converge");
}

fn all(library: &mut EvidenceLibrary) -> Vec<TimelineRow> {
    library
        .query(TimelineQuery {
            limit: 2000,
            order: TimelineOrder::OldestFirst,
            ..Default::default()
        })
        .unwrap()
        .rows
}

#[test]
fn old_and_public_metadata_inspect_without_tools_or_evidence_writes() {
    for legacy in [false, true] {
        let fixture = Fixture::new(legacy);
        fixture.write(
            "runtime/bootstrap.json",
            br#"{"tools":{"adb":"/not/an/executable","mitmdump":"/also/missing"}}"#,
        );
        fixture.write(
            "logs/device.log",
            b"1700000000.000001 1 2 I Tag: original\n",
        );
        let before = evidence_bytes(&fixture.evidence);
        let mut library = fixture.open(false);
        assert_eq!(library.descriptor().legacy, legacy);
        assert_eq!(library.descriptor().device.is_some(), !legacy);
        let rows = all(&mut library);
        assert_eq!(rows.len(), 1);
        let target = library.raw_target(&rows[0].event.id).unwrap();
        assert_eq!(target.relative_path, "logs/device.log");
        assert_eq!(target.line, 1);
        let detail = library.event(&rows[0].event.id).unwrap().unwrap();
        assert_eq!(detail.event.detail["message"], "original");
        assert_eq!(before, evidence_bytes(&fixture.evidence));
        assert!(!fixture.evidence.join("session.sqlite").exists());
    }
}

#[test]
fn incremental_index_keeps_every_record_and_exact_normalizer_ids_order() {
    let fixture = Fixture::new(true);
    let logs = (0..1500)
        .map(|n| format!("1700000000.000001 1 2 I Tag: message {n}\r\n"))
        .collect::<String>();
    let markers = format!(
        "\n{}\n",
        json!({"timestamp":"2023-11-14T22:13:20.000001Z","text":"marker"})
    );
    let network = format!(
        "{}\n",
        json!({"id":"native-1","timestamp":"2023-11-14T22:13:20.000001Z","kind":"http_response","summary":"response","raw_ref":"network/capture.mitm#F1","detail":{"method":"GET","url":"https://example.test"}})
    );
    fixture.write("logs/device.log", &logs);
    fixture.write("timeline/markers.jsonl", &markers);
    fixture.write("network/events.jsonl", &network);
    let mut library = fixture.open(false);
    let progress = library.index.progress().unwrap();
    assert!(progress.indexed < 1502);
    assert!(progress.backlog_bytes > 0);
    assert!(!progress.complete);
    finish(&mut library);
    let mut expected = timeline::events(logs.as_bytes(), markers.as_bytes(), STARTED);
    expected.extend(timeline::network_events(&fixture.evidence, STARTED, false).unwrap());
    timeline::sort(&mut expected);
    let actual = all(&mut library);
    assert_eq!(actual.len(), 1502);
    assert_eq!(
        actual.iter().map(|r| &r.event.id).collect::<Vec<_>>(),
        expected.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
    let generation = library.index.generation.clone();
    let selected = actual[0].event.id.clone();
    let page = library
        .query(TimelineQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert!(!page.rows.iter().any(|r| r.event.id == selected));
    assert_eq!(
        library.event(&selected).unwrap().unwrap().event.id,
        selected
    );
    for _ in 0..3 {
        assert_eq!(library.refresh().unwrap().indexed, 1502);
    }
    assert_eq!(library.index.generation, generation);
    drop(library);
    let mut reopened = fixture.open(false);
    assert_eq!(reopened.index.generation, generation);
    assert_eq!(all(&mut reopened).len(), 1502);
}

#[test]
fn cursor_queries_bounds_sources_and_unicode_search_are_complete() {
    let fixture = Fixture::new(false);
    let logs = (0..750)
        .map(|n| {
            format!(
                "1700000000.000001 1 2 I Tag: {}\n",
                if n == 0 {
                    "Straße Σίσυφος ﬃ İ".into()
                } else {
                    format!("record {n}")
                }
            )
        })
        .collect::<String>();
    fixture.write("logs/device.log", &logs);
    fixture.write(
        "timeline/markers.jsonl",
        format!("{}\n", json!({"timestamp":STARTED,"text":"marker-only"})),
    );
    let mut library = fixture.open(false);
    finish(&mut library);
    for search in [
        "strasse",
        "ΣΊΣΥΦΟΣ",
        "ffi",
        "i\u{307}",
        "logs/device.log#L1",
        "android_logcat_epoch",
        "Library STRASSE fixture",
    ] {
        let page = library
            .query(TimelineQuery {
                search: search.into(),
                ..Default::default()
            })
            .unwrap();
        assert!(page.matching > 0, "missing full search match for {search}");
    }
    let marker = library
        .query(TimelineQuery {
            source: Some("marker".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(marker.matching, 1);
    let mut request = TimelineQuery {
        limit: 17,
        order: TimelineOrder::OldestFirst,
        ..Default::default()
    };
    let mut ids = Vec::new();
    loop {
        let page = library.query(request.clone()).unwrap();
        ids.extend(page.rows.into_iter().map(|r| r.event.id));
        if page.next_cursor.is_none() {
            break;
        }
        request.cursor = page.next_cursor;
    }
    assert_eq!(ids.len(), 751);
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        ids.len()
    );
    assert_eq!(
        ids,
        all(&mut library)
            .iter()
            .map(|r| r.event.id.clone())
            .collect::<Vec<_>>()
    );
    let frozen = library.refresh().unwrap().latest_sequence;
    fixture.append("logs/device.log", b"1700000001.0 1 2 I Tag: appended\n");
    finish(&mut library);
    let paused = library
        .query(TimelineQuery {
            through_sequence: Some(frozen),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(paused.matching, 751);
    let cleared = library
        .query(TimelineQuery {
            after_sequence: Some(frozen),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(cleared.matching, 1);
    assert!(cleared.rows[0].event.summary.contains("appended"));
    assert!(library
        .query(TimelineQuery {
            limit: 0,
            ..Default::default()
        })
        .is_err());
    assert!(library
        .query(TimelineQuery {
            limit: 2001,
            ..Default::default()
        })
        .is_err());
    assert!(library
        .query(TimelineQuery {
            source: Some("../runtime".into()),
            ..Default::default()
        })
        .is_err());
    assert!(library
        .query(TimelineQuery {
            cursor: Some("invalid".into()),
            ..Default::default()
        })
        .is_err());
    let page = library
        .query(TimelineQuery {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(library
        .query(TimelineQuery {
            limit: 1,
            source: Some("logs".into()),
            cursor: page.next_cursor,
            ..Default::default()
        })
        .is_err());
}

#[test]
fn complete_body_tail_search_chunks_binary_and_unavailable_are_truthful() {
    let fixture = Fixture::new(false);
    let mut text = "a".repeat(5 * 1024 * 1024);
    text.push_str("body-only-tail-ß");
    let encoded = STANDARD.encode(text.as_bytes());
    fixture.write("network/events.jsonl", format!("{}\n{}\n{}\n",
        json!({"timestamp":STARTED,"kind":"http_response","summary":"large body","detail":{
            "response_body":{"encoding":"base64","text":encoded},"request_body":{"unavailable":true,"reason":"stream incomplete"},"headers":[{"name":"x-full","value":"v".repeat(2000)}]}}),
        json!({"timestamp":STARTED,"kind":"http_response","summary":"binary","detail":{"response_body":{"text":STANDARD.encode([0,255,1,2])}}}),
        json!({"timestamp":STARTED,"kind":"http_response","summary":"invalid","detail":{"response_body":{"text":"invalid base64"}}})));
    let before = evidence_bytes(&fixture.evidence);
    let mut library = fixture.open(false);
    assert!(!library.index.progress().unwrap().complete);
    assert_eq!(library.index.progress().unwrap().indexed, 0);
    finish(&mut library);
    let found = library
        .query(TimelineQuery {
            search: "BODY-ONLY-TAIL-SS".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(found.matching, 1);
    let id = &found.rows[0].event.id;
    let detail = library.event(id).unwrap().unwrap();
    assert_eq!(
        detail
            .bodies
            .iter()
            .find(|b| b.side == BodySide::Response)
            .unwrap()
            .bytes,
        text.len() as u64
    );
    assert!(detail.event.detail["response_body"].get("text").is_none());
    assert_eq!(
        detail.event.detail["headers"][0]["value"]
            .as_str()
            .unwrap()
            .len(),
        2000
    );
    let offset = text.len() as u64 - 64;
    let tail = library
        .body_chunk(id, BodySide::Response, offset, 64)
        .unwrap();
    assert!(tail.body.text);
    assert_eq!(
        STANDARD.decode(tail.chunk.data_base64).unwrap(),
        text.as_bytes()[offset as usize..]
    );
    assert!(tail.chunk.next_offset.is_none());
    let missing = library.body_chunk(id, BodySide::Request, 0, 64).unwrap();
    assert!(!missing.body.available);
    assert_eq!(missing.body.reason.as_deref(), Some("stream incomplete"));
    let raw_detail = library.detail_chunk(id, 0, 65536).unwrap();
    assert_eq!(
        STANDARD.decode(raw_detail.data_base64).unwrap().len(),
        65536
    );
    assert!(raw_detail.next_offset.is_some());
    let rows = all(&mut library);
    let binary_id = &rows
        .iter()
        .find(|r| r.event.summary == "binary")
        .unwrap()
        .event
        .id;
    let binary = library
        .body_chunk(binary_id, BodySide::Response, 0, 64)
        .unwrap();
    assert!(binary.body.available);
    assert!(!binary.body.text);
    assert_eq!(
        STANDARD.decode(binary.chunk.data_base64).unwrap(),
        [0, 255, 1, 2]
    );
    let invalid_id = &rows
        .iter()
        .find(|r| r.event.summary == "invalid")
        .unwrap()
        .event
        .id;
    assert!(
        !library
            .body_chunk(invalid_id, BodySide::Response, 0, 64)
            .unwrap()
            .body
            .available
    );
    assert!(library
        .body_chunk(id, BodySide::Response, 0, 65537)
        .is_err());
    assert!(library
        .body_chunk(id, BodySide::Response, text.len() as u64 + 1, 64)
        .is_err());
    assert_eq!(before, evidence_bytes(&fixture.evidence));
}

#[test]
fn compressed_body_search_and_decoded_chunks_keep_originals_and_reject_bombs() {
    use flate2::{
        write::{GzEncoder, ZlibEncoder},
        Compression,
    };
    let fixture = Fixture::new(false);
    let text = format!("{}compressed-only-tail-ß", "decoded ".repeat(30000));
    let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
    gzip.write_all(text.as_bytes()).unwrap();
    let gzip = gzip.finish().unwrap();
    let mut deflate = ZlibEncoder::new(Vec::new(), Compression::fast());
    deflate.write_all(b"deflate-only-content").unwrap();
    let deflate = deflate.finish().unwrap();
    let mut bomb = GzEncoder::new(Vec::new(), Compression::fast());
    bomb.write_all(&vec![b'x'; 32 * 1024 * 1024 + 1]).unwrap();
    let bomb = bomb.finish().unwrap();
    let records = [("gzip",gzip.as_slice()),("deflate",deflate.as_slice()),("gzip",bomb.as_slice())].into_iter().enumerate().map(|(n,(encoding,bytes))| format!("{}\n",json!({
        "timestamp":STARTED,"summary":format!("compressed {n}"),"detail":{
            "response_headers":[{"name":"Content-Encoding","value":encoding}],
            "response_body":{"encoding":"base64","text":STANDARD.encode(bytes),"byte_basis":"raw_content"}}}))).collect::<String>();
    fixture.write("network/events.jsonl", records);
    let before = evidence_bytes(&fixture.evidence);
    let mut library = fixture.open(false);
    finish(&mut library);
    let found = library
        .query(TimelineQuery {
            search: "COMPRESSED-ONLY-TAIL-SS".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(found.matching, 1);
    let id = &found.rows[0].event.id;
    let original = library
        .body_chunk(id, BodySide::Response, 0, 65536)
        .unwrap();
    assert_eq!(STANDARD.decode(original.chunk.data_base64).unwrap(), gzip);
    assert_eq!(original.body.byte_basis, "raw_content");
    assert!(original.body.decoded_available);
    let decoded = library
        .decoded_body_chunk(id, BodySide::Response, text.len() as u64 - 64, 64)
        .unwrap();
    assert_eq!(
        STANDARD.decode(decoded.chunk.data_base64).unwrap(),
        text.as_bytes()[text.len() - 64..]
    );
    assert_eq!(decoded.body.byte_basis, "content");
    assert!(decoded.body.text);
    let found_deflate = library
        .query(TimelineQuery {
            search: "deflate-only-content".into(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(found_deflate.matching, 1);
    let rows = all(&mut library);
    let bomb_id = &rows
        .iter()
        .find(|r| r.event.summary == "compressed 2")
        .unwrap()
        .event
        .id;
    let bomb_view = library
        .decoded_body_chunk(bomb_id, BodySide::Response, 0, 64)
        .unwrap();
    assert!(!bomb_view.body.available);
    assert_eq!(
        bomb_view.body.decoded_reason.as_deref(),
        Some("Decoded body exceeds 32 MiB")
    );
    assert!(bomb_view.chunk.data_base64.is_empty());
    let original_bomb = library
        .body_chunk(bomb_id, BodySide::Response, 0, 64)
        .unwrap();
    assert!(original_bomb.body.available);
    assert!(!original_bomb.body.decoded_available);
    assert_eq!(before, evidence_bytes(&fixture.evidence));
}

#[test]
fn incomplete_appends_are_deferred_and_final_tail_is_inspectable() {
    let fixture = Fixture::new(true);
    fixture.write(
        "logs/device.log",
        b"1700000000.0 1 2 I Tag: complete\n1700000000.1 1 2 I Tag: part",
    );
    let mut library = fixture.open(true);
    assert_eq!(all(&mut library).len(), 1);
    assert!(!library.refresh().unwrap().complete);
    fixture.append("logs/device.log", b"ial\r");
    library.refresh().unwrap();
    assert_eq!(all(&mut library).len(), 1);
    fixture.append("logs/device.log", b"\n1700000000.2 1 2 I Tag: final tail");
    library.refresh().unwrap();
    assert_eq!(all(&mut library).len(), 2);
    library.set_live(false);
    finish(&mut library);
    let rows = all(&mut library);
    assert_eq!(rows.len(), 3);
    let expected = timeline::events(
        &fs::read(fixture.evidence.join("logs/device.log")).unwrap(),
        &[],
        STARTED,
    );
    assert_eq!(
        rows.iter().map(|r| &r.event.id).collect::<Vec<_>>(),
        expected.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
}

#[test]
fn raw_replacement_and_truncation_invalidate_generation_bound_cursors() {
    let fixture = Fixture::new(true);
    fixture.write("logs/device.log", b"first\nsecond\nthird\n");
    let mut library = fixture.open(false);
    let page = library
        .query(TimelineQuery {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    let old_id = page.rows[0].event.id.clone();
    fixture.write("logs/replacement.log", b"replacement\nrecords\n");
    fs::rename(
        fixture.evidence.join("logs/replacement.log"),
        fixture.evidence.join("logs/device.log"),
    )
    .unwrap();
    library.refresh().unwrap();
    assert!(library
        .query(TimelineQuery {
            limit: 1,
            cursor: page.next_cursor,
            ..Default::default()
        })
        .is_err());
    assert!(library.event(&old_id).unwrap().is_none());
    let replacement = library
        .query(TimelineQuery {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_ne!(replacement.generation, page.generation);
    fixture.write("logs/device.log", b"short\n");
    library.refresh().unwrap();
    let short = all(&mut library);
    assert_eq!(short.len(), 1);
    assert_eq!(short[0].event.summary, "short");
    assert!(library
        .query(TimelineQuery {
            limit: 1,
            cursor: replacement.next_cursor,
            ..Default::default()
        })
        .is_err());
}

#[test]
fn one_cache_writer_and_bounded_catalog_are_enforced() {
    let fixture = Fixture::new(false);
    let first = fixture.open(false);
    assert!(EvidenceLibrary::open(fixture.evidence.clone(), fixture.cache.clone(), false).is_err());
    assert!(EvidenceLibrary::open(
        fixture.evidence.clone(),
        fixture.evidence.join("cache"),
        false
    )
    .is_err());
    assert!(!fixture.evidence.join("cache").exists());
    drop(first);
    let _second = fixture.open(false);
    let catalog = EvidenceLibrary::catalog(fixture.evidence.parent().unwrap(), 1).unwrap();
    assert_eq!(catalog.sessions.len(), 1);
    assert_eq!(catalog.sessions[0].id, "public-session");
    assert!(EvidenceLibrary::catalog(fixture.evidence.parent().unwrap(), 201).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_evidence_metadata_directory_and_cache_alias_are_refused() {
    use std::os::unix::fs::symlink;
    for target in [
        "capture.json",
        "logs/device.log",
        "network/events.jsonl",
        "timeline/markers.jsonl",
    ] {
        let fixture = Fixture::new(false);
        let outside = fixture._temp.path().join("outside");
        fs::write(&outside, b"private outside bytes").unwrap();
        if fixture.evidence.join(target).exists() {
            fs::remove_file(fixture.evidence.join(target)).unwrap();
        }
        symlink(&outside, fixture.evidence.join(target)).unwrap();
        assert!(
            EvidenceLibrary::open(fixture.evidence, fixture.cache, false).is_err(),
            "admitted {target} symlink"
        );
    }
    let fixture = Fixture::new(false);
    let alias = fixture._temp.path().join("alias");
    symlink(&fixture.evidence, &alias).unwrap();
    assert!(EvidenceLibrary::open(alias.clone(), fixture.cache.clone(), false).is_err());
    assert!(EvidenceLibrary::open(fixture.evidence.clone(), alias.join("cache"), false).is_err());
    assert!(!fixture.evidence.join("cache").exists());
    assert!(files::open(&fixture.evidence, "../outside").is_err());
    let mut library = fixture.open(false);
    fixture.write("logs/device.log", b"safe row\n");
    library.refresh().unwrap();
    let row = all(&mut library).remove(0);
    fs::remove_file(fixture.evidence.join("logs/device.log")).unwrap();
    symlink("/etc/passwd", fixture.evidence.join("logs/device.log")).unwrap();
    assert!(library.raw_target(&row.event.id).is_err());
}

fn evidence_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(evidence_bytes(&path));
        } else {
            result.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    result
}
