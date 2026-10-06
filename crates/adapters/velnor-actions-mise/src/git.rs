//! Git invocation restricted to the read-only discovery allowlist.
//!
//! Only `rev-parse`, `ls-files`, `diff`, `show`, and `config` may run,
//! always through the fixed wrapper with the isolation environment.
//! Anything else is a typed rejection, never a spawned process.

use std::ffi::OsString;
use std::path::Path;

use crate::CheckDeadline;
use crate::command::{IsolatedCommand, OUTPUT_CAPTURE_LIMIT_BYTES, ProcessOutput};
use crate::error::MiseError;

/// Program name for direct Git invocations.
const GIT_PROGRAM: &str = "git";

/// Verbs permitted for repository discovery and comparison.
///
/// `config` is read-only here: callers query repository identity through
/// `git config --get` in the working tree so linked worktrees, includes,
/// and worktree configuration resolve with Git semantics.
pub const ALLOWED_GIT_VERBS: [&str; 5] = ["rev-parse", "ls-files", "diff", "show", "config"];

/// Whether a verb is inside the discovery allowlist.
#[must_use]
pub fn is_allowed_git_verb(verb: &str) -> bool {
    ALLOWED_GIT_VERBS.contains(&verb)
}

/// One allowlisted Git invocation with byte-exact arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRequest {
    /// Allowlisted verb such as `rev-parse`.
    verb: String,
    /// Verb arguments passed byte-exact.
    args: Vec<OsString>,
}

impl GitRequest {
    /// Build a request for one verb; rejects verbs outside the allowlist.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::GitVerbRejected`] for any verb not in
    /// [`ALLOWED_GIT_VERBS`].
    pub fn new(verb: &str, args: Vec<OsString>) -> Result<Self, MiseError> {
        if !is_allowed_git_verb(verb) {
            return Err(MiseError::GitVerbRejected {
                verb: verb.to_owned(),
            });
        }
        Ok(Self {
            verb: verb.to_owned(),
            args,
        })
    }

    /// `git rev-parse` with byte-exact arguments.
    #[must_use]
    pub fn rev_parse(args: Vec<OsString>) -> Self {
        Self::allowed("rev-parse", args)
    }

    /// `git ls-files` with byte-exact arguments.
    #[must_use]
    pub fn ls_files(args: Vec<OsString>) -> Self {
        Self::allowed("ls-files", args)
    }

    /// `git diff` with byte-exact arguments.
    #[must_use]
    pub fn diff(args: Vec<OsString>) -> Self {
        Self::allowed("diff", args)
    }

    /// `git show` with byte-exact arguments.
    #[must_use]
    pub fn show(args: Vec<OsString>) -> Self {
        Self::allowed("show", args)
    }

    /// `git config` with byte-exact arguments.
    #[must_use]
    pub fn config(args: Vec<OsString>) -> Self {
        Self::allowed("config", args)
    }

    /// Allowlisted verb such as `rev-parse`.
    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// Verb arguments passed byte-exact.
    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Full argument vector: `git`, the verb, then its arguments.
    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let mut argv = Vec::with_capacity(self.args.len() + 2);
        argv.push(OsString::from(GIT_PROGRAM));
        argv.push(OsString::from(&self.verb));
        argv.extend(self.args.iter().cloned());
        argv
    }

    /// Isolated command running this request in the current directory.
    #[must_use]
    pub fn command(&self) -> IsolatedCommand {
        IsolatedCommand::direct(GIT_PROGRAM, self.git_args())
    }

    /// Isolated command running this request in `cwd`.
    #[must_use]
    pub fn command_in(&self, cwd: &Path) -> IsolatedCommand {
        self.command().with_cwd(cwd.to_path_buf())
    }

    /// Run the request and return its typed output, whatever the exit is.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when Git cannot be spawned
    /// or reaped. A nonzero exit is returned as data.
    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        self.command().run()
    }

    /// Run the request in `cwd` and return its typed output.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when Git cannot be spawned
    /// or reaped. A nonzero exit is returned as data.
    pub fn run_in(&self, cwd: &Path) -> Result<ProcessOutput, MiseError> {
        self.command_in(cwd).run()
    }

    /// Run the request in `cwd` under a caller-owned absolute deadline.
    ///
    /// # Errors
    /// Returns [`MiseError::SpawnFailed`] when Git cannot be spawned,
    /// reaped, or finish before the shared deadline.
    pub fn run_in_until(
        &self,
        cwd: &Path,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_in(cwd)
            .run_until(OUTPUT_CAPTURE_LIMIT_BYTES, deadline)
    }

    /// Run the request and return standard output as text on success.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::SpawnFailed`] when Git cannot launch,
    /// [`MiseError::NonZeroExit`] on nonzero status, and
    /// [`MiseError::InvalidUtf8`] when stdout is not text.
    pub fn run_text(&self) -> Result<String, MiseError> {
        let output = self.run()?;
        output.require_success(GIT_PROGRAM)?;
        output.stdout_text(GIT_PROGRAM)
    }

    /// Verb plus arguments as the direct child-process argument list.
    fn git_args(&self) -> Vec<OsString> {
        let mut args = Vec::with_capacity(self.args.len() + 1);
        args.push(OsString::from(&self.verb));
        args.extend(self.args.iter().cloned());
        args
    }

    /// Construct a request for a verb known at compile time to be allowed.
    fn allowed(verb: &str, args: Vec<OsString>) -> Self {
        debug_assert!(is_allowed_git_verb(verb));
        Self {
            verb: verb.to_owned(),
            args,
        }
    }
}

#[cfg(test)]
mod tests;
