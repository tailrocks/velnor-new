//! Shared hosted MBX qualification identities and typed environment fields.

use std::collections::BTreeMap;

use super::MbxQualificationPins;
use crate::yaml::Yaml;

pub(super) const WRITER_JOB_ID: &str = "mbx-cache-write-hosted";
pub(super) const READER_JOB_ID: &str = "mbx-cache-read-hosted";
pub(super) const CORRUPT_JOB_ID: &str = "mbx-cache-corrupt-import-hosted";
pub(super) const QUALIFICATION_CACHE_SCOPE: &str = "qualification-mbx-v1/single-bundle-roundtrip";
pub(super) const COMPILE_STEP_NAME: &str = "Compile MBX cache probe";

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
        ("MBX_VERSION".to_owned(), Yaml::str(request.mbx_version.clone())),
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
            Yaml::str("6"),
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
