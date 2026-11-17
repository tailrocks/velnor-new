// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::error::CiError;
use velnor_ci::setup::{Reported, check_tools, parse_reported};
use velnor_model::{MisePlatform, ToolLock, ToolPin, ToolPins, ToolRequest};

type Case<'a> = (&'a [&'a str], &'a [&'a str], &'a [(&'a str, bool)], bool);

fn pins(requested: &[&str], locked: &[&str]) -> ToolPins {
    ToolPins {
        configs: Vec::new(),
        locked_file: true,
        tools: BTreeMap::from([(
            "tool".to_owned(),
            ToolPin {
                requested: requested
                    .iter()
                    .map(|v| ToolRequest {
                        version: (*v).to_owned(),
                        ..ToolRequest::default()
                    })
                    .collect(),
                locked: locked
                    .iter()
                    .map(|v| ToolLock {
                        version: (*v).to_owned(),
                        ..ToolLock::default()
                    })
                    .collect(),
            },
        )]),
    }
}

fn check(pins: &ToolPins, reported: &Reported) -> Result<(), CiError> {
    check_tools(pins, reported, MisePlatform::new("linux", "x86_64"))
}

fn reported(versions: &[(&str, bool)]) -> String {
    let entries: Vec<String> = versions
        .iter()
        .map(|(version, installed)| {
            format!("{{\"version\":\"{version}\",\"installed\":{installed}}}")
        })
        .collect();
    format!("{{\"tool\":[{}]}}", entries.join(","))
}

#[test]
fn a_tool_passes_only_when_an_installed_version_equals_the_lock_or_the_pin() {
    let cases: [Case<'_>; 10] = [
        (&["1.2.3"], &[], &[("1.2.3", true)], true),
        (&["1.2.3"], &[], &[("1.2.4", true)], false),
        (&["1.2"], &[], &[("1.2.9", true)], true),
        (&["1.2"], &[], &[("1.20.0", true)], false),
        (&["1"], &["1.2.3"], &[("1.9.0", true)], false),
        (&["1"], &["1.2.3"], &[("1.2.3", true)], true),
        (&["1.2.3"], &[], &[("1.2.3", false)], false),
        (&["1.2.3"], &[], &[], false),
        (&["1.2"], &["1.2.3"], &[("1.2.3.1", true)], false),
        (&["1.2"], &[], &[("1.2.3.1", true)], true),
    ];
    for (requested, locked, installed, ok) in cases {
        let json = reported(installed);
        let result = check(
            &pins(requested, locked),
            &parse_reported(&json).unwrap_or_else(|e| unreachable!("{e}")),
        );
        assert_eq!(result.is_ok(), ok, "{requested:?} {locked:?} {installed:?}");
    }
}

#[test]
fn an_alias_passes_when_mise_reports_it_as_the_request_it_resolved() {
    let json =
        "{\"tool\":[{\"version\":\"24.11.1\",\"requested_version\":\"lts\",\"installed\":true}]}";
    let reported = parse_reported(json).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(check(&pins(&["lts"], &[]), &reported), Ok(()));
    assert!(check(&pins(&["latest-lts"], &[]), &reported).is_err());
    assert!(check(&pins(&["lts"], &["24.11.0"]), &reported).is_err());
}

#[test]
fn every_requested_version_must_be_installed() {
    let reported = parse_reported(&reported(&[("20.1.0", true), ("22.3.0", false)]))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let error = check(&pins(&["20", "22"], &[]), &reported)
        .err()
        .unwrap_or_else(|| unreachable!("a missing version must fail"));
    assert!(error.to_string().contains("22"), "{error}");
    assert!(!error.to_string().contains("pinned 20"), "{error}");
    let both = parse_reported(&reported_both()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(check(&pins(&["20", "22"], &[]), &both), Ok(()));
}

fn reported_both() -> String {
    reported(&[("20.1.0", true), ("22.3.0", true)])
}

fn filtered(requested: &[(&str, &[&str])], locked: &[&str]) -> ToolPins {
    let mut pins = pins(
        &requested.iter().map(|(v, _)| *v).collect::<Vec<_>>(),
        locked,
    );
    if let Some(pin) = pins.tools.get_mut("tool") {
        for (request, (_, filters)) in pin.requested.iter_mut().zip(requested) {
            if !filters.is_empty() {
                request.os = Some(filters.iter().map(|filter| (*filter).to_owned()).collect());
            }
        }
    }
    pins
}

#[test]
fn a_request_filtered_to_another_os_is_not_required_here() {
    let both = [("1.22", &["macos"][..]), ("1.23", &["linux"][..])];
    let linux =
        parse_reported(&reported(&[("1.23.4", true)])).unwrap_or_else(|e| unreachable!("{e}"));
    let mac =
        parse_reported(&reported(&[("1.22.9", true)])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        check_tools(
            &filtered(&both, &[]),
            &linux,
            MisePlatform::new("linux", "x86_64")
        ),
        Ok(())
    );
    assert_eq!(
        check_tools(
            &filtered(&both, &[]),
            &mac,
            MisePlatform::new("macos", "aarch64")
        ),
        Ok(())
    );
    assert_eq!(
        check_tools(
            &filtered(&both, &["1.22.9", "1.23.4"]),
            &linux,
            MisePlatform::new("linux", "x86_64")
        ),
        Ok(())
    );
    let error = check_tools(
        &filtered(&both, &[]),
        &mac,
        MisePlatform::new("linux", "x86_64"),
    )
    .err()
    .unwrap_or_else(|| unreachable!("the applicable version is missing"));
    assert!(error.to_string().contains("1.23"), "{error}");
}

#[test]
fn overlapping_requests_each_need_their_own_installed_entry() {
    let narrow =
        "{\"tool\":[{\"version\":\"1.22.12\",\"requested_version\":\"1.22\",\"installed\":true}]}";
    let wide = "{\"tool\":[{\"version\":\"1.22.12\",\"requested_version\":\"1.22\",\"installed\":true},{\"version\":\"1.26.7\",\"requested_version\":\"1\",\"installed\":true}]}";
    let narrow = parse_reported(narrow).unwrap_or_else(|e| unreachable!("{e}"));
    let wide = parse_reported(wide).unwrap_or_else(|e| unreachable!("{e}"));
    let error = check(&pins(&["1", "1.22"], &[]), &narrow)
        .err()
        .unwrap_or_else(|| unreachable!("request 1 has no entry"));
    assert!(error.to_string().contains("pinned 1,"), "{error}");
    assert_eq!(check(&pins(&["1", "1.22"], &[]), &wide), Ok(()));
}

#[test]
fn mise_platform_filters_require_only_tools_for_the_current_os_and_architecture() {
    let cases = [
        ("unix", "linux", "x86_64", true),
        ("unix", "macos", "aarch64", true),
        ("unix", "windows", "x86_64", false),
        ("win", "windows", "x86_64", true),
        ("linux/x64", "linux", "x86_64", true),
        ("linux/x64", "linux", "aarch64", false),
        ("linux/arm64", "linux", "aarch64", true),
        ("linux/arm64", "linux", "x86_64", false),
        ("macos/arm64", "macos", "x86_64", false),
        ("darwin/aarch64", "macos", "aarch64", true),
        ("linux/amd64", "linux", "x86_64", true),
    ];
    let absent = Reported::new();
    let mismatches: Vec<_> = cases
        .into_iter()
        .filter(|(filter, os, arch, required)| {
            check_tools(
                &filtered(&[("1.0.0", &[*filter])], &["1.0.0"]),
                &absent,
                MisePlatform::new(os, arch),
            )
            .is_err()
                != *required
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "wrong platform decisions: {mismatches:?}"
    );
}

fn parsed_tools(config: &str, lock: &str) -> ToolPins {
    let files = BTreeMap::from([
        (
            velnor_model::RepoPath::from_static("mise.toml"),
            config.as_bytes().to_vec(),
        ),
        (
            velnor_model::RepoPath::from_static("mise.lock"),
            lock.as_bytes().to_vec(),
        ),
    ]);
    velnor_rust::tool_pins(&files).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn resolved_lock_rows_follow_their_exact_requests_instead_of_overlapping_versions() {
    let cases = [
        ("lts", "24.11.1", "1.2.3"),
        ("stable", "1.99.1", "1.2.3"),
        ("nightly", "1.100.0-nightly", "1.2.3"),
        ("latest", "9.9.9", "1.2.3"),
        ("1.22", "1.22.9", "1"),
    ];
    let mut mismatches = Vec::new();
    for (mac_request, mac_version, linux_request) in cases {
        let linux_version = if linux_request == "1" {
            "1.26.0"
        } else {
            linux_request
        };
        let config = format!(
            "[tools]\ntool = [{{ version = \"{mac_request}\", os = [\"macos\"] }}, {{ version = \"{linux_request}\", os = [\"linux\"] }}]\n"
        );
        let lock = format!(
            "[[tools.tool]]\nversion = \"{mac_version}\"\nspecifiers = [\"{mac_request}\"]\n[[tools.tool]]\nversion = \"{linux_version}\"\nspecifiers = [\"{linux_request}\"]\n"
        );
        let pins = parsed_tools(&config, &lock);
        for (os, version) in [("macos", mac_version), ("linux", linux_version)] {
            let installed = parse_reported(&reported(&[(version, true)]))
                .unwrap_or_else(|e| unreachable!("{e}"));
            if let Err(error) = check_tools(&pins, &installed, MisePlatform::new(os, "x86_64")) {
                mismatches.push(format!("{mac_request}/{linux_request} on {os}: {error}"));
            }
        }
    }
    assert!(mismatches.is_empty(), "wrong resolved rows: {mismatches:?}");
}

#[test]
fn a_locked_alias_filtered_to_another_os_requires_nothing_here() {
    let mut errors = Vec::new();
    for alias in ["lts", "stable", "nightly", "beta"] {
        let config = format!("[tools]\ntool = {{ version = \"{alias}\", os = [\"macos\"] }}\n");
        let lock = format!("[[tools.tool]]\nversion = \"24.11.1\"\nspecifiers = [\"{alias}\"]\n");
        if let Err(error) = check_tools(
            &parsed_tools(&config, &lock),
            &Reported::new(),
            MisePlatform::new("linux", "x86_64"),
        ) {
            errors.push(format!("{alias}: {error}"));
        }
    }
    assert!(errors.is_empty(), "foreign aliases required: {errors:?}");
}

#[test]
fn architecture_specific_requests_require_their_own_resolved_lock_rows() {
    let config = "[tools]\ntool = [{ version = \"1\", os = [\"linux/x64\"] }, { version = \"2\", os = [\"linux/arm64\"] }]\n";
    let lock = "[[tools.tool]]\nversion = \"1.0.0\"\nspecifiers = [\"1\"]\n[[tools.tool]]\nversion = \"2.0.0\"\nspecifiers = [\"2\"]\n";
    let pins = parsed_tools(config, lock);
    for (arch, right, wrong) in [("x86_64", "1.0.0", "2.0.0"), ("aarch64", "2.0.0", "1.0.0")] {
        let right =
            parse_reported(&reported(&[(right, true)])).unwrap_or_else(|e| unreachable!("{e}"));
        let wrong =
            parse_reported(&reported(&[(wrong, true)])).unwrap_or_else(|e| unreachable!("{e}"));
        let platform = MisePlatform::new("linux", arch);
        assert_eq!(check_tools(&pins, &right, platform), Ok(()), "{arch}");
        assert!(check_tools(&pins, &wrong, platform).is_err(), "{arch}");
    }
}

#[test]
fn ambiguous_legacy_lock_rows_refuse_until_the_requests_are_recorded() {
    let config = "[tools]\ntool = [{ version = \"latest\", os = [\"macos\"] }, { version = \"1.2.3\", os = [\"linux\"] }]\n";
    let ambiguous = "[[tools.tool]]\nversion = \"9.9.9\"\n[[tools.tool]]\nversion = \"1.2.3\"\n";
    let both = parse_reported(&reported(&[("9.9.9", true), ("1.2.3", true)]))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let platform = MisePlatform::new("macos", "aarch64");
    assert!(check_tools(&parsed_tools(config, ambiguous), &both, platform).is_err());
    let recorded = "[[tools.tool]]\nversion = \"9.9.9\"\nspecifiers = [\"latest\"]\n[[tools.tool]]\nversion = \"1.2.3\"\nspecifiers = [\"1.2.3\"]\n";
    let latest =
        parse_reported(&reported(&[("9.9.9", true)])).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        check_tools(&parsed_tools(config, recorded), &latest, platform),
        Ok(())
    );
}

#[test]
fn prepare_refuses_a_configured_backend_option_missing_from_the_lock_identity() {
    let config = "[tools]\ntool = { version = \"1\", flavor = \"linux\" }\n";
    let row = "[[tools.tool]]\nversion = \"1.0.0\"\nspecifiers = [\"1\"]\n";
    let installed =
        parse_reported(&reported(&[("1.0.0", true)])).unwrap_or_else(|e| unreachable!("{e}"));
    let platform = MisePlatform::new("linux", "x86_64");
    let missing = check_tools(&parsed_tools(config, row), &installed, platform);
    assert!(missing.is_err(), "missing option identity: {missing:?}");
    let recorded = format!("{row}options = {{ flavor = \"linux\" }}\n");
    assert_eq!(
        check_tools(&parsed_tools(config, &recorded), &installed, platform),
        Ok(())
    );
    let controls = "[tools]\ntool = { version = \"1\", lazy = true, depends = [\"rust\"] }\n";
    assert_eq!(
        check_tools(&parsed_tools(controls, row), &installed, platform),
        Ok(())
    );
}

#[test]
fn documented_rust_and_packslip_controls_preserve_their_recorded_artifact_identity() {
    let cases = [
        (
            "rust",
            "[tools]\nrust = { version = \"1.99.1\", profile = \"minimal\", components = [\"rustfmt\", \"clippy\", \"clippy\"], targets = [\"x86_64-unknown-linux-gnu\", \"aarch64-unknown-linux-gnu\"], mr_boxington = true }\n",
            "[[tools.rust]]\nversion = \"1.99.1\"\nbackend = \"core:rust\"\nspecifiers = [\"1.99.1\"]\noptions = { profile = \"minimal\", components = \"clippy,rustfmt\", targets = \"aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu\" }\n",
            "1.99.1",
        ),
        (
            "mr-boxington",
            "[tools]\nmr-boxington = { version = \"1.29.2\", workflow = \"release.yml@refs/heads/main\" }\n",
            "[[tools.mr-boxington]]\nversion = \"1.29.2\"\nbackend = \"packslip:github.com/jdx/mr-boxington\"\nspecifiers = [\"1.29.2\"]\n",
            "1.29.2",
        ),
    ];
    let mut mismatches = Vec::new();
    for (name, config, lock, version) in cases {
        let installed = parse_reported(&format!(
            "{{\"{name}\":[{{\"version\":\"{version}\",\"installed\":true}}]}}"
        ))
        .unwrap_or_else(|e| unreachable!("{e}"));
        if let Err(error) = check_tools(
            &parsed_tools(config, lock),
            &installed,
            MisePlatform::new("linux", "x86_64"),
        ) {
            mismatches.push(format!("{name}: {error}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "wrong control identities: {mismatches:?}"
    );
}
