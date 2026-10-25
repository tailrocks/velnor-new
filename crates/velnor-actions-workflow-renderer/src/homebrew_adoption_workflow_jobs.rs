//! Native planner, report transport, and Required verdict jobs for adoption.

use std::collections::BTreeMap;

use crate::{
    RenderError,
    schema2::ProductReleasePins,
    steps::{self, CHECKOUT_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES},
    yaml::Yaml,
};

use super::{
    PLAN_ARTIFACT, RUN_DIR_EXPR, RUNNER,
    bootstrap::{bootstrap_script, direct_run},
};

pub(super) fn plan_job(
    common_env: &Yaml,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let mut steps = bootstrap_steps("plan", pins)?;
    steps.push(checkout_head_step("plan")?);
    steps.extend([
        internal_step(
            "Write request",
            "write-request-v1:plan-v1",
            Some("${{ runner.temp }}/velnor/request/plan-v1-request.json"),
            &[],
            None,
            None,
        )?,
        internal_step(
            "Plan",
            "plan-v1",
            Some("${{ runner.temp }}/velnor/request/plan-v1-request.json"),
            &[("GH_TOKEN", "${{ github.token }}")],
            Some(("id", Yaml::str("plan"))),
            Some((
                velnor_actions_contract::PLAN_MATRIX_OUTPUT_MODE_ENV,
                velnor_actions_contract::DYNAMIC_MATRIX_OUTPUT_MODE,
            )),
        )?,
        action_step(
            "Publish plan",
            UPLOAD_ARTIFACT_USES,
            vec![
                ("if-no-files-found", "error"),
                ("name", PLAN_ARTIFACT),
                ("path", RUN_DIR_EXPR),
                ("retention-days", "30"),
            ],
            true,
        ),
    ]);
    let fields = vec![
        ("name", Yaml::str("Plan Homebrew adoption")),
        ("runs-on", Yaml::str(RUNNER)),
        ("timeout-minutes", Yaml::Int(5)),
        ("permissions", readonly_permissions()),
        ("env", common_env.clone()),
        (
            "outputs",
            Yaml::Map(vec![
                (
                    "matrix".to_owned(),
                    Yaml::str("${{ steps.plan.outputs.matrix }}"),
                ),
                (
                    "plan_id".to_owned(),
                    Yaml::str("${{ steps.plan.outputs.plan_id }}"),
                ),
                (
                    "plan_attempt".to_owned(),
                    Yaml::str("${{ github.run_attempt }}"),
                ),
            ]),
        ),
        ("steps", Yaml::Seq(steps)),
    ];
    Ok(("plan".to_owned(), map(fields)))
}

pub(super) fn native_job(
    common_env: &Yaml,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let mut steps = bootstrap_steps("native-checks", pins)?;
    steps.push(checkout_head_step("native-checks")?);
    steps.push(action_step(
        "Download plan",
        DOWNLOAD_ARTIFACT_USES,
        vec![("name", PLAN_ARTIFACT), ("path", RUN_DIR_EXPR)],
        true,
    ));
    steps.push(internal_step(
        "Execute native Homebrew check",
        "execute-native-homebrew-check-v1",
        None,
        &[
            ("VELNOR_CHECK_ID", "${{ matrix.adapter_metadata.check_id }}"),
            (
                "VELNOR_NATIVE_CHECK_OPERATION",
                "${{ matrix.adapter_metadata.operation }}",
            ),
            ("VELNOR_TASK_ID", "${{ matrix.task_id }}"),
            ("GH_TOKEN", "${{ github.token }}"),
        ],
        None,
        None,
    )?);
    steps.push(action_step(
        "Upload matrix report",
        UPLOAD_ARTIFACT_USES,
        vec![
            ("name", "${{ matrix.artifact_id }}"),
            ("path", RUN_DIR_EXPR),
            ("if-no-files-found", "error"),
            ("overwrite", "true"),
            ("retention-days", "30"),
        ],
        true,
    ));
    let fields = vec![
        ("name", Yaml::str("Execute native Homebrew checks")),
        ("runs-on", Yaml::str(RUNNER)),
        ("timeout-minutes", Yaml::Int(5)),
        ("permissions", readonly_permissions()),
        ("env", plan_consumer_env(common_env)),
        ("needs", Yaml::Seq(vec![Yaml::str("plan")])),
        (
            "strategy",
            Yaml::Map(vec![
                ("fail-fast".to_owned(), Yaml::Bool(false)),
                ("max-parallel".to_owned(), Yaml::Int(2)),
                (
                    "matrix".to_owned(),
                    Yaml::str("${{ fromJSON(needs.plan.outputs.matrix) }}"),
                ),
            ]),
        ),
        ("steps", Yaml::Seq(steps)),
    ];
    Ok(("native-checks".to_owned(), map(fields)))
}

