//! Bounded HTTP and local-file transport for scheduled freshness probes.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use flate2::read::MultiGzDecoder;

use crate::context::FETCH_CAP;

pub(in crate::probe) mod range;

pub(super) use range::fetch_prefix_text;

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const MAX_REDIRECTS: usize = 5;
static UPSTREAM_AGENT: OnceLock<Result<ureq::Agent, String>> = OnceLock::new();

fn read_limit() -> u64 {
    u64::try_from(FETCH_CAP)
        .unwrap_or(u64::MAX)
        .saturating_add(1)
}

pub(super) fn fetch_text(url: &str) -> Result<String, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return fetch_file(path);
    }
    fetch_http_with_agent(upstream_agent()?, url, FETCH_TIMEOUT)
}

pub(super) fn fetch_http_with_agent(
    agent: &ureq::Agent,
    url: &str,
    timeout: Duration,
) -> Result<String, String> {
    fetch_http_with_agent_mode(agent, url, timeout, None)
}

fn fetch_http_with_agent_mode(
    agent: &ureq::Agent,
    url: &str,
    timeout: Duration,
    prefix_len: Option<usize>,
) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut current = url.to_owned();
    for redirects in 0..=MAX_REDIRECTS {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("upstream request exceeded its overall deadline".to_owned());
        }
        let mut request = agent
            .get(&current)
            .header("User-Agent", "velnor-freshness-probe")
            .header("Accept", "application/json");
        if let Some(prefix_len) = prefix_len {
            request = request
                .header("Accept-Encoding", "identity")
                .header("Range", format!("bytes=0-{}", prefix_len - 1));
        } else {
            request = request.header("Accept-Encoding", "gzip");
        }
        let mut response = request
            .config()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .build()
            .call()
            .map_err(|error| error.to_string())?;
        let status = response.status();
        let location = if status.is_redirection() {
            response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        } else {
            None
        };
        if !status.is_redirection()
            && let Some(prefix_len) = prefix_len
        {
            range::validate_prefix_response(&response, prefix_len)?;
        }
        let body = response_text(&mut response)?;
        if Instant::now() >= deadline {
            return Err("upstream request exceeded its overall deadline".to_owned());
        }
        if !status.is_redirection() {
            if prefix_len.is_some_and(|expected| body.len() != expected) {
                return Err(format!(
                    "range response body length was {}, expected {} bytes",
                    body.len(),
                    prefix_len.unwrap_or_default()
                ));
            }
            return Ok(body);
        }
        if redirects == MAX_REDIRECTS {
            return Err("upstream request exceeded the redirect limit".to_owned());
        }
        let location = location.ok_or_else(|| "redirect response has no Location".to_owned())?;
        current = resolve_redirect(&current, &location)?;
    }
    Err("upstream request exceeded the redirect limit".to_owned())
}

fn response_text(response: &mut ureq::http::Response<ureq::Body>) -> Result<String, String> {
    let mut encodings = response
        .headers()
        .get_all("content-encoding")
        .iter()
        .filter_map(|value| value.to_str().ok());
    let first = encodings.next();
    let encoding = match first {
        None => "identity".to_owned(),
        Some(value) if encodings.next().is_none() => value.trim().to_ascii_lowercase(),
        _ => {
            return Err("unsupported Content-Encoding: duplicate coding headers".to_owned());
        }
    };
    let mut encoded = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(read_limit())
        .read_to_end(&mut encoded)
        .map_err(|error| format!("response read failed ({error})"))?;
    if encoded.len() > FETCH_CAP {
        return Err(format!("encoded response exceeds {FETCH_CAP} bytes"));
    }
    let decoded = decode_body(&encoding, &encoded)?;
    Ok(String::from_utf8_lossy(&decoded).into_owned())
}

pub(super) fn resolve_redirect(current: &str, location: &str) -> Result<String, String> {
    let location = location.trim().split('#').next().unwrap_or_default();
    if location.is_empty() {
        return Ok(current.to_owned());
    }
    let base = http_uri(current)?;
    let scheme = base
        .scheme_str()
        .ok_or_else(|| "redirect base has no scheme".to_owned())?;
    if location.starts_with("//") {
        return Ok(http_uri(&format!("{scheme}:{location}"))?.to_string());
    }
    if location.split_once(':').is_some_and(|(candidate, _)| {
        !candidate.is_empty()
            && candidate
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
    }) {
        return Ok(http_uri(location)?.to_string());
    }
    let authority = base
        .authority()
        .ok_or_else(|| "redirect base has no authority".to_owned())?;
    let base_path = base
        .path_and_query()
        .map_or("/", ureq::http::uri::PathAndQuery::path);
    let (path, query) = if location.starts_with('?') {
        (base_path.to_owned(), location.to_owned())
    } else {
        let (relative_path, query) = location
            .split_once('?')
            .map_or((location, ""), |(path, query)| (path, query));
        let path = if relative_path.starts_with('/') {
            relative_path.to_owned()
        } else {
            let directory = base_path
                .rsplit_once('/')
                .map_or("/", |(directory, _)| directory);
            format!("{directory}/{relative_path}")
        };
        (path, query.to_owned())
    };
    let path = normalize_redirect_path(&path);
    let suffix = if query.is_empty() {
        String::new()
    } else if query.starts_with('?') {
        query.clone()
    } else {
        format!("?{query}")
    };
    Ok(http_uri(&format!("{scheme}://{authority}{path}{suffix}"))?.to_string())
}

