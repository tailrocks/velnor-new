//! Materialize the single authenticated Homebrew source into a fresh directory.

use std::{
    ffi::OsString,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::command::EnvPolicy;
use crate::{CheckDeadline, IsolatedCommand, MiseError, ProcessOutput};

const REPOSITORY: &str = "https://github.com/Homebrew/brew";
const SOURCE_COMMIT: &str = "7a056ffd71e505f725b092d162fb09164a62b510";
const TAG_COMMIT: &str = "8f10429b56be28a25032b87463fffc95e556b5bc";
const SOURCE_TREE: &str = "e8a495cf3247a4af10093b5460e074d27d47999f";
const SOURCE_DESCRIBE: &str = "7.0.10-84-g7a056ff";
const SOURCE_DISTANCE: &str = "84";
const SOURCE_DESCRIBE_ARGS: [&str; 7] = [
    "describe",
    "--tags",
    "--match",
    "7.0.10",
    "--long",
    "--abbrev=7",
    "HEAD",
];
const COMMAND_LIMIT: Duration = Duration::from_secs(120);
const OUTPUT_LIMIT: usize = 128 * 1024;
const GIT: &str = "/usr/bin/git";

/// Identity of the only supported fresh Homebrew source tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedHomebrewSource {
    /// Exact detached commit.
    commit: String,
    /// Exact checked-out Git tree.
    tree: String,
    /// Version distance from tag `7.0.10`.
    describe: String,
    /// Exact commit to which tag `7.0.10` peels.
    tag_commit: String,
    /// Fresh staging directory containing the complete checked-out repository.
    staging: PathBuf,
    /// Staging directory device and inode, retained for the prefix swap proof.
    staging_device: u64,
    /// Staging directory inode, retained for the prefix swap proof.
    staging_inode: u64,
}

impl PinnedHomebrewSource {
    /// Exact detached commit.
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.commit
    }
    /// Exact checked-out Git tree.
    #[must_use]
    pub fn tree(&self) -> &str {
        &self.tree
    }
    /// Exact `git describe` result.
    #[must_use]
    pub fn describe(&self) -> &str {
        &self.describe
    }
    /// Commit to which the pinned `7.0.10` tag peels.
    #[must_use]
    pub fn tag_commit(&self) -> &str {
        &self.tag_commit
    }
    /// Materialized, verified staging path.
    #[must_use]
    pub fn staging(&self) -> &Path {
        &self.staging
    }
}

/// Materialize the fixed Homebrew source into one empty, private staging directory.
///
/// The caller supplies only a newly created child of `RUNNER_TEMP`. This API
/// has no URL, revision, tag, refspec, Git flags, or executable parameters.
/// Git runs with a cleared environment and an absolute system executable.
///
/// # Errors
/// Returns an error if the staging path is not fresh and private, Git cannot
/// fetch the pinned source, or any checked source identity differs.
pub fn materialize_homebrew_pinned_source(
    staging: &Path,
    deadline: CheckDeadline,
) -> Result<PinnedHomebrewSource, MiseError> {
    let stage_metadata = validate_source_staging(staging)?;
    let tag_commit = fetch_pinned_history(staging, deadline)?;
    checkout_pinned_commit(staging, deadline)?;
    verify_materialized_source(staging, &stage_metadata, tag_commit, deadline)
}

fn validate_source_staging(staging: &Path) -> Result<fs::Metadata, MiseError> {
    validate_empty_staging(staging)?;
    validate_trusted_system_file(Path::new(GIT))?;
    let prefix =
        fs::symlink_metadata("/opt/homebrew").map_err(|_| invalid("homebrew_source_prefix"))?;
    if !prefix.is_dir()
        || prefix.file_type().is_symlink()
        || prefix.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(invalid("homebrew_source_prefix"));
    }
    let staging_metadata =
        fs::symlink_metadata(staging).map_err(|_| invalid("homebrew_source_staging"))?;
    if staging_metadata.dev() != prefix.dev() {
        return Err(invalid("homebrew_source_device"));
    }
    Ok(staging_metadata)
}

