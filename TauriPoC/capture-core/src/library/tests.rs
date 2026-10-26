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
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: id.to_string(),
            },
            BodySide::Response,
            offset,
            64,
        )
        .unwrap();
    assert!(tail.body.text);
    assert_eq!(
        STANDARD.decode(tail.chunk.data_base64).unwrap(),
        text.as_bytes()[offset as usize..]
    );
    assert!(tail.chunk.next_offset.is_none());
    let missing = library
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: id.to_string(),
            },
            BodySide::Request,
            0,
            64,
        )
        .unwrap();
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
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: binary_id.to_string(),
            },
            BodySide::Response,
            0,
            64,
        )
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
            .body_chunk(
                &EventRef {
                    session_key: library.session_key.clone(),
                    event_id: invalid_id.to_string()
                },
                BodySide::Response,
                0,
                64
            )
            .unwrap()
            .body
            .available
    );
    assert!(library
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: id.to_string()
            },
            BodySide::Response,
            0,
            65537
        )
        .is_err());
    assert!(library
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: id.to_string()
            },
            BodySide::Response,
            text.len() as u64 + 1,
            64
        )
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
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: id.to_string(),
            },
            BodySide::Response,
            0,
            65536,
        )
        .unwrap();
    assert_eq!(STANDARD.decode(original.chunk.data_base64).unwrap(), gzip);
    assert_eq!(original.body.byte_basis, "raw_content");
    assert!(original.body.decoded_available);
    let decoded = library
        .decoded_body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: id.to_string(),
            },
            BodySide::Response,
            text.len() as u64 - 64,
            64,
        )
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
        .decoded_body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: bomb_id.to_string(),
            },
            BodySide::Response,
            0,
            64,
        )
        .unwrap();
    assert!(!bomb_view.body.available);
    assert_eq!(
        bomb_view.body.decoded_reason.as_deref(),
        Some("Decoded body exceeds 32 MiB")
    );
    assert!(bomb_view.chunk.data_base64.is_empty());
    let original_bomb = library
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: bomb_id.to_string(),
            },
            BodySide::Response,
            0,
            64,
        )
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

