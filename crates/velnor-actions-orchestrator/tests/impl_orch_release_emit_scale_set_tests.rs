use velnor_actions_orchestrator::{ExecutionMode, prepare, render_staged_tree_with};

use crate::impl_common::TestResult;

use super::{SCALE_SET_OIDC_CONFIG, release_repo};

#[test]
fn scale_set_dispatch_routes_every_consumer_release_job() -> TestResult {
    let repo = release_repo(SCALE_SET_OIDC_CONFIG)?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree_with(&prep, Some(ExecutionMode::ScaleSet))?;
    let yaml = tree
        .get(".github/workflows/release.yml")
        .ok_or("release workflow missing")?;
    assert!(
        yaml.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"),
        "{yaml}"
    );
    assert!(!yaml.contains("runs-on: ubuntu-26.04"), "{yaml}");
    Ok(())
}
