// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn preparation_waits_for_negative_trusted_release_admission() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let preparation = spec.jobs.get("release-preparation").ok_or("preparation")?;
    assert_eq!(preparation.needs, ["release-preflight"]);
    let condition = preparation.condition.as_deref().ok_or("condition")?;
    assert!(condition.contains("needs.release-preflight.result == 'success'"));
    assert!(condition.contains("needs.release-preflight.outputs.eligible == 'false'"));
    for bad_condition in [
        condition.replace("needs.release-preflight.result == 'success'", "true"),
        condition.replace("needs.release-preflight.outputs.eligible == 'false'", "needs.release-preflight.outputs.eligible != 'true'"),
        condition.replace("needs.release-preflight.outputs.eligible == 'false'", "true"),
    ] {
        let mut forged = spec.clone();
        forged.jobs.get_mut("release-preparation").ok_or("preparation")?.condition = Some(bad_condition);
        assert!(!valid(&forged, &rust));
    }
    Ok(())
}

#[test]
fn crate_release_checkouts_use_attested_file_free_git() -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_workflow_renderer::release_jobs::ReleaseRole;
    for authentication in [ReleaseAuthentication::TrustedPublishing, ReleaseAuthentication::BootstrapToken] {
        for release_pr in [true, false] {
            let (spec, rust) = fixture_for_release_pr(authentication, release_pr)?;
            assert!(valid(&spec, &rust));
            for job in spec.jobs.values().filter(|job| job.role != ReleaseRole::PrCreate) {
                assert_eq!(job.steps[0].name, "Acquire Velnor");
                assert_eq!(job.steps[1].name, "Checkout");
                let StepKind::Shell { run, env } = &job.steps[1].kind else { return Err("policy checkout is not Velnor".into()); };
                assert_eq!(run[2], "checkout-policy");
                assert_eq!(env.get("GIT_TOKEN").map(String::as_str), Some("${{ secrets.GITHUB_TOKEN }}"));
                assert!(!job.steps.iter().any(|step| matches!(&step.kind, StepKind::Action { uses, .. } if uses.starts_with("actions/checkout@"))));
                if job.role != ReleaseRole::Preparation {
                    let source = job.steps.iter().find(|step| step.name == "Checkout exact source").ok_or("source checkout")?;
                    let StepKind::Shell { run, env } = &source.kind else { return Err("source checkout is not Velnor".into()); };
                    assert_eq!(run[2], "checkout-source");
                    assert_eq!(env.get("VELNOR_SOURCE_SHA").map(String::as_str), Some("${{ steps.release-admission.outputs.source_sha }}"));
                    assert_eq!(env.get("GIT_TOKEN").map(String::as_str), Some("${{ secrets.GITHUB_TOKEN }}"));
                }
            }
            let mut forged = spec.clone();
            let step = &mut forged.jobs.get_mut("release-preflight").ok_or("preflight")?.steps[3];
            let StepKind::Shell { env, .. } = &mut step.kind else { return Err("source checkout is not Velnor".into()); };
            env.insert("VELNOR_SOURCE_SHA".into(), "${{ github.sha }}".into());
            assert!(!valid(&forged, &rust));
        }
    }
    Ok(())
}
