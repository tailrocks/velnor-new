use super::*;

#[test]
fn acquisition_action_binds_distinct_literal_sources_after_prior_environment_writes()
-> Result<(), RenderError> {
    let checkout = "actions/checkout@0000000000000000000000000000000000000000";
    let ctx = context(checkout);
    let commit = "a".repeat(40);
    let target_values = [
        (
            "linux".to_owned(),
            "https://example.invalid/linux".to_owned(),
            "b".repeat(64),
        ),
        (
            "macos".to_owned(),
            "https://example.invalid/macos".to_owned(),
            "c".repeat(64),
        ),
    ];
    let jobs = target_values
        .iter()
        .map(|(id, url, sha)| {
            Ok((
                id.clone(),
                job(
                    id,
                    "ubuntu-26.04",
                    vec![hostile_environment_writer(), acquire(url, sha, &commit)?],
                ),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, RenderError>>()?;
    let shared = crate::lane_share::share_lanes(&jobs, &ctx)?;
    let rendered =
        crate::document::workflow_to_yaml(&workflow(jobs), &shared, &ctx, &BTreeSet::new())?;
    let workflow_yaml = crate::yaml::render_yaml(&rendered);
    assert!(!workflow_yaml.contains("${{ env.VELNOR_ACQUIRE_"));
    assert!(!workflow_yaml.contains(&commit));

    for (id, url, sha) in target_values {
        assert_rendered_acquire_action(&shared, &rendered, &id, &url, &sha, &commit);
    }
    assert_eq!(
        shared
            .files
            .iter()
            .filter(|file| file.path.starts_with(".github/actions/acquire-b3-"))
            .count(),
        2,
        "different target tuples get distinct source-bound actions"
    );
    Ok(())
}

#[test]
fn direct_acquire_step_env_overrides_mutated_job_environment() -> Result<(), RenderError> {
    let ctx = context("actions/checkout@0000000000000000000000000000000000000000");
    let url = "https://example.invalid/expected";
    let sha = "a".repeat(64);
    let commit = "b".repeat(40);
    let step = acquire(url, &sha, &commit)?;
    let hostile_job_env = BTreeMap::from([
        (
            steps::ASSET_URL_ENV.to_owned(),
            "https://attacker.invalid/job".to_owned(),
        ),
        (steps::ASSET_SHA_ENV.to_owned(), "f".repeat(64)),
        (steps::RELEASE_COMMIT_ENV.to_owned(), "e".repeat(40)),
        (
            "VELNOR_ACQUIRE_ASSET_URL".to_owned(),
            "https://attacker.invalid/alias".to_owned(),
        ),
        ("VELNOR_ACQUIRE_ASSET_SHA256".to_owned(), "d".repeat(64)),
        ("VELNOR_ACQUIRE_RELEASE_COMMIT".to_owned(), "c".repeat(40)),
    ]);
    let rendered = crate::document_steps::step_to_yaml(
        "plan",
        &step,
        &ctx,
        &[],
        true,
        &hostile_job_env,
        false,
    )?;
    let env = field(&rendered, "env").expect("literal inner acquisition env");
    assert_eq!(
        field(env, steps::ASSET_URL_ENV),
        Some(&crate::yaml::Yaml::str(url))
    );
    assert_eq!(
        field(env, steps::ASSET_SHA_ENV),
        Some(&crate::yaml::Yaml::str(sha))
    );
    assert_eq!(
        field(env, steps::RELEASE_COMMIT_ENV),
        Some(&crate::yaml::Yaml::str(commit))
    );
    Ok(())
}
