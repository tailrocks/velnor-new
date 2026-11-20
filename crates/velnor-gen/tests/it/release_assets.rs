// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_sources::{SECTION, jobs, private_repo};

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_SHA: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
const SETUP: &str = "[[test_setup]]\nroot = '.'\nname = 'Prepare the fixture'\nrun = 'bash scripts/prepare-fixture.sh'\n";

fn assets() -> String {
    format!(
        "[[release_assets]]\nrepo = 'acme/fixtures'\ntag = 'fixtures-2026.11.1'\nasset = 'dataset.tar.zst'\nsha256 = '{SHA}'\ndest = 'tests/fixtures/dataset'\n"
    )
}

fn config(extra: &str) -> String {
    format!("{SECTION}{extra}")
}

fn position(steps: &[serde_json::Value], id_or_name: &str) -> Option<usize> {
    steps
        .iter()
        .position(|step| step["id"] == id_or_name || step["name"] == id_or_name)
}

fn phase_jobs(config: &str) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    let repo = private_repo(&["Cargo.toml", "tools/Cargo.toml"], config, "0.1.108")?;
    Ok(jobs(&workflow(&repo)?)?
        .into_iter()
        .filter(|job| {
            job["steps"]
                .as_array()
                .is_some_and(|steps| steps.iter().any(|s| s["id"] == "release-assets-token"))
        })
        .collect())
}

/// A step that runs cargo and so may run a build script: every `velnor-ci run --` command
/// except formatting, `cargo-machete` and `cargo-deny`, which run none.
fn compiles(step: &serde_json::Value) -> bool {
    step["run"].as_str().is_some_and(|run| {
        run.starts_with(". \"$VELNOR_EXEC\" run -- ")
            && !["cargo-fmt", "cargo-deny", "cargo-machete"]
                .iter()
                .any(|tool| run.contains(tool))
    })
}

fn runs_repository_code(step: &serde_json::Value) -> bool {
    compiles(step) || step["name"] == "Prepare the fixture" || step["id"] == "test"
}

#[test]
fn every_job_that_compiles_has_the_prelude_before_its_first_cargo_invocation() -> TestResult {
    for shape in ["", "shape = 'groups'\n", "shape = 'lint-test'\n"] {
        let repo = private_repo(
            &["Cargo.toml", "tools/Cargo.toml"],
            &format!("{shape}{}", config(&format!("{}{SETUP}", assets()))),
            "0.1.108",
        )?;
        let mut compiling_jobs = 0;
        for job in jobs(&workflow(&repo)?)? {
            let steps = job["steps"].as_array().ok_or("steps")?;
            let first = steps.iter().position(runs_repository_code);
            let token = position(steps, "release-assets-token");
            let Some(first) = first else {
                assert!(
                    token.is_none(),
                    "a job with no cargo step got the prelude: {job}"
                );
                continue;
            };
            compiling_jobs += 1;
            let restore =
                position(steps, "release-asset-0").ok_or_else(|| format!("{shape}: {job}"))?;
            let check = position(steps, "release-assets-check").ok_or("no check")?;
            let token = token.ok_or("no token")?;
            let fetch = steps
                .iter()
                .position(|s| {
                    s["run"]
                        .as_str()
                        .is_some_and(|r| r.contains("fetch-assets"))
                })
                .ok_or("no fetch")?;
            let save = position(steps, "Save the verified release asset dataset.tar.zst")
                .ok_or("no save")?;
            assert!(restore < check && check < token && token < fetch && fetch < save);
            assert!(
                save < first,
                "{shape}: prelude must precede step {first}: {job}"
            );
            assert_eq!(
                steps[restore]["with"]["key"],
                format!("release-asset-{SHA}")
            );
            assert_eq!(
                steps[restore]["with"]["path"],
                "tests/fixtures/dataset/dataset.tar.zst"
            );
            assert_eq!(steps[save]["with"]["key"], steps[restore]["with"]["key"]);
            let guard = steps[save]["if"].as_str().unwrap_or_default();
            assert!(
                guard.contains("steps.release-asset-0.outputs.cache-hit != 'true'"),
                "{guard}"
            );
            assert_eq!(job["permissions"]["pull-requests"], "read");
        }
        assert!(
            compiling_jobs >= 2,
            "{shape}: only {compiling_jobs} compiling jobs"
        );
    }
    Ok(())
}

