// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use reqwest::header::CONTENT_TYPE;

use super::js_text;

#[derive(Debug)]
pub(super) struct Fetched {
    pub(super) sha256: String,
    pub(super) raw_name: Option<String>,
}

fn percent_decoded(text: &str) -> Result<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let digits = text
                .get(index + 1..index + 3)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .context("the blob's Content-Disposition file name is malformed")?;
            decoded.push(digits);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).context("the blob's Content-Disposition file name is malformed")
}

fn after_equals(rest: &str) -> Option<&str> {
    js_text::trim_start(rest)
        .strip_prefix('=')
        .map(js_text::trim_start)
}

fn first_match<'a>(
    header: &'a str,
    key: &str,
    capture: impl Fn(&'a str, usize) -> Option<&'a str>,
) -> Option<&'a str> {
    header
        .to_ascii_lowercase()
        .match_indices(key)
        .find_map(|(start, _)| capture(header, start))
}

fn until<'a>(text: &'a str, stops: &[char]) -> &'a str {
    &text[..text.find(stops).unwrap_or(text.len())]
}

fn extended_name(header: &str) -> Option<&str> {
    first_match(header, "filename*", |header, start| {
        let rest = after_equals(&header[start + 9..])?;
        let encoded = rest
            .get(..7)
            .filter(|head| head.eq_ignore_ascii_case("utf-8''"))
            .map(|_| &rest[7..])?;
        Some(until(encoded, &[';', '\r', '\n']))
    })
}

fn plain_name(header: &str) -> Option<&str> {
    first_match(header, "filename", |header, start| {
        if header[..start].ends_with('*') {
            return None;
        }
        let rest = after_equals(&header[start + 8..])?;
        let rest = rest.strip_prefix(['"', '\'']).unwrap_or(rest);
        Some(until(rest, &[';', '\r', '\n', '"', '\'']))
    })
}

fn raw_file_name(header: &str) -> Result<String> {
    let Some(raw) = extended_name(header)
        .filter(|name| !name.is_empty())
        .or_else(|| plain_name(header))
        .filter(|name| !name.is_empty())
    else {
        return Ok("artifact".to_owned());
    };
    let decoded = percent_decoded(js_text::trim(raw))?;
    let name = decoded
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if name.is_empty() || name == "." || name == ".." {
        bail!("the blob's Content-Disposition file name is not usable: {name:?}");
    }
    Ok(name.to_owned())
}

pub(super) fn raw_name(url: &str, headers: &reqwest::header::HeaderMap) -> Result<Option<String>> {
    let header = |name| {
        headers.get(name).map_or_default(|value| {
            value
                .as_bytes()
                .iter()
                .map(|&byte| char::from(byte))
                .collect::<String>()
        })
    };
    let content_type = header(CONTENT_TYPE);
    let mime =
        js_text::trim(content_type.split(';').next().unwrap_or_default()).to_ascii_lowercase();
    let zip_path = url::Url::parse(url)
        .is_ok_and(|parsed| parsed.path().to_ascii_lowercase().ends_with(".zip"));
    let zip = zip_path
        || matches!(
            mime.as_str(),
            "application/zip" | "application/x-zip-compressed" | "application/zip-compressed"
        );
    if zip {
        return Ok(None);
    }
    raw_file_name(&header(reqwest::header::CONTENT_DISPOSITION)).map(Some)
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use reqwest::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderMap, HeaderValue};

    use super::raw_name;

    fn name(content_type: &[u8], disposition: &[u8]) -> Result<Option<String>> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_bytes(content_type)?);
        headers.insert(CONTENT_DISPOSITION, HeaderValue::from_bytes(disposition)?);
        raw_name("http://blob.test/x", &headers)
    }

    #[test]
    fn a_next_line_byte_is_not_whitespace_to_the_toolkit() -> Result<()> {
        let cases: [(&[u8], &[u8], Option<&str>); 5] = [
            (b"application/zip", b"filename=a.txt", None),
            (b"\x85application/zip", b"filename=a.txt", Some("a.txt")),
            (b"text/plain", b"filename=\x85a.txt", Some("\u{85}a.txt")),
            (b"text/plain", b"filename\x85=a.txt", Some("artifact")),
            (b"text/plain", b"filename= \x85a.txt", Some("\u{85}a.txt")),
        ];
        for (content_type, disposition, expected) in cases {
            assert_eq!(
                name(content_type, disposition)?.as_deref(),
                expected,
                "{content_type:?} {disposition:?}"
            );
        }
        Ok(())
    }
}
