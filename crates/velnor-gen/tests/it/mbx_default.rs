// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_files};
use velnor_model::{RepoPath, RepoView};
use velnor_rust::{DetectError, detect};

use crate::facts::{TestResult, lock_pinned_tools};
use crate::pipeline::{pin_helper, view, workflow};
use crate::yaml_tree::document;

const STORE: &str = "MBX_CACHE_DIR=$RUNNER_TEMP/mbx";
const OPT_OUT: &[u8] = b"[cache]\nmbx = false\n";
const NO_LAYOUT: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";
const PIN: &str = "mr-boxington = \"v1.30.0\"\n";
const OPTION: &str = "rust = { version = \"1.99.1\", mr_boxington = true }\n";
const WRAPPER: &str =
    "\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = { MBX_CARGO_SHIM_MODE = \"1\" }\n";
const NEXTEST: &str = "\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";

fn with_mise(repo: RepoView, text: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    with_mises(repo, text, None)
}

fn with_mises(
    mut repo: RepoView,
    text: &str,
    hidden: Option<&str>,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    repo.files
        .insert(RepoPath::new("mise.toml")?, text.as_bytes().to_vec());
    if let Some(hidden) = hidden {
        repo.files
            .insert(RepoPath::new(".mise.toml")?, hidden.as_bytes().to_vec());
    }
    repo.files.remove(&RepoPath::new("mise.lock")?);
    lock_pinned_tools(&mut repo)?;
    Ok(repo)
}

fn pinned(config: Option<&[u8]>, version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml"], config)?;
    pin_helper(&mut repo, version)?;
    Ok(repo)
}

fn steps(text: &str) -> Result<Vec<(String, serde_json::Value)>, Box<dyn std::error::Error>> {
    let yaml = document(text)?;
    let mut found = Vec::new();
    for (id, job) in yaml["jobs"].as_object().ok_or("no jobs")? {
        for step in job["steps"].as_array().ok_or("no steps")? {
            found.push((id.clone(), step.clone()));
        }
    }
    Ok(found)
}

fn runs_cargo(step: &serde_json::Value) -> bool {
    step["run"]
        .as_str()
        .is_some_and(|run| run.contains(" run -- ") && !run.contains("cargo-fmt"))
}

#[test]
fn a_default_render_gives_every_compiling_job_a_private_mbx_store_before_the_tools_install()
-> TestResult {
    let text = workflow(&pinned(None, "0.1.88")?)?;
    let yaml = document(&text)?;
    let mut compiled = 0;
    for (id, job) in yaml["jobs"].as_object().ok_or("no jobs")? {
        let jobs_steps = job["steps"].as_array().ok_or("no steps")?;
        let store = jobs_steps
            .iter()
            .position(|step| step["run"].as_str().is_some_and(|run| run.contains(STORE)));
        let tools = jobs_steps.iter().position(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
        });
        let first_cargo = jobs_steps.iter().position(runs_cargo);
        if let Some(cargo) = first_cargo {
            compiled += 1;
            let (store, tools) = (
                store.ok_or(format!("{id} compiles without a private store"))?,
                tools.ok_or("no tools step")?,
            );
            assert!(store < tools && tools < cargo, "{id}");
        }
    }
    assert_ne!(compiled, 0);
    assert!(!text.contains("velnor-ci mbx-store"));
    Ok(())
}

#[test]
fn a_job_that_compiles_nothing_gets_no_mbx_store() -> TestResult {
    let mut repo = pinned(None, "0.1.88")?;
    let mise = format!("{NO_LAYOUT}cargo-machete = \"0.9.1\"\n")
        .replace("rust = \"1.99.1\"\n", &format!("{OPTION}{PIN}"))
        + WRAPPER;
    repo = with_mise(repo, &mise)?;
    let text = workflow(&repo)?;
    let policy = text.split("\n  policy:\n").nth(1).ok_or("no policy job")?;
    let policy = policy.split("\n  required:\n").next().ok_or("no end")?;
    assert!(!policy.contains(STORE), "{policy}");
    Ok(())
}

#[test]
fn the_opt_out_renders_no_mbx_store_and_needs_no_mbx_layout() -> TestResult {
    let repo = with_mise(pinned(Some(OPT_OUT), "0.1.108")?, NO_LAYOUT)?;
    let text = workflow(&repo)?;
    assert!(!text.contains("MBX_CACHE_DIR"));
    assert!(!text.contains("mr-boxington"));
    assert!(steps(&text)?.iter().any(|(_, step)| runs_cargo(step)));
    Ok(())
}

