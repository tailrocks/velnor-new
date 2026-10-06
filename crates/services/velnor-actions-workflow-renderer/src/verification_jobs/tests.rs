use velnor_actions_contract::{VerificationRunner, VerificationTask, VerificationTaskKind};

use super::VerificationTaskPolicy;
use crate::MiseSetup;

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";

fn policy(id: &str, runner: VerificationRunner) -> VerificationTaskPolicy {
    VerificationTaskPolicy {
        task: VerificationTask {
            id: id.to_owned(),
            kind: VerificationTaskKind::Verification,
            mise_task: format!("lint-{id}"),
            runner,
            timeout_minutes: 10,
        },
        runner_label: runner.runs_on().to_owned(),
        scale_set_token: Some("scale-set:velnor+ubuntu-26.04-scale-set".to_owned()),
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            version: "2026.9.18".to_owned(),
            sha256: "a".repeat(64),
        },
    }
}

mod tasks;
