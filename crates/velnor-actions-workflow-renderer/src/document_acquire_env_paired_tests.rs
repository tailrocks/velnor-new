use super::*;

#[test]
fn paired_lane_action_keeps_the_literal_tuple_inside_the_registered_acquire_action()
-> Result<(), RenderError> {
    let checkout_uses = "actions/checkout@0000000000000000000000000000000000000000";
    let ctx = context(checkout_uses);
    let selector = ScaleSetSelector::try_new(
        velnor_actions_contract::SCALE_SET_NAME,
        &[
            VELNOR_LABEL.to_owned(),
            velnor_actions_contract::SCALE_SET_NAME.to_owned(),
        ],
    )
    .map_err(RenderError::Contract)?;
    let url = "https://example.invalid/shared";
    let sha = "a".repeat(64);
    let commit = "b".repeat(40);
    let checkout = Step {
        name: "Checkout".to_owned(),
        id: None,
        role: Some(StepRole::Checkout),
        condition: None,
        kind: StepKind::Action {
            uses: checkout_uses.to_owned(),
            with: BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]),
            env: BTreeMap::new(),
        },
    };
    let steps = vec![
        checkout,
        hostile_environment_writer(),
        acquire(url, &sha, &commit)?,
    ];
    let jobs = BTreeMap::from([
        (
            "rust-0__hosted".to_owned(),
            job("Hosted lane", "ubuntu-26.04", steps.clone()),
        ),
        (
            "rust-0__local".to_owned(),
            job("Scale Set lane", &selector.token(), steps),
        ),
    ]);
    let shared = crate::lane_share::share_lanes(&jobs, &ctx)?;
    let rendered =
        crate::document::workflow_to_yaml(&workflow(jobs), &shared, &ctx, &BTreeSet::new())?;
    let workflow_yaml = crate::yaml::render_yaml(&rendered);
    assert!(!workflow_yaml.contains("${{ env.VELNOR_ACQUIRE_"));
    let hosted =
        field(field(&rendered, "jobs").expect("jobs"), "rust-0__hosted").expect("hosted lane");
    let hosted_steps = field(hosted, "steps").expect("hosted steps");
    let shared_call = named_step(hosted_steps, "Run shared steps").expect("lane composite");
    assert!(field(shared_call, "env").is_none());
    assert!(field(shared_call, "with").is_none());
    let lane_file = shared
        .files
        .iter()
        .find(|file| file.path == ".github/actions/rust-0/action.yml")
        .expect("paired lane action");
    assert!(
        lane_file
            .bytes
            .contains("uses: ./.github/actions/acquire-b3-")
    );
    assert!(!lane_file.bytes.contains(url));
    assert!(!lane_file.bytes.contains(sha.as_str()));
    let action = shared
        .files
        .iter()
        .find(|file| file.path.starts_with(".github/actions/acquire-b3-"))
        .expect("one immutable acquire action");
    assert!(action.bytes.contains(url));
    assert!(action.bytes.contains(&sha));
    assert!(action.bytes.contains(&commit));
    assert!(!action.bytes.contains("inputs:"));
    Ok(())
}

#[test]
fn paired_lane_prefix_acquisition_uses_the_registered_immutable_action() -> Result<(), RenderError>
{
    let checkout_uses = "actions/checkout@0000000000000000000000000000000000000000";
    let ctx = context(checkout_uses);
    let selector = ScaleSetSelector::try_new(
        velnor_actions_contract::SCALE_SET_NAME,
        &[
            VELNOR_LABEL.to_owned(),
            velnor_actions_contract::SCALE_SET_NAME.to_owned(),
        ],
    )
    .map_err(RenderError::Contract)?;
    let url = "https://example.invalid/mbx-prefix";
    let sha = "a".repeat(64);
    let commit = "b".repeat(40);
    let mut steps = vec![
        Step {
            name: "Checkout".to_owned(),
            id: None,
            role: Some(StepRole::Checkout),
            condition: None,
            kind: StepKind::Action {
                uses: checkout_uses.to_owned(),
                with: BTreeMap::from([("persist-credentials".to_owned(), "false".to_owned())]),
                env: BTreeMap::new(),
            },
        },
        hostile_environment_writer(),
        acquire(url, &sha, &commit)?,
    ];
    steps.extend(mbx_steps()?);
    steps.push(Step {
        name: "Run check".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["echo".to_owned(), "check".to_owned()],
            env: BTreeMap::new(),
        },
    });
    let jobs = BTreeMap::from([
        (
            "rust-0__hosted".to_owned(),
            job("Hosted lane", "ubuntu-26.04", steps.clone()),
        ),
        (
            "rust-0__local".to_owned(),
            job("Scale Set lane", &selector.token(), steps),
        ),
    ]);
    let shared = crate::lane_share::share_lanes(&jobs, &ctx)?;
    let prefix = shared
        .prefixes
        .get("rust-0__hosted")
        .expect("hosted prefix");
    let acquire_call = prefix
        .iter()
        .find(|step| step.name == steps::ACQUIRE_NAME)
        .expect("acquire remains before MBX prelude");
    assert!(matches!(
        &acquire_call.kind,
        StepKind::Action { uses, with, env }
            if crate::acquire_action::is_acquire_action_uses(uses)
                && with.is_empty()
                && env.is_empty()
    ));
    let rendered =
        crate::document::workflow_to_yaml(&workflow(jobs), &shared, &ctx, &BTreeSet::new())?;
    assert_rendered_acquire_action(&shared, &rendered, "rust-0__hosted", url, &sha, &commit);
    Ok(())
}
