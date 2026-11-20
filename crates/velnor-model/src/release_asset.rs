// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

/// One private release asset a test needs, as `[[release_assets]]` declares it. The generator and
/// `velnor-ci fetch-assets` both build it through [`ReleaseAsset::new`], so a value that one
/// accepts the other accepts, and no value can hold a shell, expression or path escape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub owner: String,
    pub repo: String,
    pub tag: String,
    pub name: String,
    pub sha256: String,
    /// A relative folder of plain names; the asset is stored at `<dest>/<name>`.
    pub dest: String,
}

/// The steps of the release asset phase of a job; every step knows all the assets of the job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseAssetPart {
    /// Restore the cache entry of the asset at this index.
    Restore(usize),
    /// Merge queue only: decide whether the queue entry may get a token.
    Check,
    /// The read-only App token for the repositories of the assets.
    Token(crate::repo::PrivateSources),
    /// Download, verify and revoke, or verify the restored files.
    Fetch,
    /// Save the verified asset at this index.
    Save(usize),
}

/// The field a value was refused for, and the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseAssetError {
    pub field: &'static str,
    pub value: String,
}

fn plain(text: &str, extra: &[u8]) -> bool {
    !text.is_empty()
        && text.len() <= 255
        && !matches!(text, "." | "..")
        && text.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') || extra.contains(&b)
        })
}

impl ReleaseAsset {
    /// # Errors
    ///
    /// Names the first field that is not valid.
    pub fn new(
        repo: &str,
        tag: &str,
        name: &str,
        sha256: &str,
        dest: &str,
    ) -> Result<Self, ReleaseAssetError> {
        let bad = |field: &'static str, value: &str| ReleaseAssetError {
            field,
            value: value.to_owned(),
        };
        let (owner, repository) = repo
            .split_once('/')
            .filter(|(owner, name)| plain(owner, b"") && plain(name, b""))
            .ok_or_else(|| bad("repo", repo))?;
        if !plain(tag, b"+") {
            return Err(bad("tag", tag));
        }
        if !plain(name, b"+") {
            return Err(bad("asset", name));
        }
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(bad("sha256", sha256));
        }
        let folder = dest
            .split('/')
            .all(|part| plain(part, b"") && !part.eq_ignore_ascii_case(".git"));
        if !folder {
            return Err(bad("dest", dest));
        }
        Ok(Self {
            owner: owner.to_owned(),
            repo: repository.to_owned(),
            tag: tag.to_owned(),
            name: name.to_owned(),
            sha256: sha256.to_owned(),
            dest: dest.to_owned(),
        })
    }

    /// The `--asset` argument of `velnor-ci fetch-assets`.
    #[must_use]
    pub fn argument(&self) -> String {
        format!(
            "{}/{},{},{},{},{}",
            self.owner, self.repo, self.tag, self.name, self.sha256, self.dest
        )
    }

    /// # Errors
    ///
    /// When the text is not an [`argument`](Self::argument) of a valid asset.
    pub fn from_argument(text: &str) -> Result<Self, ReleaseAssetError> {
        let parts: Vec<&str> = text.split(',').collect();
        let [repo, tag, name, sha256, dest] = parts[..] else {
            return Err(ReleaseAssetError {
                field: "asset",
                value: text.to_owned(),
            });
        };
        Self::new(repo, tag, name, sha256, dest)
    }
}
