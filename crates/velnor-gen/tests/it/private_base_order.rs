// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::{TestResult, path};
use crate::gate_eval::{Context, runs};
use crate::pipeline::workflow;
use crate::private_base_caches::GIT_LOCK;
use crate::private_sources::{SECTION, private_repo};

struct Run {
    event: &'static str,
    actor: &'static str,
    head_repo: &'static str,
    private: &'static str,
    allowed: &'static str,
    restores: bool,
}

const REPO: &str = "acme/app";
const FORK: &str = "someone/app";

const SCENARIOS: [Run; 12] = [
    Run {
        event: "push",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "",
        restores: false,
    },
    Run {
        event: "push",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: true,
    },
    Run {
        event: "schedule",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: true,
    },
    Run {
        event: "pull_request",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: true,
    },
    Run {
        event: "pull_request",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "",
        restores: false,
    },
    Run {
        event: "pull_request",
        actor: "donbeave",
        head_repo: FORK,
        private: "true",
        allowed: "",
        restores: false,
    },
    Run {
        event: "pull_request",
        actor: "dependabot[bot]",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "true",
        restores: true,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "false",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "true",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "false",
        allowed: "false",
        restores: false,
    },
    Run {
        event: "merge_group",
        actor: "donbeave",
        head_repo: REPO,
        private: "true",
        allowed: "",
        restores: false,
    },
];

#[test]
fn every_private_base_restore_follows_the_merge_queue_check_and_admits_a_trusted_merge_group()
-> TestResult {
    for (roots, shape) in [
        (&["Cargo.toml"][..], ""),
        (&["Cargo.toml", "tools/Cargo.toml"][..], ""),
        (
            &["Cargo.toml", "tools/Cargo.toml"][..],
            "shape = 'groups'\n",
        ),
    ] {
        let case = format!("{roots:?} {shape}");
        let mut repo = private_repo(roots, &format!("{shape}{SECTION}"), "0.1.111")?;
        for root in roots {
            repo.files.insert(
                path(root)?.sibling(&path("Cargo.lock")?),
                GIT_LOCK.as_bytes().to_vec(),
            );
        }
        let text = workflow(&repo)?;
        let mut restores = 0;
        for job in crate::private_sources::jobs(&text)? {
            let steps = job["steps"].as_array().ok_or("steps missing")?;
            let position = |id: &str| steps.iter().position(|s| s["id"] == id);
            for (index, step) in steps.iter().enumerate() {
                let gated = step["name"].as_str().is_some_and(|name| {
                    ["Restore the sources cache", "Restore the target cache"].contains(&name)
                });
                if !gated {
                    continue;
                }
                restores += 1;
                let check = position("private-check").ok_or_else(|| format!("{case}:{job}"))?;
                let repository = position("private_repo").ok_or_else(|| format!("{case}:{job}"))?;
                assert!(check < index && repository < index, "{case}:{step}");
                let condition = step["if"]
                    .as_str()
                    .and_then(|text| text.strip_prefix("${{ ")?.strip_suffix(" }}"))
                    .ok_or("restore has no condition")?;
                for scenario in &SCENARIOS {
                    let mut context = Context::new(&[
                        ("github.event_name", scenario.event),
                        ("github.actor", scenario.actor),
                        ("github.repository", REPO),
                        (
                            "github.event.pull_request.head.repo.full_name",
                            scenario.head_repo,
                        ),
                        ("github.event.pull_request.user.login", scenario.actor),
                        ("steps.private_repo.outputs.private", scenario.private),
                        ("steps.private-check.outputs.allowed", scenario.allowed),
                        ("steps.plan.outcome", "success"),
                        ("steps.plan.outputs.rust_root_mode", "full"),
                    ]);
                    context.job_ok = true;
                    assert_eq!(
                        runs(condition, &context)?,
                        scenario.restores,
                        "{case}:{}:{}:{}",
                        scenario.event,
                        scenario.private,
                        scenario.allowed
                    );
                }
            }
        }
        assert!(restores > 0, "{case}:no base cache restore");
    }
    Ok(())
}
