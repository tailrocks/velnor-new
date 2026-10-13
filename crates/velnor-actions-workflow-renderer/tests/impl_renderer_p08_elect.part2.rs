#[test]
fn tools_cache_election_binds_restore_to_static_identity_digest() -> Result<(), RenderError> {
    use velnor_actions_contract::workflow::step_identity::TOOLS_CACHE_IDENTITY_DIGEST_INPUT;
    let mut jobs = BTreeMap::from([
        ("rust-a".to_owned(), keyed_job("actionlint@1.7.12")?),
        ("rust-b".to_owned(), keyed_job("actionlint@1.7.12")?),
    ]);
    // Same canonical restore key, but a retargeted static digest splits the
    // election: each identity elects its own writer.
    let identity = &mut jobs.get_mut("rust-b").expect("second owner").steps[0];
    let StepKind::Action { with, .. } = &mut identity.kind else {
        return Err(RenderError::InvalidWorkflow(
            "test_identity_not_action".to_owned(),
        ));
    };
    with.insert(TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(), "f".repeat(64));
    assert_eq!(restore_key(&jobs["rust-a"]), restore_key(&jobs["rust-b"]));
    elect_cache_writers(&mut jobs)?;
    assert_eq!(tools_saves(&jobs["rust-a"]).len(), 1);
    assert_eq!(tools_saves(&jobs["rust-b"]).len(), 1);
    Ok(())
}