#[test]
fn middle_record_mutation_then_append_rebuilds_generation_and_search() {
    let fixture = Fixture::new(false);
    let old = "1700000000.0 1 2 I Tag: old-middle-record";
    let new = "1700000000.0 1 2 I Tag: new-middle-record";
    let padding = "1700000000.0 1 2 I Tag: padding-record\n".repeat(180);
    let original = format!("{padding}{old}\n{padding}");
    fixture.write("logs/device.log", &original);
    let mut library = fixture.open(true);
    finish(&mut library);
    let first = all(&mut library);
    let generation = library.index.generation.clone();
    fixture.write("logs/device.log", original.replace(old, new));
    fixture.append("logs/device.log", b"1700000001.0 1 2 I Tag: appended\n");
    finish(&mut library);
    assert_ne!(
        library.index.generation, generation,
        "mutated prefix must invalidate generation"
    );
    for (search, expected) in [
        ("old-middle-record", 0),
        ("new-middle-record", 1),
        ("appended", 1),
    ] {
        let page = library
            .query(TimelineQuery {
                search: search.into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.matching, expected, "wrong search result for {search}");
    }
    let rows = all(&mut library);
    assert_eq!(rows.len(), first.len() + 1);
    assert_eq!(rows[0].event.id, first[0].event.id);
    assert_eq!(library.raw_target(&rows[180].event.id).unwrap().line, 181);
    let stable_generation = library.index.generation.clone();
    fixture.append(
        "logs/device.log",
        b"1700000002.0 1 2 I Tag: ordinary append\n",
    );
    finish(&mut library);
    assert_eq!(library.index.generation, stable_generation);
}

#[test]
fn hostile_body_metadata_stays_bounded_and_full_detail_is_chunked() {
    for unavailable in [false, true] {
        let fixture = Fixture::new(false);
        let reason = format!("{}full-reason-tail", "reason".repeat(210_000));
        fixture.write("network/events.jsonl", format!("{}\n", json!({
            "timestamp": STARTED, "summary": "large metadata", "detail": {
                "response_body": {"unavailable": unavailable, "reason": reason, "extra": "extra".repeat(210_000), "text": STANDARD.encode(b"body")}
            }
        })));
        let mut library = fixture.open(false);
        finish(&mut library);
        let page = library.query(TimelineQuery::default()).unwrap();
        let row = &page.rows[0];
        assert!(
            serde_json::to_vec(row).unwrap().len() <= 6144,
            "query row exceeds 6 KiB"
        );
        let detail = library.event(&row.event.id).unwrap().unwrap();
        assert!(!detail.detail_complete);
        assert!(
            serde_json::to_vec(&detail).unwrap().len() <= 256 * 1024,
            "detail fallback exceeds 256 KiB"
        );
        let total = library
            .detail_chunk(&row.event.id, 0, 1)
            .unwrap()
            .total_bytes;
        let tail = library
            .detail_chunk(&row.event.id, total - 128, 128)
            .unwrap();
        assert!(STANDARD
            .decode(tail.data_base64)
            .unwrap()
            .windows(16)
            .any(|s| s == b"full-reason-tail"));
        assert_eq!(
            library
                .query(TimelineQuery {
                    search: "full-reason-tail".into(),
                    ..Default::default()
                })
                .unwrap()
                .matching,
            1
        );
    }
}

#[test]
fn preview_bounds_escaped_outer_fields_and_body_metadata() {
    let mut event = crate::model::Event {
        id: "\u{0}".repeat(10000),
        timestamp: "\u{0}".repeat(10000),
        original_timestamp: Some("\u{0}".repeat(10000)),
        received_at: Some("\u{0}".repeat(10000)),
        source: "\u{0}".repeat(10000),
        kind: "\u{0}".repeat(10000),
        summary: "\u{0}".repeat(10000),
        raw_ref: "\u{0}".repeat(10000),
        detail: json!({"response_body":{"reason":"\u{0}".repeat(10000),"text":STANDARD.encode(vec![0; 2048])}}),
        clock_uncertainty_ms: None,
    };
    timeline::preview(&mut event);
    let row = TimelineRow {
        sequence: u64::MAX,
        event,
    };
    assert!(serde_json::to_vec(&row).unwrap().len() <= 6144);
}

#[test]
fn body_documents_preserve_complete_text_and_distinguish_empty_binary_missing_and_decoding() {
    use flate2::{write::GzEncoder, Compression};
    let fixture = Fixture::new(false);
    let original = format!("{}9007199254740993, -0, 1e+09", "tail ".repeat(20_000));
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(original.as_bytes()).unwrap();
    let compressed = gzip.finish().unwrap();
    let records = [
        json!({"summary":"compressed", "detail":{"request_body":{"text":""}, "response_headers":[{"name":"content-encoding","value":"gzip"}], "response_body":{"text":STANDARD.encode(&compressed)}}}),
        json!({"summary":"binary", "detail":{"response_body":{"text":STANDARD.encode([0,255])}}}),
        json!({"summary":"unsupported", "detail":{"response_headers":[{"name":"content-encoding","value":"br"}], "response_body":{"text":"YQ=="}}}),
    ];
    fixture.write(
        "network/events.jsonl",
        records.iter().map(|r| format!("{r}\n")).collect::<String>(),
    );
    let before = evidence_bytes(&fixture.evidence);
    let mut library = fixture.open(false);
    finish(&mut library);
    let rows = all(&mut library);
    let pair = library
        .body_documents(&EventRef {
            session_key: library.session_key.clone(),
            event_id: rows[0].event.id.clone(),
        })
        .unwrap();
    assert!(
        matches!(pair.request.content, BodyContent::Text { ref original, bytes: 0, .. } if original.is_empty())
    );
    assert!(
        matches!(pair.response.content, BodyContent::Text { original: ref text, ref basis, .. } if text == &original && matches!(basis, BodyTextBasis::Decoded))
    );
    let raw = library
        .body_chunk(
            &EventRef {
                session_key: library.session_key.clone(),
                event_id: rows[0].event.id.clone(),
            },
            BodySide::Response,
            0,
            65536,
        )
        .unwrap();
    assert_eq!(STANDARD.decode(raw.chunk.data_base64).unwrap(), compressed);
    let pair = library
        .body_documents(&EventRef {
            session_key: library.session_key.clone(),
            event_id: rows[1].event.id.clone(),
        })
        .unwrap();
    assert!(matches!(
        pair.request.retained,
        Retained::Unavailable { .. }
    ));
    assert!(matches!(
        pair.response.content,
        BodyContent::Binary { bytes: 2, .. }
    ));
    let pair = library
        .body_documents(&EventRef {
            session_key: library.session_key.clone(),
            event_id: rows[2].event.id.clone(),
        })
        .unwrap();
    assert!(
        matches!(pair.response.content, BodyContent::Unavailable { ref reason } if reason.contains("Unsupported"))
    );
    assert_eq!(before, evidence_bytes(&fixture.evidence));
}

#[test]
fn exact_log_level_filters_counts_pages_and_cursors_but_preserves_other_sources() {
    let fixture = Fixture::new(false);
    fixture.write("logs/device.log", "1700000000.0 1 2 I Tag: info\n1700000000.0 1 2 E Tag: error one\n1700000000.0 1 2 W Tag: warning\n1700000000.0 1 2 E Tag: error two\nunparsed\n");
    fixture.write(
        "network/events.jsonl",
        format!("{}\n", json!({"timestamp":STARTED,"summary":"network"})),
    );
    fixture.write(
        "timeline/markers.jsonl",
        format!("{}\n", json!({"timestamp":STARTED,"text":"marker"})),
    );
    let mut library = fixture.open(false);
    finish(&mut library);
    let query = TimelineQuery {
        log_level: Some(LogLevel::E),
        limit: 1,
        order: TimelineOrder::OldestFirst,
        ..Default::default()
    };
    let first = library.query(query.clone()).unwrap();
    assert_eq!(first.total, 7);
    assert_eq!(first.matching, 4);
    assert_eq!(first.rows[0].event.detail["level"], "E");
    let cursor = first.next_cursor.clone();
    let mut ids = vec![first.rows[0].event.id.clone()];
    let mut next = cursor.clone();
    while let Some(cursor) = next {
        let page = library
            .query(TimelineQuery {
                cursor: Some(cursor),
                ..query.clone()
            })
            .unwrap();
        assert_eq!(page.matching, 4);
        ids.extend(page.rows.iter().map(|r| r.event.id.clone()));
        next = page.next_cursor;
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4);
    assert!(library
        .query(TimelineQuery {
            log_level: Some(LogLevel::W),
            cursor,
            ..query
        })
        .is_err());
    let unknown = library
        .query(TimelineQuery {
            source: Some("logs".into()),
            log_level: Some(LogLevel::Unknown),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(unknown.matching, 1);
    assert_eq!(unknown.rows[0].event.detail["level"], "unknown");
    assert!(serde_json::from_value::<TimelineQuery>(json!({"log_level":"bogus"})).is_err());
}

#[test]
fn version_two_cache_is_recreated_with_levels_and_a_fresh_generation() {
    let fixture = Fixture::new(false);
    fixture.write("logs/device.log", "1700000000.0 1 2 W Tag: warning\n");
    let library = fixture.open(false);
    let old_generation = library.index.generation.clone();
    library
        .index
        .db
        .execute("UPDATE meta SET value='2' WHERE key='version'", [])
        .unwrap();
    library
        .index
        .db
        .execute_batch("DROP INDEX timeline_log_level; ALTER TABLE events DROP COLUMN log_level;")
        .unwrap();
    drop(library);
    let mut reopened = fixture.open(false);
    finish(&mut reopened);
    assert_ne!(reopened.index.generation, old_generation);
    let page = reopened
        .query(TimelineQuery {
            log_level: Some(LogLevel::W),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page.matching, 1);
    assert_eq!(page.rows[0].event.detail["level"], "W");
}

#[test]
fn preview_keeps_compact_presentation_when_generic_metadata_is_pruned() {
    let fixture = Fixture::new(false);
    fixture.write(
        "logs/device.log",
        format!(
            "1700000000.0 123 456 E {}: {}\n",
            "tag".repeat(5000),
            "message".repeat(5000)
        ),
    );
    fixture.write("network/events.jsonl", format!("{}\n", json!({"detail":{"method":"PATCH","status":503,"duration_ms":123.25,"url":"https://example.test", "aaa":vec!["x".repeat(9999); 100]}})));
    let mut library = fixture.open(false);
    finish(&mut library);
    let rows = all(&mut library);
    assert_eq!(rows[0].event.detail["level"], "E");
    assert_eq!(rows[0].event.detail["pid"], 123);
    assert_eq!(rows[0].event.detail["tid"], 456);
    assert!(rows[0].event.detail["tag"]
        .as_str()
        .unwrap()
        .starts_with("tag"));
    assert!(rows[0].event.detail["message"]
        .as_str()
        .unwrap()
        .starts_with("message"));
    let detail = &rows[1].event.detail;
    assert_eq!(detail["method"], "PATCH");
    assert_eq!(detail["status"], 503);
    assert_eq!(detail["duration_ms"], 123.25);
    for row in rows {
        assert!(serde_json::to_vec(&row.event).unwrap().len() < 6 * 1024 - 96);
    }
}

#[test]
fn escape_heavy_compact_fields_stay_within_the_serialized_row_budget() {
    let mut event = Event {
        id: "\0".repeat(128),
        timestamp: STARTED.into(),
        original_timestamp: Some("\0".repeat(64)),
        received_at: Some("\0".repeat(64)),
        source: "network".into(),
        kind: "\0".repeat(128),
        summary: "\0".repeat(1024),
        raw_ref: "\0".repeat(512),
        clock_uncertainty_ms: None,
        detail: json!({"level":"\0".repeat(16), "tag":"\0".repeat(96), "message":"\0".repeat(256),
            "method":"\0".repeat(16), "url":"\0".repeat(256), "status":503, "duration_ms":123.5,
            "extra":vec!["x".repeat(500);100]}),
    };
    timeline::preview(&mut event);
    assert!(serde_json::to_vec(&event).unwrap().len() <= 6 * 1024 - 96);
    assert!(event.detail["message"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert_eq!(event.detail["status"], 503);
    assert_eq!(event.detail["duration_ms"], 123.5);
}

#[test]
fn empty_encoded_response_remains_an_available_empty_text_document() {
    let fixture = Fixture::new(false);
    fixture.write("network/events.jsonl", format!("{}\n", json!({"detail":{"method":"HEAD", "status":204,
        "response_headers":[{"name":"Content-Encoding", "value":"gzip"}], "response_body":{"text":""}}})));
    let mut library = fixture.open(false);
    let rows = all(&mut library);
    let pair = library
        .body_documents(&EventRef {
            session_key: library.session_key.clone(),
            event_id: rows[0].event.id.clone(),
        })
        .unwrap();
    assert!(
        matches!(pair.response.content, BodyContent::Text { original, bytes: 0, basis: BodyTextBasis::Retained } if original.is_empty())
    );
}

#[test]
fn complete_request_metadata_and_retained_snapshots_are_session_bound() {
    let one = Fixture::new(false);
    let two = Fixture::new(false);
    let url = format!(
        "https://example.invalid/path?start=1&tail={}",
        "q".repeat(300_000)
    );
    let record = json!({"detail":{"method":"PATCH","url":url,"request_headers":[{"name":"Content-Type","value":"application/x-protobuf; charset=binary"},{"name":"Content-Encoding","value":"br"}],"request_body":{"text":"AP9B"},"response_body":{"text":""}}});
    for fixture in [&one, &two] {
        fixture.write("network/events.jsonl", format!("{record}\n"));
    }
    let mut first = one.open(false);
    let mut second = two.open(false);
    finish(&mut first);
    finish(&mut second);
    let id = all(&mut first)[0].event.id.clone();
    assert_eq!(id, all(&mut second)[0].event.id);
    let preview = first.event(&id).unwrap().unwrap();
    assert!(!preview.detail_complete);
    assert!(preview.event.detail["url"].as_str().unwrap().len() < url.len());
    let reference = EventRef {
        session_key: preview.session_key,
        event_id: id,
    };
    let pair = first.body_documents(&reference).unwrap();
    assert_eq!(pair.request_info.url.as_deref(), Some(url.as_str()));
    assert_eq!(pair.request_info.method.as_deref(), Some("PATCH"));
    assert_eq!(pair.request.media.label, "Protobuf");
    assert_eq!(
        pair.request.media.content_type.as_deref(),
        Some("application/x-protobuf; charset=binary")
    );
    assert_eq!(pair.request.media.content_encoding.as_deref(), Some("br"));
    assert!(matches!(
        pair.request.content,
        BodyContent::Unavailable { .. }
    ));
    assert!(matches!(
        pair.request.retained,
        Retained::Available { bytes: 3 }
    ));
    let snapshot = first.retained_body(&reference, BodySide::Request).unwrap();
    assert_eq!(snapshot.bytes, [0, 255, 65]);
    assert!(second.body_documents(&reference).is_err());
    assert!(second
        .body_chunk(&reference, BodySide::Request, 0, 64)
        .is_err());
    assert!(second.retained_body(&reference, BodySide::Request).is_err());
    assert_eq!(snapshot.bytes, [0, 255, 65]);
}

#[test]
fn complete_headers_keep_order_case_duplicates_and_per_side_failures() {
    let fixture = Fixture::new(false);
    let long = format!("{}needle", "x".repeat(300_000));
    let rows = json!([{"name":"X-Mixed","value":"first"},{"name":"x-mixed","value":long}]);
    fixture.write("network/events.jsonl", format!("{}\n{}\n{}\n",
        json!({"detail":{"request_headers":rows,"response_headers":[],"request_body":{"text":"body"}}}),
        json!({"detail":{"request_headers":[{"name":"invalid","value":7}],"response_headers":[{"name":"Valid","value":"yes"}],"request_body":{"text":STANDARD.encode("still usable")}}}),
        json!({"detail":{"response_headers":null}})));
    let mut library = fixture.open(false);
    let events = all(&mut library);
    let pairs = events
        .iter()
        .map(|row| {
            library
                .body_documents(&EventRef {
                    session_key: library.session_key.clone(),
                    event_id: row.event.id.clone(),
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(&pairs[0].request_headers, HeaderDocument::Present { rows } if rows.len() == 2 && rows[0].name == "X-Mixed" && rows[1].name == "x-mixed" && rows[1].value == long)
    );
    assert!(
        matches!(&pairs[0].response_headers, HeaderDocument::Present { rows } if rows.is_empty())
    );
    assert!(matches!(
        &pairs[1].request_headers,
        HeaderDocument::Unavailable { .. }
    ));
    assert!(
        matches!(&pairs[1].response_headers, HeaderDocument::Present { rows } if rows[0].value == "yes")
    );
    assert!(
        matches!(&pairs[1].request.content, BodyContent::Text { original, .. } if original == "still usable")
    );
    assert!(matches!(&pairs[2].request_headers, HeaderDocument::Absent));
    assert!(matches!(&pairs[2].response_headers, HeaderDocument::Absent));
}

#[test]
fn checked_seek_preserves_tuple_pages_filters_fences_and_cursor_identity() {
    let fixture = Fixture::new(false);
    let records = (0..455)
        .map(|n| {
            format!(
                "{}\n",
                json!({
                    "timestamp":format!("2023-11-14T22:13:{:02}.000001Z", 59 - n / 10),
                    "summary":if n % 2 == 0 { "keep" } else { "omit" },
                    "detail":{"request_body":{"text":"body"}},
                })
            )
        })
        .collect::<String>();
    fixture.write("network/events.jsonl", records);
    fixture.write(
        "logs/device.log",
        "1700000000.000001 1 2 E Tag: keep\n1700000000.000001 1 2 I Tag: keep\n",
    );
    fixture.write(
        "timeline/markers.jsonl",
        format!(
            "{}\n",
            json!({"timestamp":"2023-11-14T22:13:20.000001Z","text":"keep"})
        ),
    );
    let mut library = fixture.open(true);
    finish(&mut library);
    for order in [TimelineOrder::OldestFirst, TimelineOrder::NewestFirst] {
        for filtered in [false, true] {
            let query = TimelineQuery {
                order,
                limit: 37,
                search: if filtered {
                    "keep".into()
                } else {
                    String::new()
                },
                log_level: Some(LogLevel::E),
                ..Default::default()
            };
            let expected = library
                .query(TimelineQuery {
                    limit: 2000,
                    ..query.clone()
                })
                .unwrap();
            let ids = expected
                .rows
                .iter()
                .map(|row| row.event.id.clone())
                .collect::<Vec<_>>();
            let first = library.query(query.clone()).unwrap();
            let second = library
                .query(TimelineQuery {
                    cursor: first.next_cursor.clone(),
                    ..query.clone()
                })
                .unwrap();
            let frozen = TimelineQuery {
                through_sequence: Some(first.progress.latest_sequence),
                ..query.clone()
            };
            let seek = |id: &str, relation| TimelineSeek {
                session_key: first.session_key.clone(),
                generation: first.generation.clone(),
                event_id: id.into(),
                relation,
            };
            let at = library
                .query(TimelineQuery {
                    seek: Some(seek(&second.rows[0].event.id, SeekRelation::At)),
                    ..frozen.clone()
                })
                .unwrap();
            assert_eq!(
                at.rows.iter().map(|r| &r.event.id).collect::<Vec<_>>(),
                second.rows.iter().map(|r| &r.event.id).collect::<Vec<_>>()
            );
            assert!(at.has_previous && at.has_next);
            let previous = library
                .query(TimelineQuery {
                    seek: Some(seek(&at.rows[0].event.id, SeekRelation::Before)),
                    ..frozen.clone()
                })
                .unwrap();
            assert_eq!(
                previous
                    .rows
                    .iter()
                    .map(|r| &r.event.id)
                    .collect::<Vec<_>>(),
                first.rows.iter().map(|r| &r.event.id).collect::<Vec<_>>()
            );
            assert!(!previous.has_previous && previous.has_next);
            let mut walked = previous
                .rows
                .iter()
                .map(|r| r.event.id.clone())
                .collect::<Vec<_>>();
            let mut current = previous;
            while current.has_next {
                current = library
                    .query(TimelineQuery {
                        seek: Some(seek(
                            &current.rows.last().unwrap().event.id,
                            SeekRelation::After,
                        )),
                        ..frozen.clone()
                    })
                    .unwrap();
                assert!(current.has_previous);
                walked.extend(current.rows.iter().map(|r| r.event.id.clone()));
            }
            assert_eq!(walked, ids);
            assert!(library
                .query(TimelineQuery {
                    seek: Some(seek(&ids[0], SeekRelation::Before)),
                    ..frozen.clone()
                })
                .unwrap()
                .rows
                .is_empty());
            assert!(library
                .query(TimelineQuery {
                    seek: Some(seek(ids.last().unwrap(), SeekRelation::After)),
                    ..frozen.clone()
                })
                .unwrap()
                .rows
                .is_empty());
            let via_cursor = library
                .query(TimelineQuery {
                    cursor: at.next_cursor.clone(),
                    ..frozen.clone()
                })
                .unwrap();
            assert_eq!(via_cursor.rows[0].event.id, ids[74]);
            assert!(library
                .query(TimelineQuery {
                    seek: Some(seek(&ids[0], SeekRelation::At)),
                    cursor: at.next_cursor,
                    ..frozen.clone()
                })
                .is_err());
            for (session_key, generation) in [
                ("wrong", first.generation.as_str()),
                (first.session_key.as_str(), "stale"),
            ] {
                assert!(library
                    .query(TimelineQuery {
                        seek: Some(TimelineSeek {
                            session_key: session_key.into(),
                            generation: generation.into(),
                            ..seek(&ids[0], SeekRelation::At)
                        }),
                        ..frozen.clone()
                    })
                    .is_err());
            }
            assert!(library
                .query(TimelineQuery {
                    seek: Some(seek(&ids[0], SeekRelation::At)),
                    search: "never-matches".into(),
                    ..frozen.clone()
                })
                .is_err());
        }
    }
    let first = library.query(TimelineQuery::default()).unwrap();
    fixture.append(
        "network/events.jsonl",
        format!(
            "{}\n",
            json!({"timestamp":"2023-11-14T22:13:59.000001Z","summary":"appended"})
        )
        .as_bytes(),
    );
    finish(&mut library);
    let frozen = library
        .query(TimelineQuery {
            through_sequence: Some(first.progress.latest_sequence),
            seek: Some(TimelineSeek {
                session_key: first.session_key,
                generation: first.generation,
                event_id: first.rows[0].event.id.clone(),
                relation: SeekRelation::At,
            }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(frozen.matching, first.matching);
    assert!(frozen.progress.latest_sequence > first.progress.latest_sequence);
    assert!(frozen
        .rows
        .iter()
        .all(|row| row.sequence <= first.progress.latest_sequence));
    let stale_seek = TimelineSeek {
        session_key: frozen.session_key,
        generation: frozen.generation,
        event_id: frozen.rows[0].event.id.clone(),
        relation: SeekRelation::At,
    };
    fixture.write(
        "network/events.jsonl",
        format!("{}\n", json!({"summary":"replacement"})),
    );
    finish(&mut library);
    assert!(library
        .query(TimelineQuery {
            seek: Some(stale_seek),
            ..Default::default()
        })
        .unwrap_err()
        .contains("generation"));
}

#[test]
fn seek_uses_id_to_break_equal_timestamp_rank_and_line_ties() {
    let fixture = Fixture::new(false);
    fixture.write("network/events.jsonl", "{}\n{}\n{}\n");
    let mut library = fixture.open(false);
    library
        .index
        .db
        .execute("UPDATE events SET line=1", [])
        .unwrap();
    let mut expected = all(&mut library)
        .iter()
        .map(|row| row.event.id.clone())
        .collect::<Vec<_>>();
    expected.sort();
    for order in [TimelineOrder::OldestFirst, TimelineOrder::NewestFirst] {
        let mut ids = expected.clone();
        if order == TimelineOrder::NewestFirst {
            ids.reverse();
        }
        let first = library
            .query(TimelineQuery {
                order,
                limit: 1,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(first.rows[0].event.id, ids[0]);
        let seek = |event_id: &str, relation| TimelineSeek {
            session_key: first.session_key.clone(),
            generation: first.generation.clone(),
            event_id: event_id.into(),
            relation,
        };
        let second = library
            .query(TimelineQuery {
                order,
                limit: 1,
                seek: Some(seek(&ids[0], SeekRelation::After)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(second.rows[0].event.id, ids[1]);
        let before = library
            .query(TimelineQuery {
                order,
                limit: 1,
                seek: Some(seek(&ids[2], SeekRelation::Before)),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(before.rows[0].event.id, ids[1]);
        assert!(before.has_previous && before.has_next);
    }
}
