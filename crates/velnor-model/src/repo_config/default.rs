use std::collections::BTreeMap;

use super::{
    CacheDebug, CheckoutScope, CiDebugInfo, DocsRun, FullRun, MbxRemote, MbxStore, OnMain, PrCi,
    PushAfterMergeGroup, RustCiConfig,
};
use crate::hosted::HostedRunners;
use crate::ids::RunnerLabel;
use crate::mbx::MbxMode;
use crate::trusted::TrustedRunners;
impl Default for RustCiConfig {
    fn default() -> Self {
        Self {
            runner: RunnerLabel::from_static("ubuntu-latest"),
            fork_runner: RunnerLabel::from_static("ubuntu-latest"),
            hosted_runners: HostedRunners::new(vec![RunnerLabel::from_static("ubuntu-latest")]),
            trusted_runners: TrustedRunners::default(),
            crates_mirror: None,
            private_sources: None,
            public_sources: None,
            mbx_store: MbxStore::Private,
            docs_run: DocsRun::Always,
            check_alone_run: DocsRun::Always,
            timeout_minutes: 30,
            root_timeouts: BTreeMap::new(),
            exclude: Vec::new(),
            non_inputs: Vec::new(),
            inputs: Vec::new(),
            default_ignores: true,
            shape: None,
            required_check: "Required".to_owned(),
            nextest_harnesses: Vec::new(),
            default_branch: "main".to_owned(),
            skip_draft_pull_requests: false,
            pr_ci: PrCi::Always,
            on_main: OnMain::Affected,
            full_run: FullRun::Nightly,
            helper: None,
            ci_debuginfo: CiDebugInfo::LineTablesOnly,
            cache_budget_bytes: 8_000_000_000,
            cache_check_blocks: false,
            cache_debug: CacheDebug::Off,
            mbx_remote: MbxRemote::Off,
            mbx: MbxMode::Default,
            push_after_merge_group: PushAfterMergeGroup::Run,
            test_setup: Vec::new(),
            check_configurations: Vec::new(),
            test_lanes: Vec::new(),
            budget: None,
            lint: Vec::new(),
            test_groups: Vec::new(),
            checkout: CheckoutScope::Full,
            workspace_groups: Vec::new(),
            policy: None,
            external_workflows: None,
            release_assets: Vec::new(),
        }
    }
}
