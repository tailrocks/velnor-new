//! Declared Bun suites through the real generator (hard decision 6 amendment).

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_bun::{MISE, bun_config, release_asset_repo, write_workspace};
use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

fn generate(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    write_workspace(repo.path(), MISE)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
}

fn failure(config: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    write_workspace(repo.path(), MISE)?;
    Ok(prepare(repo.path())
        .err()
        .ok_or("configuration accepted")?
        .to_string())
}

fn timeout_of(body: &str) -> Option<&str> {
    body.lines()
        .find_map(|line| line.trim().strip_prefix("timeout-minutes: "))
}

fn has_job(ci: &str, id: &str) -> bool {
    ci.contains(&format!("\n  {id}:\n"))
}

#[test]
fn a_suite_runs_its_scripts_in_its_own_job_beside_the_member_job() -> TestResult {
    let ci = generate(&bun_config(
        "[stacks.bun.members.\"chainargos-docs\".suites.unit]\nscripts = [\"test\"]\n",
    ))?;
    let member = job_body(&ci, "bun-chainargos-docs")?;
    assert!(member.contains("name: Typecheck\n"), "{member}");
    assert!(member.contains("name: Build\n"), "{member}");
    assert!(!member.contains("name: Test\n"), "claimed: {member}");
    let suite = job_body(&ci, "bun-chainargos-docs--unit")?;
    assert!(
        suite.contains("name: Bun / chainargos-docs / unit\n"),
        "{suite}"
    );
    assert!(suite.contains("runs-on: ubuntu-26.04\n"), "{suite}");
    assert!(suite.contains("- plan\n"), "{suite}");
    assert!(
        suite.contains("',bun-chainargos-docs--unit,'"),
        "own plan gate: {suite}"
    );
    assert!(suite.contains("contents: read"), "{suite}");
    assert!(!suite.contains("packages:"), "{suite}");
    assert!(suite.contains("persist-credentials: \"false\""), "{suite}");
    assert!(suite.contains("--frozen-lockfile"), "{suite}");
    assert!(suite.contains("name: Test\n"), "{suite}");
    assert!(suite.contains("--cwd=frontend/docs"), "{suite}");
    assert!(!suite.contains("name: Typecheck\n"), "{suite}");
    assert!(!suite.contains("VELNOR_NPM_READ_TOKEN"), "{suite}");
    let required = job_body(&ci, "required")?;
    assert!(required.contains("- bun-chainargos-docs\n"), "{required}");
    assert!(
        required.contains("- bun-chainargos-docs--unit\n"),
        "{required}"
    );
    assert!(required.contains("- bun-chainargos-ui\n"), "{required}");
    Ok(())
}

#[test]
fn the_member_job_is_omitted_when_suites_claim_every_script() -> TestResult {
    let ci = generate(&bun_config(
        "[stacks.bun.members.\"@chainargos/ui\"]\nscripts = [\"test\"]\n\
[stacks.bun.members.\"@chainargos/ui\".suites.unit]\nscripts = [\"test\"]\n",
    ))?;
    assert!(!has_job(&ci, "bun-chainargos-ui"), "{ci}");
    assert!(has_job(&ci, "bun-chainargos-ui--unit"), "{ci}");
    let required = job_body(&ci, "required")?;
    assert!(!required.contains("- bun-chainargos-ui\n"), "{required}");
    assert!(
        required.contains("- bun-chainargos-ui--unit\n"),
        "{required}"
    );
    Ok(())
}

#[test]
fn a_non_gating_suite_runs_outside_required() -> TestResult {
    let ci = generate(&bun_config(
        "[stacks.bun.members.\"chainargos-docs\".suites.smoke]\nscripts = [\"build\"]\nrequired = false\n",
    ))?;
    assert!(has_job(&ci, "bun-chainargos-docs--smoke"), "{ci}");
    let required = job_body(&ci, "required")?;
    assert!(
        !required.contains("bun-chainargos-docs--smoke"),
        "{required}"
    );
    assert!(required.contains("- bun-chainargos-docs\n"), "{required}");
    Ok(())
}