pub(super) fn required_job(
    common_env: &Yaml,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    let mut steps = bootstrap_steps("required", pins)?;
    steps.extend([
        checkout_head_step("required")?,
        action_step(
            "Download plan",
            DOWNLOAD_ARTIFACT_USES,
            vec![("name", PLAN_ARTIFACT), ("path", RUN_DIR_EXPR)],
            true,
        ),
        fetch_reports_step(pins)?,
        internal_step(
            "Write request",
            "write-request-v1:merge-v1",
            Some("${{ runner.temp }}/velnor/request/merge-v1-request.json"),
            &[
                ("VELNOR_NEEDS_JSON", "${{ toJSON(needs) }}"),
                ("VELNOR_NEEDS_EXPECTED", "[\"native-checks\",\"plan\"]"),
            ],
            None,
            None,
        )?,
        internal_step(
            "Merge reports",
            "merge-v1",
            Some("${{ runner.temp }}/velnor/request/merge-v1-request.json"),
            &[
                ("VELNOR_NEEDS_JSON", "${{ toJSON(needs) }}"),
                ("VELNOR_NEEDS_EXPECTED", "[\"native-checks\",\"plan\"]"),
                ("GH_TOKEN", "${{ github.token }}"),
            ],
            None,
            None,
        )?,
    ]);
    let final_report = crate::final_steps::publish_final_report_step()?;
    steps.push(crate::document_steps::non_internal_step_to_yaml(
        "required",
        &final_report,
        &BTreeMap::new(),
        RUNNER,
    )?);
    let fields = vec![
        ("name", Yaml::str("Native check verdict")),
        ("runs-on", Yaml::str(RUNNER)),
        ("timeout-minutes", Yaml::Int(5)),
        ("permissions", readonly_permissions()),
        ("env", plan_consumer_env(common_env)),
        (
            "needs",
            Yaml::Seq(vec![Yaml::str("plan"), Yaml::str("native-checks")]),
        ),
        ("if", Yaml::str(velnor_actions_contract::REQUIRED_CONDITION)),
        ("steps", Yaml::Seq(steps)),
    ];
    Ok((
        velnor_actions_contract::REQUIRED_JOB_ID.to_owned(),
        map(fields),
    ))
}

fn bootstrap_steps(job: &str, pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let setup = crate::mise_setup_step(&pins.linux_x86_64_setup)?;
    let setup_yaml =
        crate::document_steps::non_internal_step_to_yaml(job, &setup, &BTreeMap::new(), RUNNER)?;
    let script = bootstrap_script();
    let env = BTreeMap::from([
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        (
            "VELNOR_ADOPTION_GH".to_owned(),
            crate::commands::join_argv_for_run(&pins.gh_argv)?,
        ),
    ]);
    Ok(vec![
        setup_yaml,
        direct_run(
            "Install pinned GitHub CLI",
            &crate::commands::join_argv_for_run(&pins.install_gh_argv)?,
            BTreeMap::new(),
        ),
        direct_run("Authenticate and stage published SDK binary", &script, env),
    ])
}