fn http_uri(uri: &str) -> Result<ureq::http::Uri, String> {
    let parsed = uri
        .parse::<ureq::http::Uri>()
        .map_err(|error| format!("invalid redirect URI ({error})"))?;
    if !matches!(parsed.scheme_str(), Some("http" | "https")) || parsed.authority().is_none() {
        return Err("redirect URI must use HTTP or HTTPS".to_owned());
    }
    Ok(parsed)
}

fn normalize_redirect_path(path: &str) -> String {
    let mut input = path.to_owned();
    let mut output = String::new();
    while !input.is_empty() {
        if input.starts_with("../") {
            input.drain(..3);
        } else if input.starts_with("./") || input.starts_with("/./") {
            input.drain(..2);
        } else if input == "/." {
            "/".clone_into(&mut input);
        } else if input.starts_with("/../") {
            input.drain(..3);
            remove_last_path_segment(&mut output);
        } else if input == "/.." {
            "/".clone_into(&mut input);
            remove_last_path_segment(&mut output);
        } else if input == "." || input == ".." {
            input.clear();
        } else {
            let segment_end = if let Some(after_root) = input.strip_prefix('/') {
                after_root
                    .find('/')
                    .map_or(input.len(), |offset| offset + 1)
            } else {
                input.find('/').unwrap_or(input.len())
            };
            output.push_str(&input[..segment_end]);
            input.drain(..segment_end);
        }
    }
    output
}

fn remove_last_path_segment(output: &mut String) {
    if let Some(segment_start) = output.rfind('/') {
        output.truncate(segment_start);
    } else {
        output.clear();
    }
}

fn upstream_agent() -> Result<&'static ureq::Agent, String> {
    UPSTREAM_AGENT
        .get_or_init(build_upstream_agent)
        .as_ref()
        .map_err(Clone::clone)
}

fn build_upstream_agent() -> Result<ureq::Agent, String> {
    let trust = rustls_native_certs::load_native_certs();
    if !trust.errors.is_empty() {
        return Err("platform trust certificates could not all be loaded".to_owned());
    }
    if trust.certs.is_empty() {
        return Err("no platform trust certificates were loaded".to_owned());
    }
    let roots = trust
        .certs
        .iter()
        .map(|certificate| ureq::tls::Certificate::from_der(certificate.as_ref()).to_owned())
        .collect::<Vec<_>>();
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(roots.into())
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(FETCH_TIMEOUT))
        .max_redirects(0)
        .build();
    Ok(ureq::Agent::new_with_config(config))
}

fn fetch_file(path: &str) -> Result<String, String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err("file source path must be absolute".to_owned());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("file source metadata failed ({error})"))?;
    if metadata.file_type().is_symlink() {
        return Err("file source symlinks are not allowed".to_owned());
    }
    if !metadata.file_type().is_file() {
        return Err("file source must be a regular file".to_owned());
    }
    if metadata.len() > u64::try_from(FETCH_CAP).unwrap_or(u64::MAX) {
        return Err(format!("file freshness source exceeds {FETCH_CAP} bytes"));
    }
    let fd = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::LOOP {
            "file source symlinks are not allowed".to_owned()
        } else {
            format!(
                "file source could not open ({})",
                std::io::Error::from(error)
            )
        }
    })?;
    let file_type = rustix::fs::fstat(&fd)
        .map(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode))
        .map_err(|error| {
            format!(
                "file source metadata failed ({})",
                std::io::Error::from(error)
            )
        })?;
    if file_type != rustix::fs::FileType::RegularFile {
        return Err("file source must be a regular file".to_owned());
    }
    let file = File::from(fd);
    let mut bytes = Vec::new();
    file.take(read_limit())
        .read_to_end(&mut bytes)
        .map_err(|error| format!("file source read failed ({error})"))?;
    if bytes.len() > FETCH_CAP {
        return Err(format!("file freshness source exceeds {FETCH_CAP} bytes"));
    }
    String::from_utf8(bytes)
        .map_err(|error| format!("file freshness source is not UTF-8 ({error})"))
}

pub(super) fn decode_body(encoding: &str, encoded: &[u8]) -> Result<Vec<u8>, String> {
    let decoded = match encoding {
        "" | "identity" => encoded.to_vec(),
        "gzip" => {
            let gzip_reader = MultiGzDecoder::new(encoded);
            let mut decoded_body = Vec::new();
            gzip_reader
                .take(read_limit())
                .read_to_end(&mut decoded_body)
                .map_err(|error| format!("invalid gzip freshness response ({error})"))?;
            decoded_body
        }
        _ => return Err(format!("unsupported Content-Encoding: {encoding}")),
    };
    if decoded.len() > FETCH_CAP {
        return Err(format!("decompressed response exceeds {FETCH_CAP} bytes"));
    }
    Ok(decoded)
}
