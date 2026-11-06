// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn ineligible_preflight_does_not_prepare_or_verify_rust() -> Result<(), Box<dyn std::error::Error>> {
    let (spec, rust) = fixture()?;
    let predicate = "steps.release-admission.outputs.eligible == 'true'";
    let preflight = spec.jobs.get("release-preflight").ok_or("preflight")?;
    for name in ["Checkout exact source", "Acquire cargo-semver-checks", "Release candidate-receipt", "Upload release receipt", "Archive release Rust toolchain", "Upload release Rust toolchain", "Release verify-preflight-retention"] {
        let step = preflight.steps.iter().find(|step| step.name == name).ok_or(name)?;
        assert_eq!(step.condition.as_deref(), Some(predicate), "{name}");
    }
    for name in ["Setup Mise", "Prepare pinned tools", "Verify Rust channel manifest"] {
        let step = preflight.steps.iter().find(|step| step.name == name).ok_or(name)?;
        assert_eq!(step.condition.as_deref(), Some(predicate), "{name}");
        for forged in [None, Some("steps.release-admission.outputs.eligible != 'false'"), Some("steps.other.outputs.eligible == 'true'")] {
            let mut altered = spec.clone();
            let step = altered.jobs.get_mut("release-preflight").ok_or("preflight")?.steps.iter_mut().find(|step| step.name == name).ok_or(name)?;
            step.condition = forged.map(str::to_owned);
            assert!(!valid(&altered, &rust), "accepted altered condition for {name}");
        }
    }
    let reconcile = spec.jobs.get("release-reconcile").ok_or("reconcile")?;
    assert!(reconcile.steps.iter().find(|step| step.name == "Verify Rust channel manifest").ok_or("reconcile verify")?.condition.is_none());
    let preparation = spec.jobs.get("release-preparation").ok_or("preparation")?;
    assert!(preparation.steps.iter().find(|step| step.name == "Prepare pinned tools").ok_or("prepare")?.condition.is_none());
    let condition = preparation.condition.as_deref().ok_or("preparation condition")?;
    assert!(condition.contains("needs.release-preflight.result == 'success'"));
    assert!(condition.contains("needs.release-preflight.outputs.eligible == 'false'"));
    let context = velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(), runs_on: "ubuntu-26.04".to_owned(),
    };
    let rendered = velnor_actions_workflow_renderer::release_tree::render_release_workflow(&spec, &context, &rust)?;
    let yaml: serde_json::Value = serde_saphyr::from_str(&rendered)?;
    let steps = yaml["jobs"]["release-preflight"]["steps"].as_array().ok_or("rendered steps")?;
    for name in ["Setup Mise", "Prepare pinned tools", "Verify Rust channel manifest"] {
        let step = steps.iter().find(|step| step["name"] == name).ok_or(name)?;
        assert_eq!(step["if"].as_str(), Some(predicate), "rendered {name}");
    }
    let (bootstrap, rust) = fixture_for(ReleaseAuthentication::BootstrapToken)?;
    let rendered = velnor_actions_workflow_renderer::release_tree::render_release_workflow(&bootstrap, &context, &rust)?;
    let yaml: serde_json::Value = serde_saphyr::from_str(&rendered)?;
    let steps = yaml["jobs"]["release-preflight"]["steps"].as_array().ok_or("bootstrap steps")?;
    for name in ["Setup Mise", "Prepare pinned tools", "Verify Rust channel manifest"] {
        let step = steps.iter().find(|step| step["name"] == name).ok_or(name)?;
        assert_eq!(step["if"].as_str(), Some(predicate), "bootstrap {name}");
    }
    Ok(())
}
