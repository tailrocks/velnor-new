// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{env, ffi::OsString, fs, path::PathBuf, process::ExitCode};

use sha2::{Digest, Sha256};

#[path = "image_publication_fixture/crane.rs"]
mod crane;
#[path = "image_publication_fixture/registry.rs"]
mod registry;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            if let Some(home) = env::var_os("HOME")
                && let Err(record_error) = fs::write(
                    PathBuf::from(home).join("image-publication-fixture-error"),
                    error.to_string(),
                )
            {
                eprintln!("failed to record fixture error: {record_error}");
            }
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let args = env::args_os().collect::<Vec<_>>();
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or("missing mise payload")?;
    let payload = args.get(separator + 1..).ok_or("missing mise payload")?;
    let program = payload
        .first()
        .and_then(|value| value.to_str())
        .ok_or("missing payload program")?;
    let state_path = PathBuf::from(env::var_os("HOME").ok_or("missing fixture home")?)
        .join("fixture-state.json");
    let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&state_path)?)?;
    let result = match program {
        "gh" => gh(&payload[1..], &state).map(|()| ExitCode::SUCCESS),
        "crane" => crane::run(&payload[1..], &mut state, &state_path),
        other => Err(format!("unsupported fixture payload: {other}").into()),
    };
    match result {
        Ok(code) => Ok(code),
        Err(error) => {
            state["last_fixture_error"] = serde_json::json!(error.to_string());
            fs::write(&state_path, serde_json::to_vec(&state)?)?;
            Err(error)
        }
    }
}

fn gh(args: &[OsString], state: &serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
    let output = if args.first().is_some_and(|arg| arg == "api") {
        let route = args
            .get(1)
            .and_then(|arg| arg.to_str())
            .ok_or("missing API route")?;
        api(route, state)?
    } else if args.first().is_some_and(|arg| arg == "attestation")
        && args.get(1).is_some_and(|arg| arg == "verify")
    {
        attestation(args, state)?
    } else {
        return Err("unsupported gh request".into());
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn api(
    route: &str,
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let path = route.split('?').next().ok_or("invalid route")?;
    if let Some(source_path) = path.strip_prefix("repos/tailrocks/velnor-new/contents/") {
        let requested_source = route.split_once('?').and_then(|(_, query)| {
            query
                .split('&')
                .find_map(|field| field.strip_prefix("ref="))
        });
        if requested_source != state["source_commit"].as_str() {
            return Err("source content requested at unexpected ref".into());
        }
        let content = state["source_files"][source_path]
            .as_str()
            .ok_or("unknown source path")?;
        return Ok(serde_json::json!({"encoding":"base64","content":content}));
    }
    if path == "repos/tailrocks/velnor-new" {
        return Ok(state["repository"].clone());
    }
    if path == "repos/tailrocks/velnor-new/branches/main" {
        return Ok(serde_json::json!({"name":"main","commit":{"sha":state["main_sha"]}}));
    }
    if let Some(pair) = path.strip_prefix("repos/tailrocks/velnor-new/compare/") {
        let (source, main) = pair.split_once("...").ok_or("invalid comparison route")?;
        if source
            != state["source_commit"]
                .as_str()
                .ok_or("missing selected source")?
            || main
                != state["main_sha"]
                    .as_str()
                    .ok_or("missing default branch head")?
        {
            return Err("source comparison did not match fixture authority".into());
        }
        let status = if source == main { "identical" } else { "ahead" };
        return Ok(serde_json::json!({"status":status}));
    }
    api_run_data(route, state)
}

fn attestation(
    args: &[OsString],
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let path = PathBuf::from(args.get(2).ok_or("missing attestation subject path")?);
    let subject = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid attestation subject filename")?
        .to_owned();
    let subject_digest = hex(&Sha256::digest(fs::read(&path)?));
    let subjects = state["attestation"][0]["verificationResult"]["statement"]["subject"]
        .as_array()
        .ok_or("fixture attestation has no subjects")?;
    if !subjects
        .iter()
        .any(|value| value["name"] == subject && value["digest"]["sha256"] == subject_digest)
    {
        return Err("attestation requested for an unattested subject".into());
    }
    Ok(state["attestation"].clone())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn api_run_data(
    route: &str,
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let path = route.split('?').next().ok_or("invalid route")?;
    let run = &state["producer"];
    let run_id = run["id"].as_u64().ok_or("invalid producer run state")?;
    if path.ends_with(&format!("actions/runs/{run_id}/attempts/1")) {
        return Ok(run.clone());
    }
    if path.ends_with(&format!("actions/runs/{run_id}/artifacts")) {
        return Ok(serde_json::json!({"artifacts":[state["artifact"]]}));
    }
    if path.ends_with(&format!("actions/runs/{run_id}/attempts/1/jobs")) {
        return Ok(serde_json::json!({"jobs":state["jobs"]}));
    }
    if path.contains("actions/workflows/ci.yml/runs") {
        let query = route
            .split_once('?')
            .map(|(_, query)| query)
            .map_or("", |query| query);
        let fields = query.split('&').collect::<Vec<_>>();
        let nonces = fields
            .iter()
            .filter_map(|field| field.strip_prefix("_velnor_read_nonce="))
            .collect::<Vec<_>>();
        let authority_query = fields
            .iter()
            .filter(|field| !field.starts_with("_velnor_read_nonce="))
            .copied()
            .collect::<Vec<_>>()
            .join("&");
        let expected = format!(
            "branch=main&event=push&head_sha={}&per_page=100",
            state["source_commit"]
                .as_str()
                .ok_or("missing selected source")?
        );
        if authority_query != expected || nonces.len() != 1 || nonces[0].is_empty() {
            return Err("CI run query did not match selected source".into());
        }
        let runs = state["ci_runs"].as_array().map_or_else(
            || serde_json::json!([state["ci_run"]]),
            |_| state["ci_runs"].clone(),
        );
        return Ok(serde_json::json!({"workflow_runs":runs}));
    }
    Err(format!("unhandled GitHub API route: {path}").into())
}
