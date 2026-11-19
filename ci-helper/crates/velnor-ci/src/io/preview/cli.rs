// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::gh::GhAttestor;
use super::install::{install, write_outputs};
use super::net::Transport;
use crate::cli::{PreviewGhArgs, RunAuthorityArgs};
use crate::error::CiError;
use crate::preview::authority::modes::{Inputs, Mode, run};
use crate::preview::json::Res;
use crate::preview::runtime::{self, Host};
use crate::preview::sealed;

fn environment() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

pub(super) fn required<'a>(vars: &'a BTreeMap<String, String>, key: &str) -> Res<&'a str> {
    vars.get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CiError::new(format!("set {key}")))
}

fn decimal(text: &str) -> Res<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    text.parse::<u64>()
        .ok()
        .filter(|n| canonical && *n > 0)
        .ok_or_else(|| CiError::new("source preview runtime verification rejected"))
}

fn gh_path(arg: Option<&PathBuf>, vars: &BTreeMap<String, String>) -> PathBuf {
    arg.cloned()
        .unwrap_or_else(|| PathBuf::from(vars.get("PREVIEW_GH").map_or("", String::as_str)))
}

pub fn verify_runtime(args: &PreviewGhArgs) -> Res<String> {
    let vars = environment();
    let source = required(&vars, "PUBLIC_SOURCE")?;
    let run_id = decimal(required(&vars, "PUBLIC_RUN_ID")?)?;
    let attempt = decimal(required(&vars, "PUBLIC_RUN_ATTEMPT")?)?;
    let temp = PathBuf::from(required(&vars, "RUNNER_TEMP")?);
    let output = PathBuf::from(required(&vars, "GITHUB_OUTPUT")?);
    let net = Transport::new(required(&vars, "GH_TOKEN")?.to_owned());
    let attestor = GhAttestor::pinned(gh_path(args.gh.as_ref(), &vars), temp.clone())?;
    let host = Host::detect()?;
    let verified = runtime::verify_runtime(&net, &attestor, host, source, run_id, attempt)
        .map_err(|e| e.context("source preview runtime verification rejected"))?;
    let deadline = crate::preview::net::Net::deadline(&net);
    let outputs = verified.outputs();
    install(&temp, &verified, deadline, || {
        write_outputs(&output, &outputs)
    })
    .map_err(|e| e.context("source preview runtime verification rejected"))?;
    Ok(format!(
        "verified runtime preview {} (version {}, authority {})\n",
        verified.preview["source_sha"].as_str().unwrap_or(""),
        verified.preview["version"].as_str().unwrap_or(""),
        verified.authority.sha
    ))
}

pub fn verify_mac_proof() -> Res<String> {
    let plan = sealed::plan(&environment())?;
    super::seal::exec(&plan)?;
    Err(CiError::new("source preview executable rejected"))
}

pub fn run_authority(args: &RunAuthorityArgs) -> Res<String> {
    let vars = environment();
    let mode = Mode::parse(&args.mode)?;
    let temp = PathBuf::from(required(&vars, "RUNNER_TEMP")?);
    let net = Transport::new(required(&vars, "GH_TOKEN")?.to_owned());
    let attestor = GhAttestor::pinned(gh_path(args.gh.as_ref(), &vars), temp)?;
    let cwd = std::env::current_dir()
        .map_err(|e| CiError::new(format!("cannot read the working directory: {e}")))?;
    let inputs = Inputs { vars };
    run(mode, &net, &attestor, &inputs, Path::new(&cwd))
        .map_err(|e| e.context("source preview publication failed"))?;
    Ok(String::new())
}
