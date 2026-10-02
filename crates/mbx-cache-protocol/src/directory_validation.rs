//! Shared validation for canonical native directory records.
use super::*;
use std::collections::BTreeSet;
use std::path::Component;

impl Directory {
    /// Validate the supported schema and a unique, safe child namespace.
    pub fn validate(&self) -> eyre::Result<()> {
        if self.version != 1 {
            eyre::bail!("unsupported cache directory version");
        }
        let mut names = BTreeSet::new();
        validate_names(self.files.iter().map(|node| node.name.as_str()), &mut names)?;
        validate_names(
            self.directories.iter().map(|node| node.name.as_str()),
            &mut names,
        )?;
        validate_names(
            self.symlinks.iter().map(|node| node.name.as_str()),
            &mut names,
        )?;
        for file in &self.files {
            file.digest.validate()?;
            file.validate_mode()?;
        }
        for directory in &self.directories {
            directory.digest.validate()?;
        }
        for link in &self.symlinks {
            let target = Path::new(&link.target);
            if link.target.is_empty()
                || link.target.contains('\\')
                || link.target.contains('\0')
                || windows_drive_prefix(&link.target)
                || target.is_absolute()
                || target
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
            {
                eyre::bail!("cache directory contains an unsafe symbolic-link target");
            }
        }
        Ok(())
    }

    /// Decode the closed schema and require its canonical owner encoding.
    pub fn from_canonical_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        let directory: Self = serde_json::from_slice(bytes)?;
        directory.validate()?;
        if canonical_json(&directory)? != bytes {
            eyre::bail!("cache directory is not canonical");
        }
        Ok(directory)
    }
}

fn validate_names<'a>(
    entries: impl Iterator<Item = &'a str>,
    names: &mut BTreeSet<&'a str>,
) -> eyre::Result<()> {
    let mut previous = None;
    for name in entries {
        let mut parts = Path::new(name).components();
        if name.is_empty()
            || name.contains('\\')
            || name.contains('/')
            || name.contains('\0')
            || windows_drive_prefix(name)
            || !matches!(parts.next(), Some(Component::Normal(_)))
            || parts.next().is_some()
            || !names.insert(name)
            || previous.is_some_and(|previous| previous >= name)
        {
            eyre::bail!("cache directory contains an unsafe, duplicate, or unsorted child name");
        }
        previous = Some(name);
    }
    Ok(())
}

impl FileNode {
    /// Validate the native recorded permissions, with executability separate.
    pub fn validate_mode(&self) -> eyre::Result<()> {
        if self.mode & !0o644 != 0 {
            eyre::bail!("cache file contains an unsafe mode: {}", self.name);
        }
        Ok(())
    }
}

fn windows_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> Directory {
        Directory {
            version: 1,
            files: vec![FileNode {
                digest: Digest::blake3(b"artifact"),
                executable: false,
                mode: 0o644,
                name: "artifact".into(),
            }],
            directories: vec![],
            symlinks: vec![],
        }
    }

    #[test]
    fn rejects_unsupported_versions_and_child_path_traversal() {
        let directory = directory();
        for name in [
            "",
            ".",
            "..",
            "../outside",
            "/outside",
            "nested/file",
            "nested\\file",
            "C:/outside",
            "C:outside",
            "artifact/",
            "artifact//",
            "artifact\0",
        ] {
            let mut invalid = directory.clone();
            invalid.files[0].name = name.into();
            assert!(invalid.validate().is_err(), "{name}");
        }
        let mut invalid = directory;
        invalid.version = 2;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn rejects_cross_kind_duplicates_unsorted_names_and_foreign_fields() {
        let directory = directory();
        let mut invalid = directory.clone();
        invalid.directories.push(DirectoryNode {
            digest: Digest::blake3(b"directory"),
            mode: 0o755,
            name: "artifact".into(),
        });
        assert!(invalid.validate().is_err());
        let mut invalid = directory.clone();
        let mut first = invalid.files[0].clone();
        first.name = "z-first".into();
        invalid.files.insert(0, first);
        assert!(invalid.validate().is_err());
        let mut value = serde_json::to_value(&directory).unwrap();
        value["foreign"] = serde_json::json!(true);
        assert!(Directory::from_canonical_bytes(&canonical_json(&value).unwrap()).is_err());
    }

    #[test]
    fn accepts_canonical_records_and_rejects_unsafe_link_targets() {
        let directory = directory();
        assert_eq!(
            Directory::from_canonical_bytes(&canonical_json(&directory).unwrap()).unwrap(),
            directory
        );
        for target in [
            "",
            "/outside",
            "../outside",
            "nested/../../outside",
            "nested\\outside",
            "C:/outside",
            "C:outside",
            "artifact\0",
        ] {
            let mut invalid = directory.clone();
            invalid.symlinks.push(SymlinkNode {
                name: "link".into(),
                target: target.into(),
                mode: 0,
            });
            assert!(invalid.validate().is_err());
        }
        let mut bytes = canonical_json(&directory).unwrap();
        bytes.push(b'\n');
        assert!(Directory::from_canonical_bytes(&bytes).is_err());
    }

    #[test]
    fn rejects_file_modes_the_native_materializer_cannot_accept() {
        for mode in [0o755, 0o666, 0o4644, u32::MAX] {
            let mut invalid = directory();
            invalid.files[0].mode = mode;
            assert!(invalid.validate().is_err());
        }
        for mode in [0, 0o200, 0o400, 0o600, 0o644] {
            let mut valid = directory();
            valid.files[0].mode = mode;
            assert!(valid.validate().is_ok());
        }
    }
}
