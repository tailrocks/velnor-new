//! Framing for fixed native config queries, never Git config syntax parsing.
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;

use super::invalid;
use crate::MiseError;

const MAX_CONFIG_ENTRIES: usize = 4096;
type NativeRecords<'a> = Vec<(&'a [u8], &'a [u8])>;

pub(super) fn native_names(bytes: &[u8]) -> Result<Vec<&[u8]>, MiseError> {
    let names = nul_records(bytes)?;
    if names
        .iter()
        .any(|key| key.is_empty() || key.contains(&b'\n'))
    {
        return Err(invalid("native_config_key_unsupported"));
    }
    Ok(names)
}

pub(super) fn named_values(key: &[u8], bytes: &[u8]) -> Result<VecDeque<Vec<u8>>, MiseError> {
    let mut values = VecDeque::new();
    for record in nul_records(bytes)? {
        let Some(rest) = record.strip_prefix(key) else {
            return Err(invalid("native_config_capture_drift"));
        };
        let Some(value) = rest.strip_prefix(b"\n") else {
            return Err(invalid("native_config_valueless_unsupported"));
        };
        values.push_back(value.to_vec());
    }
    if values.is_empty() {
        return Err(invalid("native_config_capture_drift"));
    }
    Ok(values)
}

pub(super) fn ordered_tuples(
    keys: &[&[u8]],
    mut values: BTreeMap<Vec<u8>, VecDeque<Vec<u8>>>,
) -> Result<Vec<u8>, MiseError> {
    let mut result = Vec::new();
    for key in keys {
        let value = values
            .get_mut(*key)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| invalid("native_config_capture_drift"))?;
        result.extend_from_slice(key);
        result.push(b'\n');
        result.extend(value);
        result.push(0);
    }
    if values.values().any(|remaining| !remaining.is_empty()) {
        return Err(invalid("native_config_capture_drift"));
    }
    Ok(result)
}

pub(super) fn native_records(bytes: &[u8]) -> Result<NativeRecords<'_>, MiseError> {
    let mut records = Vec::new();
    for record in nul_records(bytes)? {
        let Some(delimiter) = record.iter().position(|byte| *byte == b'\n') else {
            return Err(invalid("native_config_valueless_unsupported"));
        };
        let (key, rest) = record.split_at(delimiter);
        if key.is_empty() {
            return Err(invalid("native_config_protocol_invalid"));
        }
        records.push((key, &rest[1..]));
    }
    Ok(records)
}

pub(super) fn single_pair_matches(bytes: &[u8], key: &OsString, value: &OsString) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        native_records(bytes)
            .is_ok_and(|records| records.as_slice() == [(key.as_bytes(), value.as_bytes())])
    }
    #[cfg(not(unix))]
    {
        let _ = (bytes, key, value);
        false
    }
}

fn nul_records(bytes: &[u8]) -> Result<Vec<&[u8]>, MiseError> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let Some(payload) = bytes.strip_suffix(&[0]) else {
        return Err(invalid("native_config_protocol_invalid"));
    };
    let records: Vec<_> = payload
        .split(|byte| *byte == 0)
        .take(MAX_CONFIG_ENTRIES + 1)
        .collect();
    if records.len() > MAX_CONFIG_ENTRIES {
        return Err(invalid("native_config_protocol_invalid"));
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::{named_values, native_names};

    #[test]
    fn native_names_refuse_ambiguous_newline_subsections() {
        assert!(native_names(b"filter.foo.clean\nx.var\0").is_err());
    }

    #[test]
    fn known_key_boundary_preserves_empty_and_multiline_values() {
        let values = named_values(b"section.key", b"section.key\n\0section.key\na\nb\0")
            .expect("known native key");
        assert_eq!(values[0], b"");
        assert_eq!(values[1], b"a\nb");
        assert!(named_values(b"section.key", b"section.key\0").is_err());
    }
}

impl super::OwnedConfig {
    pub(in crate::command::git) fn verify_source(
        &self,
        owner: &super::IsolatedCommand,
        cwd: &super::super::private_root::BoundCwd,
        context: Option<&super::RepositoryContext>,
        bounds: &super::Bounds<'_>,
    ) -> Result<(), MiseError> {
        let observed = Self::capture(owner, cwd, context, bounds)?;
        if observed.captured != self.captured {
            return Err(invalid("native_config_source_changed"));
        }
        Ok(())
    }
}

pub(super) fn name_arguments() -> Vec<OsString> {
    vec![
        "--null".into(),
        "--list".into(),
        "--includes".into(),
        "--name-only".into(),
    ]
}