#[test]
fn the_opt_out_is_named_as_a_check_that_does_less() -> TestResult {
    let repo = with_mise(pinned(Some(OPT_OUT), "0.1.108")?, NO_LAYOUT)?;
    let weakenings: Vec<String> = detect(&repo)?
        .weakenings
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(weakenings.len(), 1, "{weakenings:?}");
    assert!(
        weakenings[0].starts_with(".velnor/rust-ci.toml: cache.mbx = false"),
        "{weakenings:?}"
    );
    let standard = detect(&pinned(None, "0.1.88")?)?;
    assert!(standard.weakenings.is_empty(), "{:?}", standard.weakenings);
    Ok(())
}

type Refusal = (String, Option<String>, &'static str);

fn refusal_cases() -> Vec<Refusal> {
    let sccache = "[wrappers.cargo]\ncommand = \"sccache\"\n";
    vec![
        (NO_LAYOUT.to_owned(), None, "pin"),
        (
            format!("[tools]\nrust = \"1.99.1\"\n{PIN}{NEXTEST}{WRAPPER}"),
            None,
            "option",
        ),
        (format!("[tools]\n{OPTION}{NEXTEST}"), None, "pin"),
        (
            format!("[tools]\n{OPTION}{PIN}{NEXTEST}\n[wrappers.cargo]\ncommand = \"mbx\"\n"),
            None,
            "shim",
        ),
        (
            format!(
                "[tools]\n{OPTION}{PIN}{NEXTEST}\n[wrappers.cargo]\ncommand = \"sccache\"\nenv = {{ MBX_CARGO_SHIM_MODE = \"1\" }}\n"
            ),
            None,
            "wrapper",
        ),
        (
            format!(
                "[tools]\n{OPTION}{PIN}{NEXTEST}\n[wrappers.cargo]\ncommand = \"mbx\"\nenv = {{ MBX_CARGO_SHIM_MODE = \"1\", EXTRA = \"1\" }}\n"
            ),
            None,
            "extras",
        ),
        (
            format!(
                "[tools]\n{OPTION}mr-boxington = {{ version = \"v1.30.0\", os = [\"linux\"] }}\n{NEXTEST}"
            ),
            None,
            "pin",
        ),
        (
            format!("[tools]\n{OPTION}mr-boxington = [\"v1.30.0\", \"v1.29.0\"]\n{NEXTEST}"),
            None,
            "pin",
        ),
        (
            format!(
                "[tools]\nrust = {{ version = \"1.99.1\", mr_boxington = \"true\" }}\n{PIN}{NEXTEST}"
            ),
            None,
            "option",
        ),
        (format!("[tools]\n{PIN}{NEXTEST}"), None, "norust"),
        (
            format!("[tools]\n{OPTION}{PIN}{NEXTEST}{WRAPPER}"),
            Some(sccache.to_owned()),
            "split",
        ),
        (
            format!("[tools]\nrust = \"1.99.1\"\n{PIN}{NEXTEST}"),
            Some(format!("[tools]\n{OPTION}")),
            "split",
        ),
        (
            format!("[tools]\n{OPTION}{PIN}{NEXTEST}"),
            Some(WRAPPER.to_owned()),
            "split",
        ),
    ]
}

fn assert_refused((mise, hidden, missing): Refusal) -> TestResult {
    let repo = with_mises(pinned(None, "0.1.88")?, &mise, hidden.as_deref())?;
    let error = expected_files(&repo)
        .err()
        .ok_or(format!("accepted: {mise} {hidden:?}"))?;
    let GenerateError::Detect(detect) = &error else {
        return Err(format!("{error}").into());
    };
    let text = error.to_string();
    match (missing, detect) {
        ("pin", DetectError::MbxPinRequired { .. }) => {
            assert!(text.contains("mr-boxington = {"), "{text}");
            assert!(text.contains("`aqua:` or `github:`"), "{text}");
        }
        ("split", DetectError::MbxConfigSplit(split)) => {
            assert_eq!(split.first.as_str(), "mise.toml", "{text}");
            assert_eq!(split.second.as_str(), ".mise.toml", "{text}");
            assert!(text.contains("mbx = false"), "{text}");
            return Ok(());
        }
        ("wrapper", DetectError::MbxLayout(layout)) => {
            assert!(layout.wrapper_refused() && !layout.shim_missing(), "{mise}");
            assert!(!layout.rust_option, "{mise}");
            assert!(text.contains("mr_boxington = true"), "{text}");
        }
        ("extras", DetectError::MbxLayout(layout)) => {
            assert!(
                layout.wrapper_has_extras() && !layout.shim_missing(),
                "{mise}"
            );
            assert!(text.contains("extra keys"), "{text}");
            assert!(!text.contains("cargo -V"), "{text}");
        }
        ("norust", DetectError::MbxLayout(layout)) => {
            assert!(
                layout.wrapper_refused() && !layout.rust && !layout.rust_option,
                "{mise}"
            );
            assert!(
                !layout.shim_missing() && !layout.wrapper_has_extras(),
                "{mise}"
            );
            assert!(text.contains("pins no rust tool"), "{text}");
        }
        ("option" | "shim", DetectError::MbxLayout(layout)) => {
            assert_eq!(layout.rust_option, missing == "option", "{mise}");
            assert_eq!(layout.shim_missing(), missing == "shim", "{mise}");
            assert!(text.contains("mr_boxington = true"), "{text}");
            if missing == "shim" {
                assert!(text.contains("MBX_CARGO_SHIM_MODE = \"1\""), "{text}");
                assert!(text.contains("cargo -V"), "{text}");
                assert!(text.contains("native layout"), "{text}");
            }
        }
        _ => return Err(format!("{missing}: {error}").into()),
    }
    assert!(text.contains("mbx = false"), "{text}");
    Ok(())
}

#[test]
fn a_mise_configuration_that_does_not_route_cargo_through_mbx_is_refused_with_the_snippet()
-> TestResult {
    for case in refusal_cases() {
        assert_refused(case)?;
    }
    Ok(())
}

#[test]
fn the_whole_layout_in_the_hidden_mise_file_is_accepted_next_to_an_unrelated_mise_toml()
-> TestResult {
    let repo = with_mises(
        pinned(None, "0.1.88")?,
        &format!("[tools]\n{NEXTEST}"),
        Some(&format!("[tools]\n{OPTION}{PIN}{WRAPPER}")),
    )?;
    let text = workflow(&repo)?;
    assert!(text.contains(STORE), "{text}");
    Ok(())
}

#[test]
fn the_native_layout_is_accepted_without_a_cargo_wrapper() -> TestResult {
    let repo = with_mise(
        pinned(None, "0.1.88")?,
        &format!("[tools]\n{OPTION}{PIN}{NEXTEST}"),
    )?;
    let text = workflow(&repo)?;
    assert!(text.contains(STORE), "{text}");
    Ok(())
}

#[test]
fn the_explicit_layout_with_the_shim_env_is_still_accepted() -> TestResult {
    let repo = with_mise(
        pinned(None, "0.1.88")?,
        &format!("[tools]\n{OPTION}{PIN}{NEXTEST}{WRAPPER}"),
    )?;
    let text = workflow(&repo)?;
    assert!(text.contains(STORE), "{text}");
    Ok(())
}

#[test]
fn a_shared_store_or_a_remote_cache_keeps_its_own_wiring_and_gets_no_private_store() -> TestResult {
    for (config, version, wired) in [
        (
            &b"mbx_shared_store = true\n"[..],
            "0.1.88",
            "velnor-ci mbx-store",
        ),
        (
            &b"[cache]\nmbx_remote = \"github-actions\"\n"[..],
            "0.1.88",
            "jdx/mr-boxington-action@",
        ),
    ] {
        let text = workflow(&pinned(Some(config), version)?)?;
        assert!(text.contains(wired), "{wired}");
        assert!(!text.contains(STORE), "{wired}");
    }
    Ok(())
}

#[test]
fn turning_mbx_off_next_to_a_shared_store_or_a_remote_cache_is_refused() -> TestResult {
    for config in [
        &b"mbx_shared_store = true\n[cache]\nmbx = false\n"[..],
        b"[cache]\nmbx = false\nmbx_remote = \"github-actions\"\n",
    ] {
        let error = expected_files(&pinned(Some(config), "0.1.108")?)
            .err()
            .ok_or("accepted")?;
        assert!(error.to_string().contains("cache.mbx = false"), "{error}");
    }
    Ok(())
}

#[test]
fn the_mbx_key_needs_a_helper_that_reads_it_and_the_default_needs_nothing() -> TestResult {
    for config in [OPT_OUT, b"[cache]\nmbx = true\n"] {
        let old = with_mise(pinned(Some(config), "0.1.88")?, NO_LAYOUT)?;
        assert!(matches!(
            expected_files(&old).err(),
            Some(GenerateError::MbxKeyHelperTooOld(_))
        ));
        let mut current = pinned(Some(config), "0.1.108")?;
        if config == OPT_OUT {
            current = with_mise(current, NO_LAYOUT)?;
        }
        assert!(expected_files(&current).is_ok());
    }
    assert!(expected_files(&pinned(None, "0.1.88")?).is_ok());
    Ok(())
}