#[test]
fn suite_timeouts_fall_back_to_member_then_stack_then_crate() -> TestResult {
    let ci = generate(&bun_config(
        "timeout_minutes = 30\n\
[stacks.bun.members.\"chainargos-docs\"]\ntimeout_minutes = 40\n\
[stacks.bun.members.\"chainargos-docs\".suites.unit]\nscripts = [\"test\"]\ntimeout_minutes = 50\n\
[stacks.bun.members.\"chainargos-docs\".suites.build]\nscripts = [\"build\"]\n\
[stacks.bun.members.\"@chainargos/ui\".suites.lint]\nscripts = [\"lint\"]\n",
    ))?;
    let minutes = |id: &str| -> Result<String, Box<dyn std::error::Error>> {
        Ok(timeout_of(job_body(&ci, id)?).ok_or("timeout")?.to_owned())
    };
    assert_eq!(minutes("bun-chainargos-docs")?, "40");
    assert_eq!(minutes("bun-chainargos-docs--unit")?, "50");
    assert_eq!(minutes("bun-chainargos-docs--build")?, "40");
    assert_eq!(minutes("bun-chainargos-ui--lint")?, "30");
    assert_eq!(minutes("bun-chainargos-ui")?, "30");
    let crate_only = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
crate_timeout_minutes = 12\n[stacks.bun]\nroots = [\"frontend\"]\n\
[stacks.bun.members.\"@chainargos/ui\".suites.lint]\nscripts = [\"lint\"]\n";
    let ci = generate(crate_only)?;
    assert_eq!(
        timeout_of(job_body(&ci, "bun-chainargos-ui--lint")?),
        Some("12")
    );
    Ok(())
}

#[test]
fn suite_configuration_errors_stop_generation() -> TestResult {
    let missing = failure(&bun_config(
        "[stacks.bun.members.\"chainargos-docs\".suites.unit]\nscripts = [\"nope\"]\n",
    ))?;
    assert!(
        missing.contains("bun_script_missing:chainargos-docs:nope"),
        "{missing}"
    );
    let twice = failure(&bun_config(
        "[stacks.bun.members.\"chainargos-docs\".suites.a]\nscripts = [\"test\"]\n\
[stacks.bun.members.\"chainargos-docs\".suites.b]\nscripts = [\"test\"]\n",
    ))?;
    assert!(twice.contains("suite_script_claimed_twice:test"), "{twice}");
    let reserved = failure(&bun_config(
        "[stacks.bun.members.\"chainargos-docs\".suites.report]\nscripts = [\"test\"]\n",
    ))?;
    assert!(reserved.contains("reserved_suite_id:report"), "{reserved}");
    Ok(())
}

#[test]
fn one_assets_job_fetches_release_assets_and_suites_hold_no_secret() -> TestResult {
    let repo = release_asset_repo(
        "[stacks.bun.members.\"chainargos-docs\".suites.unit]\nscripts = [\"test\"]\n",
    )?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    assert_asset_job(ci)?;
    assert_suite_asset_handoff(ci)?;
    let member = job_body(ci, "bun-chainargos-docs")?;
    assert!(
        member.contains("name: Fetch private sources"),
        "member job keeps HD-7: {member}"
    );
    let required = job_body(ci, "required")?;
    assert!(
        required.contains("- bun-chainargos-docs--unit\n"),
        "{required}"
    );
    assert!(!required.contains("--assets"), "{required}");
    Ok(())
}

fn assert_asset_job(ci: &str) -> TestResult {
    let assets = job_body(ci, "bun-chainargos-docs--assets")?;
    assert!(
        assets.contains("name: Bun / chainargos-docs / assets"),
        "{assets}"
    );
    let position = |needle: &str| {
        assets
            .find(needle)
            .ok_or(format!("missing {needle}: {assets}"))
    };
    let mint = position("name: Mint read-only token for private sources")?;
    let fetch = position("name: Fetch private sources")?;
    let revoke = position("name: Revoke private-source token")?;
    let bundle = position("name: Bundle verified private assets")?;
    let upload = position("name: Upload private assets")?;
    assert!(
        mint < fetch && fetch < revoke && revoke < bundle && bundle < upload,
        "{assets}"
    );
    assert!(
        assets.contains("path: ${{ runner.temp }}/velnor/bun-assets/bun-chainargos-docs/velnor-pw-assets-bun-chainargos-docs.tar.gz"),
        "{assets}"
    );
    assert!(assets.contains("archive: \"false\""), "{assets}");
    assert!(
        !assets.contains("bun install"),
        "the assets job runs no repository code"
    );
    Ok(())
}

fn assert_suite_asset_handoff(ci: &str) -> TestResult {
    let suite = job_body(ci, "bun-chainargos-docs--unit")?;
    assert!(
        suite.contains("- plan\n      - bun-chainargos-docs--assets\n"),
        "{suite}"
    );
    for secret in [
        "deps-token",
        "VELNOR_DEPS_APP_PRIVATE_KEY",
        "Mint read-only token",
    ] {
        assert!(!suite.contains(secret), "{secret} in a suite job: {suite}");
    }
    let prepare = suite
        .find("name: Prepare private asset handoff")
        .ok_or("prepare")?;
    let download = suite
        .find("name: Download private assets")
        .ok_or("download")?;
    let download_body = &suite[download..];
    assert!(
        download_body.contains("name: velnor-pw-assets-bun-chainargos-docs.tar.gz"),
        "{download_body}"
    );
    assert!(
        download_body.contains("digest-mismatch: error"),
        "{download_body}"
    );
    let verify = suite
        .find("name: Import and verify private assets")
        .ok_or("verify")?;
    let install = suite.find("name: Install dependencies").ok_or("install")?;
    assert!(
        prepare < download && download < verify && verify < install,
        "{suite}"
    );
    assert!(
        suite.contains(&format!(
            "VELNOR_BUN_ASSET_PINS: \"ui-assets:{}\"",
            "0123456789abcdef".repeat(4)
        )) || suite.contains(&format!(
            "VELNOR_BUN_ASSET_PINS: ui-assets:{}",
            "0123456789abcdef".repeat(4)
        )),
        "{suite}"
    );
    assert!(suite.contains("bun-assets-import-v1"), "{suite}");
    Ok(())
}

#[test]
fn suite_generation_is_deterministic() -> TestResult {
    let config = bun_config(
        "[stacks.bun.members.\"chainargos-docs\".suites.unit]\nscripts = [\"test\"]\n\
[stacks.bun.members.\"chainargos-docs\".suites.build]\nscripts = [\"build\"]\n",
    );
    assert_eq!(generate(&config)?, generate(&config)?);
    Ok(())
}
