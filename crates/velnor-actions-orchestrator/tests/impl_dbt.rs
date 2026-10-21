//! `[stacks.dbt]` project jobs through the real generator and planner.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use velnor_actions_contract::{JobDisposition, Plan};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};
use crate::impl_select::{commit, plan_pr, plan_push};
use crate::impl_select_affected_jobs::{fixture, put};

const SUM: &str = "665039bfac4f458eebcfd7cc421df454020e2a39d7e3f3ba98addd7c0252d70e";
const VERSION: &str = "2.0.0-preview.219";
const MISE: &str = "[tools]\ndbt-fusion = \"2.0.0-preview.219\"\n";

const DBT: &str = "
[stacks.dbt]
projects = [\"warehouse\"]
services = [\"postgres\"]
checks = [\"parse\", \"compile\"]
env = { DBT_HOST = \"127.0.0.1\", DBT_PORT = \"40000\", DBT_USER = \"chainargos\", DBT_PASSWORD = \"chainargos\", DBT_DBNAME = \"chainargos\" }
";

fn config(extra: &str) -> String {
    format!("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{extra}")
}

fn lock(checksum: bool) -> String {
    let sum = if checksum {
        format!("checksum = \"sha256:{SUM}\"\n")
    } else {
        String::new()
    };
    let row = |key: &str, triple: &str| {
        format!(
            "[tools.dbt-fusion.\"platforms.{key}\"]\n{sum}url = \"https://public.cdn.getdbt.com/fs/cli/fs-v{VERSION}-{triple}.tar.gz\"\n\n"
        )
    };
    format!(
        "lockfile_version = 3\n\n[[tools.dbt-fusion]]\nversion = \"{VERSION}\"\nbackend = \"aqua:getdbt.com/dbt-fusion\"\n\n{}{}{}",
        row("linux-arm64", "aarch64-unknown-linux-gnu"),
        row("linux-x64", "x86_64-unknown-linux-gnu"),
        row("macos-arm64", "aarch64-apple-darwin"),
    )
}

fn write_project(root: &Path, with_checksum: bool) -> TestResult {
    let write = |rel: &str, text: &str| -> TestResult {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().ok_or("parent")?)?;
        fs::write(path, text)?;
        Ok(())
    };
    write("mise.toml", MISE)?;
    write("mise.lock", &lock(with_checksum))?;
    write(
        "warehouse/dbt_project.yml",
        "name: warehouse\nprofile: warehouse\nversion: '1.0'\nmodel-paths: [\"models\"]\n",
    )?;
    write(
        "warehouse/profiles.yml",
        "warehouse:\n  target: ci\n  outputs:\n    ci:\n      type: postgres\n      host: \"{{ env_var('DBT_HOST') }}\"\n      password: \"{{ env_var('DBT_PASSWORD') }}\"\n",
    )?;
    write("warehouse/models/a.sql", "select 1 as id\n")?;
    git(&["add", "."], root)?;
    Ok(())
}

fn ci_yaml(repo: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(&prepare(repo)?)?;
    Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
}

#[test]
fn each_project_gets_one_job_with_a_service_and_parallel_checks() -> TestResult {
    let repo = make_repo(&config(DBT))?;
    write_project(repo.path(), true)?;
    let ci = ci_yaml(repo.path())?;
    let job = job_body(&ci, "dbt-warehouse")?;
    assert!(job.contains("name: dbt / warehouse"), "{job}");
    assert!(job.contains("runs-on: ubuntu-26.04\n"), "{job}");
    assert!(job.contains("contents: read"), "{job}");
    assert!(
        job.contains("services:") && job.contains("postgres@sha256:"),
        "{job}"
    );
    assert!(job.contains("persist-credentials: \"false\""), "{job}");
    let install = job.find("name: Install dbt-fusion").ok_or("install")?;
    let deps = job.find("name: dbt deps").ok_or("deps")?;
    assert!(install < deps, "{job}");
    for step in ["dbt parse", "dbt compile"] {
        let after = job
            .split(&format!("- name: {step}\n"))
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .map(str::trim);
        assert_eq!(after, Some("background: true"), "{step}: {job}");
        assert!(
            deps < job.find(&format!("name: {step}")).ok_or(step)?,
            "{job}"
        );
    }
    assert!(job.contains("compile --static-analysis off"), "{job}");
    assert!(job.contains("VELNOR_TOOL_DBT_FUSION"), "{job}");
    assert!(job.contains("Resolve project tool versions"), "{job}");
    assert!(job.contains("wait-all"), "{job}");
    assert!(job.contains("DBT_PASSWORD: chainargos"), "{job}");
    for step in ["Install dbt-fusion", "dbt deps", "dbt parse", "dbt compile"] {
        let body = job
            .split(&format!("- name: {step}\n"))
            .nth(1)
            .and_then(|rest| rest.split("\n      - name:").next())
            .ok_or(step)?;
        assert!(body.contains("DBT_PROJECT_DIR: ."), "{step}: {body}");
        assert!(body.contains("DBT_PROFILES_DIR: ."), "{step}: {body}");
    }
    assert!(!job.contains("secrets."), "{job}");
    let required = job_body(&ci, "required")?;
    assert!(required.contains("- dbt-warehouse"), "{required}");
    Ok(())
}

const EXPERIMENTAL: &str = "DBT_ALLOW_EXPERIMENTAL_ADAPTERS";

/// The step body after `- name: <step>`, up to the next step.
fn step_body<'a>(job: &'a str, step: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    job.split(&format!("- name: {step}\n"))
        .nth(1)
        .and_then(|rest| rest.split("\n      - name:").next())
        .ok_or_else(|| step.to_owned().into())
}

#[test]
fn a_postgres_profile_enables_the_experimental_adapter_gate_on_every_step() -> TestResult {
    let repo = make_repo(&config(DBT))?;
    write_project(repo.path(), true)?;
    let ci = ci_yaml(repo.path())?;
    let job = job_body(&ci, "dbt-warehouse")?;
    for step in ["Install dbt-fusion", "dbt deps", "dbt parse", "dbt compile"] {
        let body = step_body(job, step)?;
        assert!(
            body.contains(&format!("{EXPERIMENTAL}: \"true\""))
                || body.contains(&format!("{EXPERIMENTAL}: true")),
            "{step}: {body}"
        );
    }
    Ok(())
}

#[test]
fn another_adapter_does_not_get_the_experimental_gate() -> TestResult {
    let repo = make_repo(&config(
        "[stacks.dbt]\nprojects = [\"warehouse\"]\nchecks = [\"parse\"]\n",
    ))?;
    write_project(repo.path(), true)?;
    fs::write(
        repo.path().join("warehouse/profiles.yml"),
        "warehouse:\n  target: ci\n  outputs:\n    ci:\n      type: snowflake\n      account: \"{{ env_var('DBT_HOST') }}\"\n",
    )?;
    git(&["add", "."], repo.path())?;
    let ci = ci_yaml(repo.path())?;
    assert!(!job_body(&ci, "dbt-warehouse")?.contains(EXPERIMENTAL));
    Ok(())
}

#[test]
fn the_gate_stays_generator_owned_and_cannot_be_set_through_env() -> TestResult {
    let repo = make_repo(&config(
        "[stacks.dbt]\nprojects = [\"warehouse\"]\nenv = { DBT_ALLOW_EXPERIMENTAL_ADAPTERS = \"true\" }\n",
    ))?;
    write_project(repo.path(), true)?;
    let error = prepare(repo.path()).err().ok_or("env must stay closed")?;
    assert!(
        error
            .to_string()
            .contains("bad_env_name:DBT_ALLOW_EXPERIMENTAL_ADAPTERS"),
        "{error}"
    );
    Ok(())
}

#[test]
fn generation_is_deterministic() -> TestResult {
    let render = || -> Result<String, Box<dyn std::error::Error>> {
        let repo = make_repo(&config(DBT))?;
        write_project(repo.path(), true)?;
        ci_yaml(repo.path())
    };
    assert_eq!(render()?, render()?);
    Ok(())
}

#[test]
fn configuration_cannot_redirect_profiles_outside_the_project() -> TestResult {
    let redirected = DBT.replace(
        "DBT_HOST",
        r#"DBT_PROFILES_DIR = "../shared-profiles", DBT_HOST"#,
    );
    let repo = make_repo(&config(&redirected))?;
    write_project(repo.path(), true)?;
    let error = prepare(repo.path()).expect_err("redirected profile path must be rejected");
    assert!(
        error.to_string().contains("bad_env_name:DBT_PROFILES_DIR"),
        "{error}"
    );
    Ok(())
}

#[test]
fn a_lock_without_a_checksum_stops_generation() -> TestResult {
    let repo = make_repo(&config(DBT))?;
    write_project(repo.path(), false)?;
    let error = prepare(repo.path()).expect_err("unlocked");
    let text = error.to_string();
    assert!(
        text.contains("dbt_fusion_checksum_missing:linux-x64"),
        "{text}"
    );
    Ok(())
}

#[test]
fn project_policy_failures_stop_generation() -> TestResult {
    let cases = [
        ("[tools]\nnode = \"24.20.0\"\n", "dbt_fusion_not_declared"),
        (
            "[tools]\ndbt-fusion = \"2.0.0-beta.1\"\n",
            "project_tool_version_not_exact",
        ),
    ];
    for (mise, code) in cases {
        let repo = make_repo(&config(DBT))?;
        write_project(repo.path(), true)?;
        fs::write(repo.path().join("mise.toml"), mise)?;
        git(&["add", "."], repo.path())?;
        let error = prepare(repo.path()).expect_err("rejected");
        assert!(error.to_string().contains(code), "{code}: {error}");
    }
    let repo = make_repo(&config(DBT))?;
    write_project(repo.path(), true)?;
    fs::remove_file(repo.path().join("warehouse/dbt_project.yml"))?;
    git(&["add", "-A"], repo.path())?;
    let error = prepare(repo.path()).expect_err("no project file");
    assert!(error.to_string().contains("dbt_project_missing"), "{error}");
    Ok(())
}

fn dbt_fixture() -> Result<(tempfile::TempDir, String), Box<dyn std::error::Error>> {
    let (dir, _) = fixture(DBT, false)?;
    let root = dir.path();
    write_project(root, true)?;
    put(root, "other/readme.md", "x\n")?;
    let base = commit(root, "dbt")?;
    Ok((dir, base))
}

fn runs(plan: &Plan) -> BTreeSet<&str> {
    plan.jobs
        .iter()
        .filter(|job| job.job_id.starts_with("dbt-") && job.disposition == JobDisposition::Run)
        .map(|job| job.job_id.as_str())
        .collect()
}

fn pr(edit: impl FnOnce(&Path) -> TestResult) -> Result<Plan, Box<dyn std::error::Error>> {
    let (dir, base) = dbt_fixture()?;
    edit(dir.path())?;
    let head = commit(dir.path(), "change")?;
    Ok(plan_pr(dir.path(), Some(&base), &head)?.0)
}

#[test]
fn an_unrelated_change_skips_the_project_and_a_project_change_runs_it() -> TestResult {
    let plan = pr(|root| put(root, "other/readme.md", "y\n"))?;
    assert!(runs(&plan).is_empty(), "{:?}", plan.jobs);
    assert_eq!(
        plan.jobs
            .iter()
            .filter(|job| job.job_id == "dbt-warehouse")
            .count(),
        1
    );
    let plan = pr(|root| put(root, "warehouse/models/a.sql", "select 2 as id\n"))?;
    assert_eq!(runs(&plan), BTreeSet::from(["dbt-warehouse"]));
    Ok(())
}

#[test]
fn a_tool_declaration_change_and_a_push_run_the_project() -> TestResult {
    let plan = pr(|root| put(root, "mise.lock", &format!("{}\n", lock(true))))?;
    assert_eq!(runs(&plan), BTreeSet::from(["dbt-warehouse"]));
    let (dir, base) = dbt_fixture()?;
    put(dir.path(), "other/readme.md", "z\n")?;
    let head = commit(dir.path(), "change")?;
    let (plan, _) = plan_push(dir.path(), Some(&base), &head)?;
    assert_eq!(runs(&plan), BTreeSet::from(["dbt-warehouse"]));
    Ok(())
}

#[test]
fn service_values_must_match_the_catalog_service() -> TestResult {
    let wrong = DBT.replace("DBT_PORT = \"40000\"", "DBT_PORT = \"5432\"");
    let repo = make_repo(&config(&wrong))?;
    write_project(repo.path(), true)?;
    let error = prepare(repo.path()).expect_err("wrong port");
    assert!(
        error
            .to_string()
            .contains("dbt_env_service_mismatch:DBT_PORT"),
        "{error}"
    );
    Ok(())
}
