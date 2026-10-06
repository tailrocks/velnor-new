//! Bounded regression cases.
use super::*;

#[test]
fn strict_render_elects_single_writer_per_shared_key()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::{plan_step, shell_step};
    let prepare = || {
        shell_step(
            "Prepare pinned tools",
            vec![
                "mise".to_owned(),
                "install".to_owned(),
                "rust@1.98.1".to_owned(),
            ],
            BTreeMap::new(),
        )
    };
    let text = strict(
        &fixture_ir(vec![
            job(
                "plan",
                "Plan",
                Vec::new(),
                vec![prepare()?, acquire_fixture()?, plan_step()],
            ),
            job(
                "rust-demo",
                "Rust / demo",
                vec!["plan".to_owned()],
                vec![prepare()?],
            ),
        ]),
        &fixture_ctx(),
    )?;
    let plan_at = text.find("\n  plan:\n").expect("plan block");
    let crate_at = text.find("\n  rust-demo:\n").expect("crate block");
    let (plan_block, crate_block) = text.split_at(crate_at);
    let plan_block = &plan_block[plan_at..];
    assert!(
        plan_block.contains("- name: Save Mise tools"),
        "plan wins the shared key:\n{text}"
    );
    assert!(
        plan_block.contains("if: success() && github.event_name == 'push'"),
        "winner saves push-only:\n{text}"
    );
    assert!(
        !crate_block.contains("Save Mise tools"),
        "crate restores read-only:\n{text}"
    );
    assert_eq!(
        text.matches("- name: Save Mise tools").count(),
        1,
        "exactly one saver per key:\n{text}"
    );
    assert!(
        !text.contains("cache_save: ${{"),
        "no setup promises a built-in save:\n{text}"
    );
    Ok(())
}
