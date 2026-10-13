#[test]
fn consumer_tree_unaffected() -> TestResult {
    let repo = make_repo("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n")?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    assert_eq!(report.files_written.len(), 4, "four consumer tree items");
    let workflow_paths = generated_workflow_paths(&report.files_written);
    assert_eq!(
        workflow_paths,
        vec![WORKFLOW_PATH.to_owned()],
        "consumer policy emits only CI"
    );
    assert!(
        !preview.join(".zizmor.yml").exists(),
        "no staging config in output"
    );
    let input = ZizmorConfigInput {
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        workflows: read_generated_workflows(&preview, &workflow_paths)?,
    };
    assert!(
        render_zizmor_yaml(&input)?.approved_ignores.is_empty(),
        "consumer needs no ignore"
    );
    Ok(())
}

#[test]
fn different_unpinned_tag_still_fails() -> TestResult {
    let (_repo, _parent, preview, yaml, _, workflows) = policy_preview()?;
    let pinned = format!("actions/checkout@{CHECKOUT_ACTION_SHA}");
    let unpinned = format!("actions/checkout@{CHECKOUT_ACTION_VERSION}");
    let mutated = yaml.replacen(&pinned, &unpinned, 1);
    assert_ne!(mutated, yaml, "fixture still pins checkout");
    let staged = stage(&preview, &workflows, &mutated)?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(!output.success, "unpinned checkout must fail: {text}");
    assert!(text.contains("unpinned-uses"), "rule fires: {text}");
    assert!(text.contains(&unpinned), "other ref flagged: {text}");
    Ok(())
}

#[test]
fn blessed_repo_tag_ref_still_fails() -> TestResult {
    let (_repo, _parent, preview, yaml, _, workflows) = policy_preview()?;
    let blessed = format!("{ALINT_ACTION}@{ALINT_ACTION_SHA}");
    let wrong = format!("{ALINT_ACTION}@{ALINT_ACTION_VERSION}");
    let mutated = yaml.replacen(&blessed, &wrong, 1);
    assert_ne!(mutated, yaml, "fixture still carries blessed SHA");
    let staged = stage(&preview, &workflows, &mutated)?;
    let output = run_zizmor(staged.path())?;
    let text = streams(&output);
    assert!(!output.success, "wrong alint tag must fail: {text}");
    assert!(text.contains("unpinned-uses"), "rule fires: {text}");
    assert!(text.contains(&wrong), "wrong ref flagged: {text}");
    Ok(())
}
