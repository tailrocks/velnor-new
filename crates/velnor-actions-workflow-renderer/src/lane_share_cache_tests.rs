use velnor_actions_contract::StepKind;

use super::{
    HOSTED_RUNS, ctx, echo_step, paired, render_jobs, scale_token, share_lanes, workflow_ir,
};

#[test]
fn elected_save_stays_on_the_winner_job() {
    let mut jobs = paired(&[echo_step(0, "one")]);
    let expected_checkout = jobs
        .get("rust-0__hosted")
        .expect("hosted")
        .steps
        .first()
        .expect("checkout")
        .clone();
    let save = crate::cache_steps::tools_cache_step(
        false,
        "mise-tools-v2-fixture-${{steps.velnor-tool-cache-identity.outputs.identity}}",
        Some(crate::cache_p08::tools_cache_save_condition()),
    )
    .expect("save");
    jobs.get_mut("rust-0__hosted")
        .expect("hosted")
        .steps
        .push(save);
    let shared = share_lanes(&jobs, &ctx()).expect("share");
    let hosted = shared.jobs.get("rust-0__hosted").expect("hosted");
    let local = shared.jobs.get("rust-0__local").expect("local");
    assert_eq!(
        shared.checkouts.get("rust-0__hosted"),
        Some(&expected_checkout)
    );
    assert_eq!(
        shared.checkouts.get("rust-0__local"),
        Some(&expected_checkout)
    );
    assert_eq!(hosted.steps.len(), 1);
    assert_eq!(hosted.steps.first().expect("save").name, "Save Mise tools");
    assert!(local.steps.is_empty());
    let action = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("composite");
    assert!(!action.bytes.contains("Save Mise tools"));
    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("yaml");
    assert_eq!(yaml.matches("Save Mise tools").count(), 1);
    let checkout_at = yaml.find("name: Checkout").expect("checkout");
    let call_at = yaml.find("uses: ./.github/actions/rust-0").expect("call");
    let save_at = yaml.find("name: Save Mise tools").expect("save");
    assert!(checkout_at < call_at && call_at < save_at);
}

#[test]
fn tools_cache_runtime_preludes_stay_lane_specific_outside_shared_action() {
    use crate::cache_p08::{ToolsCacheInputs, ToolsCachePayload};

    let setup = crate::setup::MiseSetup {
        uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4".to_owned(),
    };
    let specs = ["actionlint@1.7.12".to_owned()];
    let prelude = |runs_on: &str| {
        let payload = ToolsCachePayload::new(ToolsCacheInputs {
            runs_on,
            target: "x86_64-unknown-linux-gnu",
            mise_setup: &setup,
            tool_specs: &specs,
            rustup_toolchain: None,
            rustup_components: &[],
        })?;
        Ok::<_, crate::RenderError>(vec![
            payload.runtime_identity_step()?,
            payload.restore_step()?,
        ])
    };
    let setup_step = crate::setup::mise_setup_step(&setup).expect("cache-disabled setup");
    let mut jobs = paired(&[setup_step, echo_step(0, "same tool work")]);
    let hosted_id = "rust-0__hosted";
    let local_id = "rust-0__local";
    let hosted_prelude = prelude(HOSTED_RUNS).expect("hosted V2 prelude");
    let local_prelude = prelude(&scale_token()).expect("Scale Set V2 prelude");
    for (id, steps) in [(hosted_id, hosted_prelude), (local_id, local_prelude)] {
        jobs.get_mut(id)
            .expect("paired lane")
            .steps
            .splice(1..1, steps);
    }

    let shared = share_lanes(&jobs, &ctx()).expect("lane-specific prelude factors");
    for id in [hosted_id, local_id] {
        let prelude = &shared.runtime_preludes[id];
        assert_eq!(prelude.len(), 2);
        assert_eq!(prelude[0].name, crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME);
        assert_eq!(prelude[1].name, crate::cache_steps::TOOLS_RESTORE_NAME);
    }
    let identity_lane = |id: &str| match &shared.runtime_preludes[id][0].kind {
        StepKind::Shell { env, .. } => env.get("VELNOR_CACHE_LANE").map(String::as_str),
        _ => None,
    };
    assert_eq!(identity_lane(hosted_id), Some(HOSTED_RUNS));
    assert_eq!(identity_lane(local_id), Some(scale_token().as_str()));

    let composite = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("shared action");
    assert!(composite.bytes.contains("Setup Mise"));
    assert!(!composite.bytes.contains("Identify Mise cache runtime"));
    assert!(!composite.bytes.contains("Restore Mise tools"));
    let yaml = render_jobs(&workflow_ir(), &shared, &ctx()).expect("shared workflow");
    let checkout = yaml.find("name: Checkout").expect("checkout");
    let identity = yaml
        .find("name: Identify Mise cache runtime")
        .expect("identity");
    let restore = yaml.find("name: Restore Mise tools").expect("restore");
    let call = yaml.find("uses: ./.github/actions/rust-0").expect("call");
    assert!(checkout < identity && identity < restore && restore < call);
}