fn checkout_head_step(job_id: &str) -> Result<Yaml, RenderError> {
    let step = steps::checkout_step(CHECKOUT_USES)?;
    let mut rendered =
        crate::document_steps::non_internal_step_to_yaml(job_id, &step, &BTreeMap::new(), RUNNER)?;
    if let Yaml::Map(fields) = &mut rendered
        && let Some((_, Yaml::Map(with))) = fields.iter_mut().find(|(key, _)| key == "with")
    {
        with.push((
            "ref".to_owned(),
            Yaml::str("${{ github.event.pull_request.head.sha }}"),
        ));
        with.push(("submodules".to_owned(), Yaml::Bool(false)));
        with.push(("fetch-depth".to_owned(), Yaml::Int(0)));
    }
    Ok(rendered)
}

fn internal_step(
    name: &str,
    operation: &str,
    request_file: Option<&str>,
    extra_env: &[(&str, &str)],
    id: Option<(&str, Yaml)>,
    mode: Option<(&str, &str)>,
) -> Result<Yaml, RenderError> {
    let binary = "\"$RUNNER_TEMP/velnor/bin/velnor-actions-$VELNOR_HOMEBREW_ADOPTION_VERSION\"";
    let (base_operation, target) = if operation == "execute-native-homebrew-check-v1" {
        (operation, operation)
    } else {
        steps::split_internal_operation(operation)?
    };
    if let Some(request_file) = request_file {
        let expected = format!("${{{{ runner.temp }}}}/velnor/request/{target}-request.json");
        if request_file != expected {
            return Err(RenderError::InvalidWorkflow(
                "adoption_request_path".to_owned(),
            ));
        }
    } else if matches!(
        base_operation,
        steps::WRITE_REQUEST_OPERATION | steps::PLAN_OPERATION | steps::MERGE_OPERATION
    ) {
        return Err(RenderError::InvalidWorkflow(
            "adoption_request_missing".to_owned(),
        ));
    }
    let mut env = vec![(steps::INTERNAL_OP_ENV.to_owned(), Yaml::str(base_operation))];
    if let Some(request_file) = request_file {
        env.push(("VELNOR_REQUEST_FILE".to_owned(), Yaml::str(request_file)));
    }
    env.extend(
        extra_env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value))),
    );
    if let Some((key, value)) = mode {
        env.push((key.to_owned(), Yaml::str(value)));
    }
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if let Some((key, value)) = id {
        fields.push((key.to_owned(), value));
    }
    fields.extend([
        ("env".to_owned(), Yaml::Map(env)),
        ("run".to_owned(), Yaml::str(binary)),
    ]);
    Ok(Yaml::Map(fields))
}

fn action_step(name: &str, uses: &str, with: Vec<(&str, &str)>, always: bool) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if always {
        fields.push(("if".to_owned(), Yaml::str("${{ always() }}")));
    }
    if name == "Download plan" {
        fields.push(("continue-on-error".to_owned(), Yaml::Bool(true)));
    }
    fields.push(("uses".to_owned(), Yaml::str(uses)));
    fields.push((
        "with".to_owned(),
        Yaml::Map(
            with.into_iter()
                .map(|(key, value)| (key.to_owned(), Yaml::str(value)))
                .collect(),
        ),
    ));
    Yaml::Map(fields)
}

fn fetch_reports_step(_pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    internal_step(
        "Download every expected matrix artifact",
        steps::FETCH_OPERATION,
        None,
        &[
            ("GH_REPO", "${{ github.repository }}"),
            ("GH_TOKEN", "${{ github.token }}"),
        ],
        None,
        None,
    )
}

pub(super) fn readonly_permissions() -> Yaml {
    Yaml::Map(vec![
        ("contents".to_owned(), Yaml::str("read")),
        ("actions".to_owned(), Yaml::str("read")),
        ("attestations".to_owned(), Yaml::str("read")),
    ])
}

fn plan_consumer_env(common_env: &Yaml) -> Yaml {
    let mut entries = match common_env {
        Yaml::Map(entries) => entries
            .iter()
            .filter(|(key, _)| key != "VELNOR_RUN_KEY_ATTEMPT")
            .cloned()
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    entries.push((
        "VELNOR_RUN_KEY_ATTEMPT".to_owned(),
        Yaml::str("${{ needs.plan.outputs.plan_attempt }}"),
    ));
    Yaml::Map(entries)
}

fn map(fields: Vec<(&str, Yaml)>) -> Yaml {
    Yaml::Map(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}
