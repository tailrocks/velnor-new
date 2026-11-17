// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::view;

fn npm_config(request: &str, options: &str, vars: &str) -> TestResult {
    npm_locked(request, options, vars, "24")
}

fn npm_locked(request: &str, options: &str, vars: &str, version: &str) -> TestResult {
    let mut repo = view(&["Cargo.toml"], None)?;
    let config = RepoPath::from_static("mise.toml");
    let locked = RepoPath::from_static("mise.lock");
    let mise = String::from_utf8(repo.files.get(&config).ok_or("no config")?.clone())?;
    let lock = String::from_utf8(repo.files.get(&locked).ok_or("no lock")?.clone())?;
    repo.files.insert(
        config,
        format!("{mise}\"npm:fixture\" = {{ {request} }}\n{vars}").into_bytes(),
    );
    repo.files.insert(locked, format!("{lock}\n[[tools.\"npm:fixture\"]]\nversion = \"{version}.0.0\"\nbackend = \"npm:fixture\"\nspecifiers = [\"{version}\"]\n{options}\n[tools.\"npm:fixture\".\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/npm\"\n").into_bytes());
    velnor_rust::check_tools(&velnor_rust::detect(&repo)?)?;
    expected_files(&repo)?;
    Ok(())
}

#[test]
fn npm_lock_identity_canonicalizes_lists() -> TestResult {
    for key in ["allow_builds", "trust_policy_excludes", "allow_exotic_deps"] {
        npm_config(
            &format!("version = \"24\", {key} = [\"z\", \"a\", \"z\", \"\"]"),
            &format!("options = {{ {key} = '[\"a\", \"z\"]' }}"),
            "",
        )?;
        assert!(
            npm_config(
                &format!("version = \"24\", {key} = [\"different\"]"),
                &format!("options = {{ {key} = '[\"a\", \"z\"]' }}"),
                ""
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn npm_lock_identity_omits_default_options() -> TestResult {
    npm_config(
        "version = \"24\", allow_builds = false, trust_policy_excludes = [], allow_exotic_deps = false",
        "",
        "",
    )?;
    assert!(npm_config("version = \"24\", allow_builds = true", "", "").is_err());
    Ok(())
}

#[test]
fn templated_versions_use_the_rendered_lock_specifier() -> TestResult {
    npm_config("version = \"{{ vars.v }}\"", "", "[vars]\nv = \"24\"\n")?;
    npm_config(
        "version = \"{{ vars.v | trim }}\"",
        "",
        "[vars]\nv = \" 24 \"\n",
    )?;
    npm_config(
        "version = \"{{ vars.alias }}\"",
        "",
        "[vars]\nv = \"24\"\nalias = \"{{ vars.v }}\"\n",
    )?;
    assert!(npm_config("version = \"{{ vars.v }}\"", "", "[vars]\nv = \"25\"\n").is_err());
    assert!(
        npm_config(
            "version = \"{{ vars.missing }}\"",
            "",
            "[vars]\nv = \"24\"\n"
        )
        .is_err()
    );
    assert!(
        npm_config(
            "version = \"{{ vars.v }}\"",
            "",
            "[vars]\nv = \"{{ vars.v }}\"\n"
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn templated_versions_follow_vars_declaration_order() -> TestResult {
    let forward = "[vars]\nz = '24'\na = \"{{ vars.z | default(value='25') }}\"\n";
    let reverse = "[vars]\na = \"{{ vars.z | default(value='25') }}\"\nz = '24'\n";
    let request = "version = \"{{ vars.a }}\"";
    npm_locked(request, "", forward, "24")?;
    assert!(npm_locked(request, "", forward, "25").is_err());
    npm_locked(request, "", reverse, "25")?;
    assert!(npm_locked(request, "", reverse, "24").is_err());
    Ok(())
}

#[test]
fn templated_versions_use_mise_string_values_for_primitive_vars() -> TestResult {
    for (value, condition) in [
        ("24", "vars.n == '24'"),
        ("true", "vars.n == 'true'"),
        ("false", "vars.n is undefined"),
        ("{ value = 24 }", "vars.n == '24'"),
    ] {
        let request = format!("version = \"{{% if {condition} %}}24{{% else %}}25{{% endif %}}\"");
        let vars = format!("[vars]\nn = {value}\n");
        npm_locked(&request, "", &vars, "24")?;
        assert!(npm_locked(&request, "", &vars, "25").is_err(), "{value}");
    }
    for value in ["24.0", "[24]", "{ nested = 24 }"] {
        let error = npm_locked(
            "version = \"{{ vars.n }}\"",
            "",
            &format!("[vars]\nn = {value}\n"),
            "24",
        )
        .err()
        .ok_or("invalid var was accepted")?;
        assert!(error.to_string().contains("mise.toml"), "{error}");
    }
    Ok(())
}

#[test]
fn version_templates_reject_unavailable_context_even_in_guarded_branches() -> TestResult {
    for (template, vars) in [
        ("{% if env is defined %}25{% else %}24{% endif %}", ""),
        ("{{ env | default(value='24') }}", ""),
        (
            "{% if __tera_context is defined %}24{% else %}25{% endif %}",
            "",
        ),
        (
            "{% if env.VERSION is defined %}{{ env.VERSION }}{% else %}24{% endif %}",
            "",
        ),
        ("{{ env.VERSION | default(value='24') }}", ""),
        (
            "{{ vars.a }}",
            "[vars]\na = \"{{ env.VERSION | default(value='24') }}\"\n",
        ),
        (
            "{% if false %}{% set env = {} %}{% endif %}{{ env.VERSION | default(value='24') }}",
            "",
        ),
        (
            "{% if false %}{{ exec(command='anything') }}{% endif %}24",
            "",
        ),
    ] {
        let files = std::collections::BTreeMap::from([(
            RepoPath::from_static("mise.toml"),
            format!("[tools]\nnode = \"{template}\"\n{vars}").into_bytes(),
        )]);
        let error = velnor_rust::tool_pins(&files)
            .err()
            .ok_or("unavailable context was accepted")?;
        assert!(error.to_string().contains("unsupported"), "{error}");
    }
    for template in [
        "{{ vars.missing | default(value='24') }}",
        "{% if vars.missing is defined %}25{% else %}24{% endif %}",
        "{{ 'env.VERSION' | replace(from='env.VERSION', to='24') }}",
        "{% if '{% set env = 1 %}' == '{% set env = 1 %}' %}24{% endif %}",
    ] {
        npm_config(&format!("version = \"{template}\""), "", "")?;
    }
    Ok(())
}

#[test]
fn linux_aarch64_uses_arm_policy_tools_and_prebuilts() -> TestResult {
    let platform = velnor_model::MisePlatform::runner("linux-aarch64").ok_or("no platform")?;
    assert_eq!(platform.lock_key(), "linux-arm64");
    assert!(platform.matches("linux/arm64"));
    assert!(!platform.matches("linux/x64"));
    assert!(
        crate::mise_platform::generate("linux/arm64", "linux-aarch64", true, true)?
            .contains("cargo-deny check")
    );
    assert!(
        crate::mise_platform::generate("linux/x64", "linux-aarch64", true, true)
            .is_err_and(|error| error.contains("not pinned"))
    );
    assert!(
        crate::mise_platform::generate("linux/arm64", "linux-aarch64", false, false)
            .is_err_and(|error| error.contains("prebuilt"))
    );
    Ok(())
}
