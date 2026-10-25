//! Git-scoped fixture paths and no-follow symlink checks.

use std::error::Error;
use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

pub(super) fn first_fixture_symlink(root: &Path) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let fixtures = root.join("fixtures");
    let metadata = std::fs::symlink_metadata(&fixtures)?;
    if metadata.file_type().is_symlink() {
        return Ok(Some(fixtures));
    }
    if !metadata.is_dir() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidInput, "fixtures is not a directory").into(),
        );
    }

    let output = crate::impl_cli_tmp::git_fixture::command(root)?
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "fixtures",
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!("git ls-files fixtures failed: {}", output.status).into());
    }

    #[cfg(unix)]
    let candidates = parse_nul_paths(&output.stdout);
    #[cfg(not(unix))]
    let candidates = parse_nul_paths(&output.stdout)?;
    for candidate in candidates {
        if let Some(path) = candidate_symlink(root, &candidate)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn candidate_symlink(root: &Path, candidate: &Path) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let mut components = candidate.components();
    let Some(Component::Normal(first)) = components.next() else {
        return Err("git returned a non-relative fixture path".into());
    };
    if first != OsStr::new("fixtures") {
        return Err("git returned a path outside fixtures".into());
    }

    let mut path = root.join(first);
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() {
        return Ok(Some(path));
    }

    let mut components = components.peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err("git returned a non-normal fixture path component".into());
        };
        path.push(name);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if metadata.file_type().is_symlink() {
            return Ok(Some(path));
        }
        if components.peek().is_some() && !metadata.is_dir() {
            return Ok(None);
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn parse_nul_paths(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(path_from_git_bytes)
        .collect()
}

#[cfg(not(unix))]
fn parse_nul_paths(bytes: &[u8]) -> io::Result<Vec<PathBuf>> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(path_from_git_bytes)
        .collect()
}

#[cfg(unix)]
fn path_from_git_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
}

#[cfg(not(unix))]
fn path_from_git_bytes(bytes: &[u8]) -> io::Result<PathBuf> {
    let path = std::str::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(PathBuf::from(path))
}

#[cfg(unix)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use super::{first_fixture_symlink, parse_nul_paths};

    struct TestRepository(PathBuf);

    impl TestRepository {
        fn new() -> Result<Self, Box<dyn Error>> {
            let root = crate::impl_cli_tmp::fresh_tempdir("fixture-tree")?;
            crate::impl_cli_tmp::git_init(&root)?;
            Ok(Self(root))
        }

        fn add(&self, force: bool, paths: &[&str]) -> Result<(), Box<dyn Error>> {
            let mut command = crate::impl_cli_tmp::git_fixture::command(&self.0)?;
            command.arg("add");
            if force {
                command.arg("--force");
            }
            let output = command.arg("--").args(paths).output()?;
            if !output.status.success() {
                return Err(format!("git add failed: {}", output.status).into());
            }
            Ok(())
        }

        fn ignore(&self, relative: &str) -> Result<(), Box<dyn Error>> {
            fs::write(self.0.join(".gitignore"), format!("/{relative}\n"))?;
            self.add(false, &[".gitignore"])
        }

        fn write_file(&self, relative: &str, contents: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().ok_or("fixture path has no parent")?)?;
            fs::write(&path, contents)?;
            Ok(path)
        }

        fn symlink(&self, relative: &str) -> Result<PathBuf, Box<dyn Error>> {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().ok_or("fixture path has no parent")?)?;
            symlink("unopened-target", &path)?;
            Ok(path)
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            crate::impl_cli_tmp::cleanup(&self.0);
        }
    }

    #[test]
    fn tracked_fixture_symlink_is_rejected() -> Result<(), Box<dyn Error>> {
        let repo = TestRepository::new()?;
        let link = repo.symlink("fixtures/tracked-link")?;
        repo.add(false, &["fixtures/tracked-link"])?;
        assert_eq!(first_fixture_symlink(&repo.0)?, Some(link));
        Ok(())
    }

    #[test]
    fn tracked_ignored_fixture_symlink_is_rejected() -> Result<(), Box<dyn Error>> {
        let repo = TestRepository::new()?;
        repo.ignore("fixtures/tracked-ignored-link")?;
        let link = repo.symlink("fixtures/tracked-ignored-link")?;
        repo.add(true, &["fixtures/tracked-ignored-link"])?;
        assert_eq!(first_fixture_symlink(&repo.0)?, Some(link));
        Ok(())
    }

    #[test]
    fn nonignored_untracked_fixture_symlink_is_rejected() -> Result<(), Box<dyn Error>> {
        let repo = TestRepository::new()?;
        let link = repo.symlink("fixtures/untracked-link")?;
        assert_eq!(first_fixture_symlink(&repo.0)?, Some(link));
        Ok(())
    }

    #[test]
    fn ignored_generated_symlink_is_excluded() -> Result<(), Box<dyn Error>> {
        let repo = TestRepository::new()?;
        repo.ignore("fixtures/generated-target")?;
        let link = repo.symlink("fixtures/generated-target")?;
        assert!(std::fs::symlink_metadata(&link)?.file_type().is_symlink());
        assert_eq!(first_fixture_symlink(&repo.0)?, None);
        Ok(())
    }

    #[test]
    fn regular_tracked_and_untracked_files_pass() -> Result<(), Box<dyn Error>> {
        let repo = TestRepository::new()?;
        repo.write_file("fixtures/tracked.txt", b"tracked")?;
        repo.write_file("fixtures/untracked.txt", b"untracked")?;
        repo.add(false, &["fixtures/tracked.txt"])?;
        assert_eq!(first_fixture_symlink(&repo.0)?, None);
        Ok(())
    }

    #[test]
    fn replaced_fixture_ancestor_symlink_is_rejected() -> Result<(), Box<dyn Error>> {
        let repo = TestRepository::new()?;
        repo.write_file("fixtures/nested/entry.txt", b"tracked")?;
        repo.add(false, &["fixtures/nested/entry.txt"])?;

        let ancestor = repo.0.join("fixtures/nested");
        fs::remove_dir_all(&ancestor)?;
        let replacement = repo.0.join("replacement");
        fs::create_dir_all(&replacement)?;
        fs::write(replacement.join("entry.txt"), b"unread")?;
        symlink(&replacement, &ancestor)?;

        assert_eq!(first_fixture_symlink(&repo.0)?, Some(ancestor));
        Ok(())
    }

    #[test]
    fn nul_path_records_preserve_non_utf8_bytes() {
        let paths = parse_nul_paths(b"fixtures/non-utf8-\xff\0fixtures/second\0");
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].as_os_str().as_bytes(), b"fixtures/non-utf8-\xff");
        assert_eq!(paths[1], Path::new("fixtures/second"));
    }
}
