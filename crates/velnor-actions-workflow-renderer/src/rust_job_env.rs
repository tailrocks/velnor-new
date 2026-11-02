// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobEnv, Step, StepKind};

use crate::waste_lint::ExecWord;

const SEPARATORS: [char; 10] = [';', '&', '|', '(', ')', '\'', '"', '\\', '<', '>'];

const TOOLCHAIN_WORDS: [ExecWord; 4] = [
    ExecWord::Cargo,
    ExecWord::Rustup,
    ExecWord::Rustc,
    ExecWord::Mbx,
];

pub(crate) fn step_tokens(step: &Step) -> Vec<String> {
    let words = match &step.kind {
        StepKind::Shell { run, .. } => run,
        StepKind::TaskExecution { argv, .. } => argv,
        _ => return Vec::new(),
    };
    words
        .iter()
        .flat_map(|word| word.split(|c: char| c.is_whitespace() || SEPARATORS.contains(&c)))
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn exec_words(tokens: &[String]) -> Vec<ExecWord> {
    tokens
        .iter()
        .filter_map(|token| ExecWord::from_word(token))
        .collect()
}

fn installs_from_lockfile(tokens: &[String]) -> bool {
    tokens.iter().enumerate().any(|(at, token)| {
        let rest = &tokens[at + 1..];
        let flags = rest.iter().take_while(|flag| flag.starts_with("--"));
        token == "mise"
            && flags.clone().any(|flag| flag == "--locked")
            && rest
                .get(flags.count())
                .is_some_and(|word| word == "install")
    })
}

fn runs_toolchain(step: &Step) -> bool {
    let tokens = step_tokens(step);
    installs_from_lockfile(&tokens)
        || exec_words(&tokens)
            .iter()
            .any(|word| TOOLCHAIN_WORDS.contains(word))
}

pub(crate) fn installs_rust(job: &Job) -> bool {
    crate::cache_p08::infer_job_tools(job)
        .iter()
        .any(|spec| velnor_actions_mise::rust_spec_version(spec).is_some())
}

pub(crate) fn is_rust_bearing(job: &Job) -> bool {
    installs_rust(job) || job.steps.iter().any(runs_toolchain)
}

pub(crate) fn bind(jobs: &mut BTreeMap<String, Job>) {
    for job in jobs.values_mut() {
        if is_rust_bearing(job) {
            job.env = JobEnv::RustToolchain;
        }
    }
}

pub(crate) fn extend_env(env: &mut BTreeMap<String, String>, job_env: JobEnv) {
    env.extend(
        job_env
            .pairs()
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned())),
    );
}
