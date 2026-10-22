//! The Bun assets job (HD-7 for suites): one job holds the App-token block.

use std::collections::BTreeMap;

use velnor_actions_contract::{JobTimeout, Step};
use velnor_actions_workflow_renderer::private_source_steps::{
    PrivateSourcesSpec, private_source_steps,
};
use velnor_actions_workflow_renderer::verification_jobs::{
    BunAssetsPolicy, BunSuitePolicy, build_bun_assets_job, build_bun_suite_job,
    bun_assets_download_step, bun_assets_upload_step,
};
use velnor_actions_workflow_renderer::{RenderError, shell_step};

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

fn step(name: &str, parallel: bool) -> Result<Step, RenderError> {
    let mut step = shell_step(name, vec!["true".to_owned()], BTreeMap::new())?;
    step.parallel = parallel;
    Ok(step)
}

fn block() -> Result<Vec<Step>, RenderError> {
    let repositories = ["ui-assets".to_owned()];
    let roots = [".".to_owned()];
    let env = BTreeMap::new();
    Ok(private_source_steps(&PrivateSourcesSpec {
        owner: "ChainArgos",
        app_id_variable: "VELNOR_DEPS_APP_ID",
        app_private_key_secret: "VELNOR_DEPS_APP_PRIVATE_KEY",
        repositories: &repositories,
        kinds: &["release-assets"],
        roots: &roots,
        toolchain_env: &env,
    })?
    .into())
}

fn suite(steps: Vec<Step>) -> BunSuitePolicy {
    BunSuitePolicy {
        member: "@chainargos/ui".to_owned(),
        suite: "unit".to_owned(),
        id: "bun-chainargos-ui--unit".to_owned(),
        timeout: JobTimeout::DEFAULT,
        required: true,
        steps,
        matrix: None,
        report: None,
        assets: None,
    }
}

#[test]
fn an_assets_job_holds_the_block_and_a_suite_needs_it() -> Result<(), RenderError> {
    let mut steps = block()?;
    steps.push(bun_assets_upload_step("bun-chainargos-ui")?);
    let assets = BunAssetsPolicy {
        member: "@chainargos/ui".to_owned(),
        id: "bun-chainargos-ui--assets".to_owned(),
        timeout: JobTimeout::DEFAULT,
        steps,
    };
    let job = build_bun_assets_job(&assets, CHECKOUT)?;
    assert_eq!(job.display_name, "Bun / @chainargos/ui / assets");
    assert_eq!(job.needs, ["plan"]);
    let no_fetch = BunAssetsPolicy {
        steps: vec![step("Index", false)?],
        ..assets.clone()
    };
    let error = build_bun_assets_job(&no_fetch, CHECKOUT).expect_err("no fetch");
    assert!(
        error.to_string().contains("bun_assets_without_fetch"),
        "{error}"
    );
    let mut with_assets = suite(vec![
        bun_assets_download_step("bun-chainargos-ui")?,
        step("Test", true)?,
    ]);
    with_assets.assets = Some("bun-chainargos-ui--assets".to_owned());
    let job = build_bun_suite_job(&with_assets, CHECKOUT)?;
    assert_eq!(job.needs, ["plan", "bun-chainargos-ui--assets"]);
    with_assets.assets = Some("bun-other--assets".to_owned());
    let error = build_bun_suite_job(&with_assets, CHECKOUT).expect_err("foreign assets");
    assert!(
        error.to_string().contains("bun_suite_assets_job"),
        "{error}"
    );
    Ok(())
}