fn fetch_pinned_history(staging: &Path, deadline: CheckDeadline) -> Result<String, MiseError> {
    run(
        staging,
        [
            "-c",
            "credential.helper=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "protocol.file.allow=never",
            "-c",
            "submodule.recurse=false",
            "-c",
            "init.templateDir=/dev/null",
            "init",
            "--quiet",
        ],
        deadline,
    )?;
    run(
        staging,
        [
            "fetch",
            "--no-tags",
            "--no-recurse-submodules",
            "--depth=512",
            REPOSITORY,
            SOURCE_COMMIT,
            "refs/tags/7.0.10:refs/tags/7.0.10",
        ],
        deadline,
    )?;
    let tag_commit = value(
        staging,
        ["rev-parse", "--verify", "refs/tags/7.0.10^{}"],
        deadline,
    )?;
    if tag_commit != TAG_COMMIT {
        return Err(invalid("homebrew_source_tag_commit"));
    }
    Ok(tag_commit)
}

fn checkout_pinned_commit(staging: &Path, deadline: CheckDeadline) -> Result<(), MiseError> {
    run(
        staging,
        ["checkout", "--quiet", "--detach", SOURCE_COMMIT],
        deadline,
    )?;
    Ok(())
}

fn verify_materialized_source(
    staging: &Path,
    stage_metadata: &fs::Metadata,
    tag_commit: String,
    deadline: CheckDeadline,
) -> Result<PinnedHomebrewSource, MiseError> {
    let commit = value(
        staging,
        ["rev-parse", "--verify", "HEAD^{commit}"],
        deadline,
    )?;
    let tree = value(staging, ["rev-parse", "--verify", "HEAD^{tree}"], deadline)?;
    let describe = value(staging, SOURCE_DESCRIBE_ARGS, deadline)?;
    let distance = value(
        staging,
        [
            "rev-list",
            "--count",
            "7.0.10..7a056ffd71e505f725b092d162fb09164a62b510",
        ],
        deadline,
    )?;
    run(
        staging,
        ["merge-base", "--is-ancestor", "7.0.10", SOURCE_COMMIT],
        deadline,
    )?;
    let final_status = run(
        staging,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        deadline,
    )?;
    if !final_status.stdout.is_empty()
        || commit != SOURCE_COMMIT
        || tree != SOURCE_TREE
        || describe != SOURCE_DESCRIBE
        || distance != SOURCE_DISTANCE
    {
        return Err(invalid("homebrew_source_identity"));
    }
    let final_metadata =
        fs::symlink_metadata(staging).map_err(|_| invalid("homebrew_source_staging"))?;
    if final_metadata.dev() != stage_metadata.dev()
        || final_metadata.ino() != stage_metadata.ino()
        || final_metadata.uid() != stage_metadata.uid()
    {
        return Err(invalid("homebrew_source_staging_changed"));
    }
    Ok(PinnedHomebrewSource {
        commit,
        tree,
        describe,
        tag_commit,
        staging: staging.to_path_buf(),
        staging_device: final_metadata.dev(),
        staging_inode: final_metadata.ino(),
    })
}

fn validate_empty_staging(staging: &Path) -> Result<(), MiseError> {
    let runner_temp = std::env::var_os("RUNNER_TEMP")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("homebrew_source_runner_temp"))?;
    if !runner_temp.is_absolute()
        || staging.parent() != Some(runner_temp.as_path())
        || !staging
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("velnor-homebrew-stage-"))
    {
        return Err(invalid("homebrew_source_staging_path"));
    }
    let runner_metadata =
        fs::symlink_metadata(&runner_temp).map_err(|_| invalid("homebrew_source_runner_temp"))?;
    let stage_metadata =
        fs::symlink_metadata(staging).map_err(|_| invalid("homebrew_source_staging"))?;
    if !runner_metadata.is_dir()
        || runner_metadata.file_type().is_symlink()
        || runner_metadata.uid() != rustix::process::geteuid().as_raw()
        || !stage_metadata.is_dir()
        || stage_metadata.file_type().is_symlink()
        || stage_metadata.uid() != runner_metadata.uid()
        || stage_metadata.mode() & 0o077 != 0
        || fs::read_dir(staging)
            .map_err(|_| invalid("homebrew_source_staging"))?
            .next()
            .is_some()
    {
        return Err(invalid("homebrew_source_staging"));
    }
    Ok(())
}

