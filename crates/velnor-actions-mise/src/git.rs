// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::Path;

use crate::CheckDeadline;
use crate::command::{IsolatedCommand, OUTPUT_CAPTURE_LIMIT_BYTES, ProcessOutput};
use crate::error::MiseError;

const GIT_PROGRAM: &str = "git";

pub const ALLOWED_GIT_VERBS: [&str; 6] = [
    "rev-parse",
    "ls-files",
    "diff",
    "show",
    "config",
    "merge-base",
];

#[must_use]
pub fn is_allowed_git_verb(verb: &str) -> bool {
    ALLOWED_GIT_VERBS.contains(&verb)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRequest {
    verb: String,
    args: Vec<OsString>,
}

impl GitRequest {
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

    #[must_use]
    pub fn rev_parse(args: Vec<OsString>) -> Self {
        Self::allowed("rev-parse", args)
    }

    #[must_use]
    pub fn ls_files(args: Vec<OsString>) -> Self {
        Self::allowed("ls-files", args)
    }

    #[must_use]
    pub fn diff(args: Vec<OsString>) -> Self {
        Self::allowed("diff", args)
    }

    #[must_use]
    pub fn show(args: Vec<OsString>) -> Self {
        Self::allowed("show", args)
    }

    #[must_use]
    pub fn config(args: Vec<OsString>) -> Self {
        Self::allowed("config", args)
    }

    #[must_use]
    pub fn merge_base(args: Vec<OsString>) -> Self {
        Self::allowed("merge-base", args)
    }

    pub fn rev_list_count(source_sha: &str) -> Result<Self, MiseError> {
        if source_sha.len() != 40
            || !source_sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(MiseError::InvalidStepInput {
                field: "git_rev_list_source_sha".to_owned(),
                value: "requires_full_lowercase_sha1".to_owned(),
            });
        }
        Ok(Self {
            verb: "rev-list".to_owned(),
            args: vec![OsString::from("--count"), OsString::from(source_sha)],
        })
    }

    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    #[must_use]
    pub fn argv(&self) -> Vec<OsString> {
        let mut argv = Vec::with_capacity(self.args.len() + 2);
        argv.push(OsString::from(GIT_PROGRAM));
        argv.push(OsString::from(&self.verb));
        argv.extend(self.args.iter().cloned());
        argv
    }

    #[must_use]
    pub fn command(&self) -> IsolatedCommand {
        IsolatedCommand::direct(GIT_PROGRAM, self.git_args())
    }

    #[must_use]
    pub fn command_in(&self, cwd: &Path) -> IsolatedCommand {
        self.command().with_cwd(cwd.to_path_buf())
    }

    pub fn run(&self) -> Result<ProcessOutput, MiseError> {
        self.command().run()
    }

    pub fn run_in(&self, cwd: &Path) -> Result<ProcessOutput, MiseError> {
        self.command_in(cwd).run()
    }

    pub fn run_in_until(
        &self,
        cwd: &Path,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.command_in(cwd)
            .run_until(OUTPUT_CAPTURE_LIMIT_BYTES, deadline)
    }

    pub fn run_text(&self) -> Result<String, MiseError> {
        let output = self.run()?;
        output.require_success(GIT_PROGRAM)?;
        output.stdout_text(GIT_PROGRAM)
    }

    fn git_args(&self) -> Vec<OsString> {
        let mut args = Vec::with_capacity(self.args.len() + 1);
        args.push(OsString::from(&self.verb));
        args.extend(self.args.iter().cloned());
        args
    }

    fn allowed(verb: &str, args: Vec<OsString>) -> Self {
        debug_assert!(is_allowed_git_verb(verb));
        Self {
            verb: verb.to_owned(),
            args,
        }
    }
}
