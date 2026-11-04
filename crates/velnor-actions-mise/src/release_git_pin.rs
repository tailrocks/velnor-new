// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest as _, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const GIT_SHA256: &str = "eb54b535467e0975c1284d4c4d08eea779aeb248a5d34b02e2ee171382e22201";
pub const GIT_HTTP_SHA256: &str =
    "71cdcfbb6f04a67b021304ce5cf234dcea7abbce09daf9972fd8d6c58f51f642";
pub const GIT_PACKAGE_SHA256: &str =
    "c79b7948ef9860c97ec74293a90178676e63368a0711c95c28a55af4edf1adab";
pub const GIT_PACKAGE_URL: &str = "https://snapshot.ubuntu.com/ubuntu/20261103T000000Z/pool/main/g/git/git_2.43.0-1ubuntu7.4_amd64.deb";

pub trait ReleaseGitExecutable {
    fn executable(&self) -> &Path;
    fn exec_path(&self) -> &Path;
    fn revalidate(&self) -> Result<(), &'static str>;
}

#[derive(Debug, Clone)]
pub struct VerifiedGit {
    executable: PathBuf,
    exec_path: PathBuf,
}

impl VerifiedGit {
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    #[must_use]
    pub fn exec_path(&self) -> &Path {
        &self.exec_path
    }
}

impl ReleaseGitExecutable for VerifiedGit {
    fn executable(&self) -> &Path {
        self.executable()
    }
    fn exec_path(&self) -> &Path {
        self.exec_path()
    }
    fn revalidate(&self) -> Result<(), &'static str> {
        verify_bundle(&self.exec_path).map(|_| ())
    }
}

pub fn verify_bundle(root: &Path) -> Result<VerifiedGit, &'static str> {
    verify_with(root, GIT_SHA256, GIT_HTTP_SHA256)?;
    Ok(VerifiedGit {
        executable: root.join("git"),
        exec_path: root.to_owned(),
    })
}

fn verify_with(root: &Path, git_hash: &str, http_hash: &str) -> Result<(), &'static str> {
    let invalid = || "release_git_bundle_invalid";
    if !root.is_absolute() || root.canonicalize().map_err(|_| invalid())? != root || !root.is_dir()
    {
        return Err(invalid());
    }
    let mut entries = fs::read_dir(root)
        .map_err(|_| invalid())?
        .map(|item| item.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid())?;
    entries.sort();
    if entries != ["git", "git-remote-http", "git-remote-https"] {
        return Err(invalid());
    }
    if fs::read_link(root.join("git-remote-https")).map_err(|_| invalid())?
        != Path::new("git-remote-http")
    {
        return Err(invalid());
    }
    for (name, expected) in [("git", git_hash), ("git-remote-http", http_hash)] {
        let path = root.join(name);
        let meta = fs::symlink_metadata(&path).map_err(|_| invalid())?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16 * 1024 * 1024 {
            return Err(invalid());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if meta.permissions().mode() & 0o111 == 0 {
                return Err(invalid());
            }
        }
        let bytes = fs::read(path).map_err(|_| invalid())?;
        let mut hex = String::with_capacity(64);
        for byte in Sha256::digest(&bytes) {
            use std::fmt::Write as _;
            write!(&mut hex, "{byte:02x}").map_err(|_| invalid())?;
        }
        if hex != expected {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::verify_with;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    #[test]
    fn accepts_only_exact_git_and_remote_helper_bundle() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        fs::write(root.join("git"), b"fixture-git")?;
        fs::write(root.join("git-remote-http"), b"fixture-http")?;
        for name in ["git", "git-remote-http"] {
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o500))?;
        }
        symlink("git-remote-http", root.join("git-remote-https"))?;
        let hashes = [
            "25b512eecb97580981907c7a574434c66321f5bbad5effe6522238a0d592ac48",
            "6e2e6e57d48e3bf0e2c3725268d711f31c99e663d216d686daed2cfe182918f1",
        ];
        assert!(verify_with(&root, hashes[0], hashes[1]).is_ok());
        fs::write(root.join("extra"), b"bad")?;
        assert!(verify_with(&root, hashes[0], hashes[1]).is_err());
        fs::remove_file(root.join("extra"))?;
        fs::remove_file(root.join("git-remote-https"))?;
        assert!(verify_with(&root, hashes[0], hashes[1]).is_err());
        symlink("git", root.join("git-remote-https"))?;
        assert!(verify_with(&root, hashes[0], hashes[1]).is_err());
        fs::remove_file(root.join("git-remote-https"))?;
        symlink("git-remote-http", root.join("git-remote-https"))?;
        fs::set_permissions(root.join("git"), fs::Permissions::from_mode(0o400))?;
        assert!(verify_with(&root, hashes[0], hashes[1]).is_err());
        fs::set_permissions(root.join("git"), fs::Permissions::from_mode(0o500))?;
        let alias = temp.path().join("alias");
        symlink(&root, &alias)?;
        assert!(verify_with(&alias, hashes[0], hashes[1]).is_err());
        fs::remove_file(alias)?;
        fs::set_permissions(
            root.join("git-remote-http"),
            fs::Permissions::from_mode(0o700),
        )?;
        fs::write(root.join("git-remote-http"), b"tampered")?;
        assert!(verify_with(&root, hashes[0], hashes[1]).is_err());
        Ok(())
    }
}
