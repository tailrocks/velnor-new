// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_ci::setup::{check_tools, parse_reported};
use velnor_model::{MisePlatform, RepoPath};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn check(config: &str, lock: &str, installed: &str) -> TestResult {
    let files = BTreeMap::from([
        (
            RepoPath::from_static("mise.toml"),
            config.as_bytes().to_vec(),
        ),
        (RepoPath::from_static("mise.lock"), lock.as_bytes().to_vec()),
    ]);
    let pins = velnor_rust::tool_pins(&files)?;
    let reported = parse_reported(&format!(
        "{{\"npm:fixture\":[{{\"version\":\"{installed}.0.0\",\"installed\":true}}]}}"
    ))?;
    check_tools(
        &pins,
        &reported,
        MisePlatform::runner("linux-aarch64").ok_or("no runner platform")?,
    )?;
    Ok(())
}

fn lock(version: &str, options: &str) -> String {
    format!(
        "[[tools.\"npm:fixture\"]]\nversion = \"{version}.0.0\"\nspecifiers = [\"{version}\"]\nbackend = \"npm:fixture\"\n{options}\n"
    )
}

#[test]
fn prepare_matches_npm_canonical_options_and_omitted_defaults() -> TestResult {
    for (configured, recorded) in [
        (
            "allow_builds = [\"z\", \"a\", \"z\", \"\"]",
            "options = { allow_builds = '[\"a\", \"z\"]' }",
        ),
        (
            "allow_builds = false, trust_policy_excludes = [], allow_exotic_deps = false",
            "",
        ),
    ] {
        let config = format!(
            "[tools]\n\"npm:fixture\" = {{ version = \"24\", os = [\"linux/arm64\"], {configured} }}\n"
        );
        check(&config, &lock("24", recorded), "24")?;
        assert!(check(&config, &lock("24", recorded), "25").is_err());
    }
    Ok(())
}

#[test]
fn prepare_uses_rendered_versions_in_vars_declaration_order() -> TestResult {
    for (vars, version, stale) in [
        (
            "z = '24'\na = \"{{ vars.z | default(value='25') }}\"",
            "24",
            "25",
        ),
        (
            "a = \"{{ vars.z | default(value='25') }}\"\nz = '24'",
            "25",
            "24",
        ),
    ] {
        let config = format!(
            "[tools]\n\"npm:fixture\" = {{ version = \"{{{{ vars.a }}}}\", os = [\"linux/arm64\"] }}\n[vars]\n{vars}\n"
        );
        check(&config, &lock(version, ""), version)?;
        assert!(check(&config, &lock(version, ""), stale).is_err());
        assert!(check(&config, &lock(stale, ""), stale).is_err());
    }
    Ok(())
}

#[test]
fn prepare_uses_mise_string_values_for_primitive_vars() -> TestResult {
    for (value, condition) in [
        ("24", "vars.n == '24'"),
        ("true", "vars.n == 'true'"),
        ("false", "vars.n is undefined"),
        ("{ value = 24 }", "vars.n == '24'"),
    ] {
        let config = format!(
            "[tools]\n\"npm:fixture\" = \"{{% if {condition} %}}24{{% else %}}25{{% endif %}}\"\n[vars]\nn = {value}\n"
        );
        check(&config, &lock("24", ""), "24")?;
        assert!(check(&config, &lock("24", ""), "25").is_err(), "{value}");
        assert!(check(&config, &lock("25", ""), "25").is_err(), "{value}");
    }
    Ok(())
}
