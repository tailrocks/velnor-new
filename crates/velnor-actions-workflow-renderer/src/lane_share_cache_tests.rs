use std::collections::BTreeMap;

use velnor_actions_contract::StepKind;

use super::{HOSTED_RUNS, ctx, echo_step, paired, render_jobs, share_lanes, workflow_ir};

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
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
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
fn only_qualified_hosted_lane_keeps_a_tools_cache_prelude() {
    let setup = crate::setup::MiseSetup {
        uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4".to_owned(),
    };
    let setup_step = crate::setup::mise_setup_step(&setup).expect("cache-disabled setup");
    let mut jobs = paired(&[setup_step, echo_step(0, "same tool work")]);
    let hosted_id = "rust-0__hosted";
    let local_id = "rust-0__local";
    let hosted_prelude = tools_cache_prelude(HOSTED_RUNS).expect("hosted V2 prelude");
    jobs.get_mut(hosted_id)
        .expect("hosted lane")
        .steps
        .splice(1..1, hosted_prelude);

    let wrong_lane = with_wrong_identity_lane(&jobs, hosted_id);
    assert!(share_lanes(&wrong_lane, &ctx()).is_err());

    let shared = share_lanes(&jobs, &ctx()).expect("lane-specific prelude factors");
    let hosted_prelude = &shared.runtime_preludes[hosted_id];
    assert_eq!(hosted_prelude.len(), 2);
    assert_eq!(
        hosted_prelude[0].name,
        crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME
    );
    assert_eq!(
        hosted_prelude[1].name,
        crate::cache_steps::TOOLS_RESTORE_NAME
    );
    assert!(shared.runtime_preludes[local_id].is_empty());
    let identity_uses = match &shared.runtime_preludes[hosted_id][0].kind {
        StepKind::Action { uses, .. } => Some(uses.as_str()),
        _ => None,
    };
    assert_eq!(
        identity_uses,
        crate::cache_p08::runtime_prelude_action_uses(HOSTED_RUNS)
    );
    let restore_uses = match &hosted_prelude[1].kind {
        StepKind::Action { uses, .. } => Some(uses.as_str()),
        _ => None,
    };
    assert_eq!(restore_uses, Some(crate::cache_steps::TOOLS_RESTORE_USES));

    let composite = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("shared action");
    assert!(composite.bytes.contains("Setup Mise"));
    assert!(!composite.bytes.contains("V2 identity"));
    assert!(!composite.bytes.contains("Restore Mise tools"));
    let prelude_action = shared
        .files
        .iter()
        .find(|file| file.path.ends_with("velnor-tools-prelude-u26/action.yml"));
    assert!(
        prelude_action.is_none(),
        "shared files are added by render_merged"
    );
    assert_emitted_restore_is_hosted_only(&shared, hosted_id, local_id);
}

fn tools_cache_prelude(
    runs_on: &str,
) -> Result<Vec<velnor_actions_contract::Step>, crate::RenderError> {
    let setup = crate::setup::MiseSetup {
        uses: "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5".to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4".to_owned(),
    };
    let specs = ["actionlint@1.7.12".to_owned()];
    let payload = crate::cache_p08::ToolsCachePayload::new(crate::cache_p08::ToolsCacheInputs {
        runs_on,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &setup,
        tool_specs: &specs,
        rustup_toolchain: None,
        rustup_components: &[],
    })?;
    Ok(vec![
        payload.runtime_prelude_step()?,
        payload.restore_step()?,
    ])
}

fn assert_emitted_restore_is_hosted_only(
    shared: &crate::lane_share::LaneShare,
    hosted_id: &str,
    local_id: &str,
) {
    let yaml = render_jobs(&workflow_ir(), shared, &ctx()).expect("shared workflow");
    assert_eq!(
        yaml.matches(crate::cache_steps::TOOLS_RESTORE_USES).count(),
        1
    );
    let checkout = yaml.find("name: Checkout").expect("checkout");
    let identity = yaml.find("name: V2 identity").expect("identity");
    let restore = yaml.find("name: Restore Mise tools").expect("restore");
    let shared_id = hosted_id
        .strip_suffix("__hosted")
        .expect("hosted lane suffix");
    let call = yaml
        .find(&format!("uses: ./.github/actions/{shared_id}"))
        .expect("call");
    assert!(checkout < identity && identity < restore && restore < call);
    let local_start = yaml.find(&format!("{local_id}:")).expect("Scale Set job");
    let local = &yaml[local_start..];
    assert!(!local.contains("V2 identity"), "{local}");
    assert!(!local.contains("Restore Mise tools"), "{local}");
}

fn with_wrong_identity_lane(
    jobs: &BTreeMap<String, velnor_actions_contract::Job>,
    hosted_id: &str,
) -> BTreeMap<String, velnor_actions_contract::Job> {
    let mut wrong_lane = jobs.clone();
    let identity = wrong_lane
        .get_mut(hosted_id)
        .expect("hosted lane")
        .steps
        .iter_mut()
        .find(|step| step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity))
        .expect("runtime identity");
    let StepKind::Action { uses, .. } = &mut identity.kind else {
        panic!("identity uses a local composite action");
    };
    *uses = crate::cache_p08::runtime_prelude_action_uses("ubuntu-24.04")
        .expect("supported fixture lane")
        .to_owned();
    wrong_lane
}
