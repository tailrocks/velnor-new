//! Shared hosted MBX qualification identities and typed environment fields.

use super::MbxQualificationPins;
use crate::yaml::Yaml;

pub(super) const WRITER_JOB_ID: &str = "mbx-cache-write-hosted";
pub(super) const READER_JOB_ID: &str = "mbx-cache-read-hosted";
pub(super) const CORRUPT_JOB_ID: &str = "mbx-cache-corrupt-import-hosted";
pub(super) const QUALIFICATION_CACHE_SCOPE: &str = "qualification-mbx-v1/single-bundle-roundtrip";
pub(super) const COMPILE_STEP_NAME: &str = "Compile MBX cache probe";
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QualificationRole {
    Writer,
    Reader,
    CorruptReader,
}

impl QualificationRole {
    pub(super) fn is_writer(self) -> bool {
        self == Self::Writer
    }

    pub(super) fn is_regular_reader(self) -> bool {
        self == Self::Reader
    }
}

pub(super) fn permission_yaml(writer: bool) -> Yaml {
    mapping(&[
        ("contents", "read"),
        ("actions", if writer { "write" } else { "read" }),
    ])
}

pub(super) fn qualification_env(request: &MbxQualificationPins) -> Yaml {
    Yaml::Map(vec![
        ("MBX_GC_AUTO".to_owned(), Yaml::str("1")),
        (
            "MBX_VERSION".to_owned(),
            Yaml::str(request.mbx_version.clone()),
        ),
        (
            "RUSTUP_TOOLCHAIN".to_owned(),
            Yaml::str(request.rust_version.clone()),
        ),
        (
            "MBX_CACHE_SCOPE".to_owned(),
            Yaml::str(QUALIFICATION_CACHE_SCOPE),
        ),
        (
            "MBX_QUALIFICATION_ACTION_REF".to_owned(),
            Yaml::str(request.mbx_action_uses.clone()),
        ),
        (
            "MBX_QUALIFICATION_PHASE_FILE".to_owned(),
            Yaml::str("${{ runner.temp }}/mbx-cache-evidence/phases.tsv"),
        ),
        (
            "MBX_QUALIFICATION_IMPORT_RECEIPT".to_owned(),
            Yaml::str("${{ runner.temp }}/mbx-cache-evidence/import-receipt.txt"),
        ),
        (
            "MBX_QUALIFICATION_EXPORT_RECEIPT".to_owned(),
            Yaml::str("${{ runner.temp }}/mbx-cache-evidence/export-receipt.txt"),
        ),
        (
            "MBX_QUALIFICATION_SAMPLE_INTERVAL".to_owned(),
            Yaml::str("5"),
        ),
        (
            "MBX_QUALIFICATION_FINALIZER_WAIT".to_owned(),
            Yaml::str("15"),
        ),
    ])
}

fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn base64_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = if chunk.len() > 1 { chunk[1] } else { 0 };
        let third = if chunk.len() > 2 { chunk[2] } else { 0 };
        encoded.push(BASE64_ALPHABET[(first >> 2) as usize] as char);
        encoded.push(BASE64_ALPHABET[(((first & 3) << 4) | (second >> 4)) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            BASE64_ALPHABET[(((second & 15) << 2) | (third >> 6)) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(third & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

#[cfg(test)]
pub(super) fn verify_embedded_payload(source: &str, encoded: &str, expected_sha256: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut base64_child = Command::new("base64")
        .arg("--decode")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start base64 decoder");
    base64_child
        .stdin
        .as_mut()
        .expect("base64 stdin")
        .write_all(encoded.as_bytes())
        .expect("write encoded source");
    let decode_output = base64_child
        .wait_with_output()
        .expect("decode embedded source");
    assert!(decode_output.status.success(), "base64 decoder failed");
    assert_eq!(decode_output.stdout, source.as_bytes());

    let mut hasher = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start sha256sum");
    hasher
        .stdin
        .as_mut()
        .expect("sha256sum stdin")
        .write_all(&decode_output.stdout)
        .expect("write decoded source");
    let digest = hasher.wait_with_output().expect("hash embedded source");
    assert!(digest.status.success(), "sha256sum failed");
    let actual = String::from_utf8(digest.stdout).expect("sha256sum output is UTF-8");
    assert_eq!(actual.split_whitespace().next(), Some(expected_sha256));
}

#[cfg(test)]
mod tests {
    use super::base64_encode;

    #[test]
    fn base64_encoding_handles_padding_and_binary_bytes() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(&[0, 255, 128]), "AP+A");
    }
}
