use super::qualification_cache_stage_path;
use crate::cache_action_stage::StageContext;
use std::fs;
use velnor_actions_contract::workflow::QUALIFICATION_CACHE_FORMAT_VERSION;

#[test]
fn qualification_path_matches_the_action_stage_context() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let runner_temp = temp.path().join("runner-temp");
    let workspace = temp.path().join("workspace");
    fs::create_dir_all(&runner_temp).expect("runner temp");
    fs::create_dir_all(&workspace).expect("workspace");
    let campaign = "campaign-1";
    let layer = "tofu_providers";
    let key = format!(
        "velnor-qcache-v{QUALIFICATION_CACHE_FORMAT_VERSION}-{campaign}-{layer}-runtime-k1"
    );

    let helper_path = qualification_cache_stage_path(&runner_temp, campaign, layer, &key)
        .expect("stage helper path");
    let context_path = StageContext::qualification(&runner_temp, &workspace, campaign, layer, &key)
        .expect("action stage context")
        .action_path()
        .to_owned();
    let canonical_runner_temp = fs::canonicalize(&runner_temp).expect("canonical runner temp");

    assert_eq!(helper_path, context_path);
    assert!(helper_path.starts_with(canonical_runner_temp.join("velnor/cache-staging")));
}