fn validate_trusted_system_file(path: &Path) -> Result<(), MiseError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid("homebrew_source_git"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.mode() & 0o111 == 0
    {
        return Err(invalid("homebrew_source_git"));
    }
    Ok(())
}

fn value<const N: usize>(
    cwd: &Path,
    words: [&str; N],
    deadline: CheckDeadline,
) -> Result<String, MiseError> {
    Ok(run(cwd, words, deadline)?
        .stdout_text("git")?
        .trim()
        .to_owned())
}

fn run<const N: usize>(
    cwd: &Path,
    words: [&str; N],
    deadline: CheckDeadline,
) -> Result<ProcessOutput, MiseError> {
    let command_deadline = CheckDeadline::from_start(Instant::now(), COMMAND_LIMIT)?;
    let mut git_args = vec![
        OsString::from("-c"),
        OsString::from("credential.helper="),
        OsString::from("-c"),
        OsString::from("core.hooksPath=/dev/null"),
        OsString::from("-c"),
        OsString::from("protocol.file.allow=never"),
        OsString::from("-c"),
        OsString::from("submodule.recurse=false"),
        OsString::from("-c"),
        OsString::from("core.fsmonitor=false"),
    ];
    git_args.extend(words.into_iter().map(OsString::from));
    let env = [
        ("HOME", cwd.as_os_str().to_owned()),
        ("PATH", OsString::from("/usr/bin:/bin:/usr/sbin:/sbin")),
        ("TMPDIR", std::env::temp_dir().into_os_string()),
        ("GIT_CONFIG_NOSYSTEM", OsString::from("1")),
        ("GIT_CONFIG_SYSTEM", OsString::from("/dev/null")),
        ("GIT_CONFIG_GLOBAL", OsString::from("/dev/null")),
        ("GIT_TERMINAL_PROMPT", OsString::from("0")),
        ("GIT_ALLOW_PROTOCOL", OsString::from("https")),
        ("GIT_OPTIONAL_LOCKS", OsString::from("0")),
    ]
    .map(|(key, value)| (OsString::from(key), value));
    let command = IsolatedCommand::repo_task(GIT, git_args, &[])?
        .with_env(&env)?
        .with_policy(EnvPolicy::QualifiedProbe)
        .with_cwd(cwd.to_path_buf());
    let output = deadline
        .scope(|| {
            command_deadline.scope(|| command.run_quiescent_until(OUTPUT_LIMIT, command_deadline))
        })
        .map_err(|_| invalid("homebrew_source_deadline"))???;
    let (output, _drained) = output;
    output.require_success("git")?;
    deadline.remaining()?;
    Ok(output)
}

fn invalid(problem: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "homebrew_pinned_source".to_owned(),
        value: problem.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_profile_is_a_closed_tag_commit_tree_and_distance_tuple() {
        assert_eq!(SOURCE_COMMIT.len(), 40);
        assert_eq!(TAG_COMMIT.len(), 40);
        assert_eq!(SOURCE_TREE.len(), 40);
        assert_eq!(SOURCE_DESCRIBE, "7.0.10-84-g7a056ff");
        assert!(SOURCE_DESCRIBE_ARGS.contains(&"--abbrev=7"));
        let (_, hash) = SOURCE_DESCRIBE
            .rsplit_once("-g")
            .expect("pinned describe result has a git hash suffix");
        assert_eq!(
            hash.len(),
            7,
            "Homebrew source policy uses seven hash chars"
        );
        assert_eq!(SOURCE_DISTANCE, "84");
        assert_ne!(SOURCE_COMMIT, TAG_COMMIT);
        assert_eq!(GIT, "/usr/bin/git");
    }

    #[test]
    fn materialization_rejects_any_caller_path_outside_its_fixed_staging_slot() {
        let deadline = CheckDeadline::after(Duration::from_secs(10)).expect("deadline");
        assert!(materialize_homebrew_pinned_source(Path::new("relative"), deadline).is_err());
        assert!(materialize_homebrew_pinned_source(Path::new("/tmp/homebrew"), deadline).is_err());
    }
}
