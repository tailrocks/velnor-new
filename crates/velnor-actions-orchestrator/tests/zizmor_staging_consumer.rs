use super::*;

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
    assert_eq!(
        report.files_written,
        [
            ".github/AGENTS.md",
            ".github/CLAUDE.md",
            ".github/actionlint.yaml",
            ".github/actions/velnor-tool-seed/action.yml",
            ".github/workflows/ci.yml",
        ]
        .map(str::to_owned)
    );
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
