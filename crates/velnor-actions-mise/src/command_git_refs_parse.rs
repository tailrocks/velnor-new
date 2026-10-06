use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;

use crate::MiseError;

use super::NativeRef;

pub(super) fn parse(bytes: &[u8], width: usize) -> Result<Vec<NativeRef>, MiseError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut records = Vec::new();
        let mut names = BTreeMap::new();
        let mut remaining = bytes;
        while !remaining.is_empty() {
            if records.len() >= super::MAX_REFS {
                return Err(super::invalid("native_ref_count_limit"));
            }
            let name = field(&mut remaining)?;
            let oid = field(&mut remaining)?;
            let target = field(&mut remaining)?;
            if remaining.first() != Some(&b'\n') {
                return Err(super::invalid("native_ref_framing"));
            }
            remaining = &remaining[1..];
            if !admitted_name(name)
                || (!target.is_empty() && !admitted_name(target))
                || oid.len() != width
                || !oid.iter().all(u8::is_ascii_hexdigit)
                || oid.iter().all(|byte| *byte == b'0')
            {
                return Err(super::invalid("native_ref_record_unsupported"));
            }
            if names.insert(name.to_vec(), target.to_vec()).is_some() {
                return Err(super::invalid("native_ref_duplicate"));
            }
            records.push(NativeRef {
                name: OsString::from_vec(name.to_vec()),
                oid: OsString::from_vec(oid.to_vec()),
                symbolic: (!target.is_empty()).then(|| OsString::from_vec(target.to_vec())),
            });
        }
        validate_chains(&names)?;
        Ok(records)
    }
    #[cfg(not(unix))]
    {
        let _ = (bytes, width);
        Err(super::invalid("native_refs_platform_unsupported"))
    }
}

fn field<'a>(remaining: &mut &'a [u8]) -> Result<&'a [u8], MiseError> {
    let end = remaining
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| super::invalid("native_ref_framing"))?;
    let source = *remaining;
    let value = &source[..end];
    *remaining = &source[end + 1..];
    Ok(value)
}

pub(super) fn admitted_name(name: &[u8]) -> bool {
    (name == b"HEAD" || name.starts_with(b"refs/"))
        && !name.starts_with(b"refs/replace/")
        && !name
            .iter()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

pub(super) fn validate_chains(names: &BTreeMap<Vec<u8>, Vec<u8>>) -> Result<(), MiseError> {
    let mut resolved = BTreeSet::<&[u8]>::new();
    for name in names.keys() {
        let mut current = name.as_slice();
        let mut visiting = BTreeSet::new();
        loop {
            if resolved.contains(current) {
                break;
            }
            if !visiting.insert(current) {
                return Err(super::invalid("native_ref_cycle"));
            }
            let target = names
                .get(current)
                .ok_or_else(|| super::invalid("native_ref_unresolved"))?;
            if target.is_empty() {
                break;
            }
            current = target;
        }
        resolved.extend(visiting);
    }
    Ok(())
}
