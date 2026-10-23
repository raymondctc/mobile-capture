use super::{BodyDescriptor, BodySide};
use crate::model::Event;
use base64::{engine::general_purpose::STANDARD, Engine};
use flate2::read::{DeflateDecoder, MultiGzDecoder, ZlibDecoder};
use std::io::Read;

const DECODED_BOUND: usize = 32 * 1024 * 1024;

pub(super) fn view(event: &Event, side: BodySide, decoded: bool) -> (BodyDescriptor, Vec<u8>) {
    let value = &event.detail[side.key()];
    let encoding = encoding(event, side);
    let unavailable = if value.is_null() {
        Some("Body was not recorded".to_owned())
    } else if value["unavailable"] == true {
        Some(
            value["reason"]
                .as_str()
                .unwrap_or("Body unavailable in raw evidence")
                .to_owned(),
        )
    } else {
        None
    };
    let raw = if let Some(reason) = unavailable {
        Err(reason)
    } else {
        value["text"]
            .as_str()
            .ok_or_else(|| "Body bytes are missing".to_owned())
            .and_then(|text| {
                STANDARD
                    .decode(text)
                    .map_err(|_| "Body base64 is invalid".to_owned())
            })
    };
    let mut descriptor = BodyDescriptor {
        side,
        available: raw.is_ok(),
        reason: raw.as_ref().err().cloned(),
        bytes: raw.as_ref().map_or(0, |v| v.len() as u64),
        text: raw.as_ref().is_ok_and(|b| is_text(b)),
        byte_basis: if decoded { "content" } else { "raw_content" }.into(),
        content_encoding: encoding.clone(),
        decoded_available: false,
        decoded_reason: None,
    };
    let Ok(raw) = raw else {
        descriptor.decoded_reason = descriptor.reason.clone();
        return (descriptor, vec![]);
    };
    if encoding.is_none() {
        descriptor.decoded_available = raw.len() <= DECODED_BOUND;
        if !descriptor.decoded_available {
            descriptor.decoded_reason = Some("Decoded body exceeds 32 MiB".into());
        }
        if decoded && !descriptor.decoded_available {
            descriptor.available = false;
            descriptor.reason = descriptor.decoded_reason.clone();
            descriptor.bytes = 0;
            descriptor.text = false;
            return (descriptor, vec![]);
        }
        return (descriptor, raw);
    }
    let content = decode(&raw, encoding.as_deref().unwrap());
    descriptor.decoded_available = content.is_ok();
    descriptor.decoded_reason = content.as_ref().err().cloned();
    if decoded {
        descriptor.available = descriptor.decoded_available;
        descriptor.reason = descriptor.decoded_reason.clone();
        descriptor.bytes = content.as_ref().map_or(0, |v| v.len() as u64);
        descriptor.text = content.as_ref().is_ok_and(|b| is_text(b));
        (descriptor, content.unwrap_or_default())
    } else {
        (descriptor, raw)
    }
}

fn encoding(event: &Event, side: BodySide) -> Option<String> {
    let key = match side {
        BodySide::Request => "request_headers",
        BodySide::Response => "response_headers",
    };
    let headers = event.detail[key].as_array()?;
    let values = headers
        .iter()
        .filter(|h| {
            h["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case("content-encoding"))
        })
        .filter_map(|h| h["value"].as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(", "))
}

fn decode(raw: &[u8], encoding: &str) -> Result<Vec<u8>, String> {
    let encodings = encoding.split(',').map(str::trim).collect::<Vec<_>>();
    if encodings.len() > 4 {
        return Err("Content encoding chain exceeds four layers".into());
    }
    let mut bytes = raw.to_vec();
    for encoding in encodings.into_iter().rev() {
        bytes = match encoding.to_ascii_lowercase().as_str() {
            "identity" => bytes,
            "gzip" | "x-gzip" => bounded(MultiGzDecoder::new(bytes.as_slice()))?,
            "deflate" => match bounded(ZlibDecoder::new(bytes.as_slice())) {
                Ok(output) => output,
                Err(error) if error == "Decoded body exceeds 32 MiB" => return Err(error),
                Err(_) => bounded(DeflateDecoder::new(bytes.as_slice()))?,
            },
            other => return Err(format!("Unsupported content encoding: {other}")),
        };
        if bytes.len() > DECODED_BOUND {
            return Err("Decoded body exceeds 32 MiB".into());
        }
    }
    Ok(bytes)
}

fn bounded(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    reader
        .take(DECODED_BOUND as u64 + 1)
        .read_to_end(&mut output)
        .map_err(|e| format!("Invalid compressed body: {e}"))?;
    if output.len() > DECODED_BOUND {
        return Err("Decoded body exceeds 32 MiB".into());
    }
    Ok(output)
}

fn is_text(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|s| {
        !s.chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    })
}