#[test]
fn the_token_is_read_only_scoped_to_the_listed_repositories_fork_fenced_and_skipped_on_a_hit()
-> TestResult {
    let extra = format!(
        "{}[[release_assets]]\nrepo = 'acme/other-fixtures'\ntag = 'v2'\nasset = 'b.bin'\nsha256 = '{OTHER_SHA}'\ndest = 'tests/fixtures/b'\n",
        assets()
    );
    for job in phase_jobs(&config(&extra))? {
        let steps = job["steps"].as_array().ok_or("steps")?;
        let token = &steps[position(steps, "release-assets-token").ok_or("token")?];
        assert_eq!(token["with"]["permission-contents"], "read");
        assert_eq!(token["with"]["owner"], "acme");
        assert_eq!(
            token["with"]["repositories"].as_str().map(str::trim_end),
            Some("fixtures\nother-fixtures")
        );
        assert!(token["with"].get("skip-token-revoke").is_none(), "{token}");
        let guard = token["if"].as_str().unwrap_or_default();
        for needle in [
            "github.event.pull_request.head.repo.full_name == github.repository",
            "!endsWith(github.actor, '[bot]')",
            "steps.release-assets-check.outputs.allowed == 'true'",
            "steps.release-asset-0.outputs.cache-hit != 'true' || steps.release-asset-1.outputs.cache-hit != 'true'",
        ] {
            assert!(guard.contains(needle), "{needle}: {guard}");
        }
        assert!(
            guard.contains("&& ((github.event_name == 'merge_group'"),
            "{guard}"
        );
        let fetch = steps
            .iter()
            .find(|s| {
                s["run"]
                    .as_str()
                    .is_some_and(|r| r.contains("fetch-assets"))
            })
            .ok_or("fetch")?;
        assert_eq!(
            fetch["env"]["VELNOR_PRIVATE_TOKEN"],
            "${{ steps.release-assets-token.outputs.token }}"
        );
        assert_eq!(fetch["env"]["VELNOR_API_URL"], "${{ github.api_url }}");
        let run = fetch["run"].as_str().ok_or("run")?;
        assert!(run.contains(&format!("--asset acme/fixtures,fixtures-2026.11.1,dataset.tar.zst,{SHA},tests/fixtures/dataset")) || run.contains(&format!("'acme/fixtures,fixtures-2026.11.1,dataset.tar.zst,{SHA},tests/fixtures/dataset'")), "{run}");
        assert!(!run.contains("${{"), "{run}");
        for step in steps {
            let holds_key = step.to_string().contains("DEPS_APP_PRIVATE_KEY");
            let token_step = step["id"] == "release-assets-token" || step["id"] == "private-token";
            assert_eq!(holds_key, token_step, "{step}");
        }
        let check = &steps[position(steps, "release-assets-check").ok_or("check")?];
        assert!(
            check["if"]
                .as_str()
                .is_some_and(|g| g.contains("merge_group"))
        );
    }
    Ok(())
}

#[test]
fn a_config_without_the_table_renders_none_of_it() -> TestResult {
    let repo = private_repo(&["Cargo.toml"], SECTION, "0.1.108")?;
    let text = workflow(&repo)?;
    for forbidden in ["release-asset", "fetch-assets"] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
    Ok(())
}

#[test]
fn the_table_is_refused_by_a_helper_older_than_0_1_108() -> TestResult {
    let repo = private_repo(&["Cargo.toml"], &config(&assets()), "0.1.106")?;
    assert!(
        workflow(&repo).is_err_and(|e| e.to_string().contains("pin velnor-ci 0.1.108 or later"))
    );
    Ok(())
}

