//! Sole source asset mapping for the shared archive inventory compiler.
//!
//! Source selection and template digests describe code. They grant neither a
//! metadata projection nor host/runtime qualification or publisher authority.

use velnor_actions_contract::compiled_source_sha256;

/// Closed inventory source programs, with one shared algorithm implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventorySourceProgram {
    /// Canonical projected payload traversal, without original filesystem entry.
    Archive,
    /// Shared traversal plus original filesystem observation entry.
    OriginalFilesystem,
}

/// Fixed ordered module names and complete compiler-owned source bytes.
///
/// Names are Python module stems, without `.py`. Consumers must use this mapping
/// rather than reconstruct an import closure or include Mise files themselves.
#[must_use]
pub fn fixed_sources(program: InventorySourceProgram) -> Vec<(&'static str, &'static str)> {
    let mut sources = vec![
        (
            "source_archive_inventory_common",
            include_str!("source_archive_inventory_common.py"),
        ),
        ("metadata_container", include_str!("metadata_container.py")),
        (
            "source_archive_inventory_fs",
            include_str!("source_archive_inventory_fs.py"),
        ),
        (
            "opaque_inventory_metadata",
            include_str!("opaque_inventory_metadata.py"),
        ),
        (
            "source_archive_inventory_leaf",
            include_str!("source_archive_inventory_leaf.py"),
        ),
        (
            "source_archive_inventory_walk",
            include_str!("source_archive_inventory_walk.py"),
        ),
        (
            "source_archive_inventory",
            include_str!("source_archive_inventory.py"),
        ),
    ];
    if program == InventorySourceProgram::OriginalFilesystem {
        sources.push((
            "source_archive_inventory_original",
            include_str!("source_archive_inventory_original.py"),
        ));
    }
    sources
}

/// Length-framed source identity before embedding any capability literals.
///
/// SHA256 input: framed schema, unsigned BE u64 module count, then each framed
/// name and source bytes in fixed order. A frame is BE u64 length plus bytes.
/// The emitted helper digest stays external; this hash has no self-reference.
#[must_use]
pub fn source_template_sha256(program: InventorySourceProgram) -> String {
    let sources = fixed_sources(program);
    let mut bytes = Vec::new();
    frame(&mut bytes, b"velnor-source-archive-inventory-template-v1");
    bytes.extend_from_slice(&(sources.len() as u64).to_be_bytes());
    for (name, source) in sources {
        frame(&mut bytes, name.as_bytes());
        frame(&mut bytes, source.as_bytes());
    }
    compiled_source_sha256(&bytes)
}

fn frame(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_program_extends_the_single_fixed_archive_mapping() {
        let archive = fixed_sources(InventorySourceProgram::Archive);
        let original = fixed_sources(InventorySourceProgram::OriginalFilesystem);
        assert_eq!(archive.len(), 7);
        assert_eq!(original.len(), 8);
        assert_eq!(&original[..archive.len()], archive.as_slice());
        assert_eq!(original[7].0, "source_archive_inventory_original");
        assert!(archive.iter().all(|(_, source)| !source.is_empty()));
        for (index, (name, _)) in archive.iter().enumerate() {
            assert!(archive[..index].iter().all(|(other, _)| other != name));
        }
    }

    #[test]
    fn template_programs_have_separate_stable_identities() {
        let archive = source_template_sha256(InventorySourceProgram::Archive);
        let original = source_template_sha256(InventorySourceProgram::OriginalFilesystem);
        assert_eq!(archive.len(), 64);
        assert_eq!(original.len(), 64);
        assert_ne!(archive, original);
        assert_eq!(
            archive,
            source_template_sha256(InventorySourceProgram::Archive)
        );
    }
}
