// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{env, ffi::OsString, fs, path::PathBuf, process::ExitCode};

#[path = "image_publication_fixture/crane.rs"]
mod crane;
#[path = "image_publication_fixture/digest.rs"]
mod digest;
#[path = "image_publication_fixture/pagination.rs"]
mod pagination;
#[path = "image_publication_fixture/registry.rs"]
mod registry;
#[path = "image_publication_fixture/runs.rs"]
mod runs;
#[path = "image_publication_fixture/snapshot.rs"]
mod snapshot;
use digest::hex;

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
        "github-api" if payload.get(1).is_some_and(|argument| argument == "api") => {
            github_api(&payload[1..], &state).map(|()| ExitCode::SUCCESS)
        }
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

fn github_api(
    args: &[OsString],
    state: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(output) = gh_api(args, state)? {
        println!("{}", serde_json::to_string(&output)?);
    }
    Ok(())
}

fn gh_api(
    args: &[OsString],
    state: &serde_json::Value,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error>> {
    let mut route_index = 1;
    let mut paginate = false;
    let mut slurp = false;
    while let Some(argument) = args.get(route_index) {
        if argument == "--paginate" {
            paginate = true;
        } else if argument == "--slurp" {
            slurp = true;
        } else if argument.to_string_lossy().starts_with('-') {
            route_index += 1;
            continue;
        } else {
            break;
        }
        route_index += 1;
    }
    let route = args
        .get(route_index)
        .and_then(|arg| arg.to_str())
        .ok_or("missing API route")?;
    if route.starts_with("repos/tailrocks/velnor-new/tarball/") {
        snapshot::write_tar(route, state)?;
        return Ok(None);
    }
    if paginate
        && slurp
        && route
            .split('?')
            .next()
            .is_some_and(|path| path.starts_with("repos/tailrocks/velnor-new/attestations/sha256:"))
    {
        attestation_pages(route, state).map(Some)
    } else {
        api(route, state).map(Some)
    }
}

fn api(
    route: &str,
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let path = route.split('?').next().ok_or("invalid route")?;
    if path.starts_with("repos/tailrocks/velnor-new/git/commits/")
        || path.starts_with("repos/tailrocks/velnor-new/git/trees/")
    {
        return snapshot::tree_api(path, state);
    }
    if let Some(tag) = path.strip_prefix("repos/tailrocks/velnor-new/git/ref/tags/") {
        let promotion = &state["promotion"];
        return Ok(
            serde_json::json!({"ref":format!("refs/tags/{tag}"),"object":{"type":promotion["reference_object_type"],"sha":promotion["tag_object_sha"]}}),
        );
    }
    if let Some(object_sha) = path.strip_prefix("repos/tailrocks/velnor-new/git/tags/") {
        let promotion = &state["promotion"];
        if promotion["tag_object_sha"] != object_sha {
            return Err("unknown annotated tag object".into());
        }
        return Ok(
            serde_json::json!({"sha":promotion["tag_object_sha"],"tag":promotion["tag"],"object":{"type":"commit","sha":promotion["annotation_target"]}}),
        );
    }
    if let Some(source_path) = path.strip_prefix("repos/tailrocks/velnor-new/contents/") {
        let requested_source = route.split_once('?').and_then(|(_, query)| {
            query
                .split('&')
                .find_map(|field| field.strip_prefix("ref="))
        });
        if requested_source != state["source_commit"].as_str() {
            return Err("source content requested at unexpected ref".into());
        }
        let source_path = source_path.replace("%2B", "+").replace("%40", "@");
        let content = state["source_files"][source_path]
            .as_str()
            .ok_or("unknown source path")?;
        return Ok(serde_json::json!({"encoding":"base64","content":content}));
    }
    if path == "repos/tailrocks/velnor-new" {
        return Ok(state["repository"].clone());
    }
    if path == "repos/tailrocks/velnor-new/branches/main" {
        let mut branch = serde_json::json!({"name":"main","protected":state.get("main_protected").unwrap_or(&serde_json::Value::Bool(true)),"commit":{"sha":state["main_sha"]}});
        if state["main_protected_missing"] == true {
            branch
                .as_object_mut()
                .ok_or("branch object missing")?
                .remove("protected");
        }
        return Ok(branch);
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
    runs::api_run_data(route, state)
}

fn attestation_pages(
    route: &str,
    state: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let route_path = route
        .split('?')
        .next()
        .ok_or("attestation route is missing")?;
    let digest = route_path
        .split_once("sha256:")
        .map(|(_, digest)| digest)
        .ok_or("attestation digest is missing")?;
    if let Some(pages) = state.get("signed_attestation_pages") {
        return Ok(pages.clone());
    }
    let qualification = state.get("qualification_attestation").filter(|report| {
        report
            .as_array()
            .and_then(|entries| entries.first())
            .is_some_and(|entry| report_contains_digest(entry, digest))
    });
    if qualification.is_none()
        && let Some(pages) = state.get("attestation_pages")
    {
        return Ok(pages.clone());
    }
    let response = qualification
        .or_else(|| {
            state
                .get("attestation")
                .filter(|report| report_contains_digest(report, digest))
        })
        .ok_or("unattested subject")?;
    let outputs = response
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![response.clone()]);
    let attestations = outputs
        .into_iter()
        .map(|output| serde_json::json!({"bundle":{"fixtureOutput":[output]}}))
        .collect::<Vec<_>>();
    Ok(serde_json::json!([{"attestations":attestations}]))
}

fn report_contains_digest(report: &serde_json::Value, digest: &str) -> bool {
    let entry_contains_digest = |entry: &serde_json::Value| {
        entry["verificationResult"]["statement"]["subject"]
            .as_array()
            .is_some_and(|subjects| {
                subjects
                    .iter()
                    .any(|subject| subject["digest"]["sha256"] == digest)
            })
    };
    match report {
        serde_json::Value::Array(entries) => entries.iter().any(entry_contains_digest),
        serde_json::Value::Object(_) => entry_contains_digest(report),
        _ => false,
    }
}