#[test]
fn every_misconfiguration_is_refused() -> TestResult {
    let good = assets();
    let bad = [
        good.replace(&format!("sha256 = '{SHA}'\n"), ""),
        good.replace(SHA, &SHA.to_uppercase()),
        good.replace(SHA, &SHA[1..]),
        good.replace(SHA, &"0".repeat(64).replace('0', "g")),
        good.replace("tests/fixtures/dataset", "../outside"),
        good.replace("tests/fixtures/dataset", "/etc"),
        good.replace("tests/fixtures/dataset", "a/../b"),
        good.replace("tests/fixtures/dataset", "a//b"),
        good.replace("tests/fixtures/dataset", ".git/hooks"),
        good.replace("tests/fixtures/dataset", "a\\b"),
        good.replace("tests/fixtures/dataset", "."),
        good.replace("tests/fixtures/dataset", "a/${{ github.sha }}"),
        good.replace("acme/fixtures", "fixtures"),
        good.replace("acme/fixtures", "acme/fixtures/extra"),
        good.replace("fixtures-2026.11.1", "v1 --flag"),
        good.replace("fixtures-2026.11.1", "${{ github.ref }}"),
        good.replace("dataset.tar.zst", "a;b"),
        good.replace("dataset.tar.zst", ".."),
        format!("{good}extra = 1\n"),
        format!(
            "{good}{}",
            good.replace("tests/fixtures/dataset", "tests/other")
        ),
        format!("{good}{}", good.replace(SHA, OTHER_SHA)),
        format!(
            "{good}{}",
            good.replace("acme/fixtures", "other/fixtures")
                .replace(SHA, OTHER_SHA)
        ),
    ];
    for table in bad {
        let repo = private_repo(&["Cargo.toml"], &config(&table), "0.1.108")?;
        assert!(workflow(&repo).is_err(), "{table}");
    }
    let without_credentials = private_repo(&["Cargo.toml"], &good, "0.1.108")?;
    assert!(workflow(&without_credentials).is_err());
    Ok(())
}

const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode']\npackages = ['package-0']\nfilter = 'test(~local::)'\n";

#[test]
fn the_prelude_precedes_clippy_doc_check_alone_the_tests_and_the_lane_steps() -> TestResult {
    let mut seen: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for shape in ["", "shape = 'groups'\n", "shape = 'lint-test'\n"] {
        let mut repo = private_repo(
            &["Cargo.toml", "tools/Cargo.toml"],
            &format!("{shape}{}", config(&format!("{}{LANE}", assets()))),
            "0.1.108",
        )?;
        let root = crate::facts::path("Cargo.toml")?;
        let member = repo
            .rust_metadata
            .get_mut(&root)
            .and_then(|metadata| metadata.members.get_mut(0))
            .ok_or("no member")?;
        member.targets.push(velnor_model::Target {
            name: velnor_model::TargetName::new("package-0")?,
            kinds: vec![velnor_model::TargetKind::Lib],
            tests: false,
            doctest: false,
            source: None,
            requires_features: false,
        });
        for job in jobs(&workflow(&repo)?)? {
            let steps = job["steps"].as_array().ok_or("steps")?;
            let Some(first) = steps.iter().position(compiles) else {
                continue;
            };
            let save = position(steps, "Save the verified release asset dataset.tar.zst")
                .ok_or_else(|| format!("{shape}: a compiling job has no prelude: {job}"))?;
            assert!(save < first);
            for step in steps.iter().filter(|step| compiles(step)) {
                let index = steps.iter().position(|s| s == step).ok_or("step")?;
                assert!(
                    save < index,
                    "{shape}: {} runs before the prelude",
                    step["name"]
                );
                seen.entry(shape.to_owned())
                    .or_default()
                    .insert(step["name"].as_str().unwrap_or_default().to_owned());
            }
        }
    }
    for (shape, names) in &seen {
        for needed in ["Lint", "Documentation", "Check package-0 alone"] {
            assert!(
                names.contains(needed),
                "{shape:?} lacks {needed}: {names:?}"
            );
        }
    }
    let default = &seen[""];
    // With `[private_sources]` the groups shape builds and runs the tests in one job, and a helper
    // of 0.1.107 records the build in its test step, so there is no separate nextest build step.
    for needed in [
        "Test",
        "Test (features: local-mode)",
        "Test lane guard (features: local-mode)",
    ] {
        assert!(default.contains(needed), "{default:?}");
    }
    Ok(())
}
