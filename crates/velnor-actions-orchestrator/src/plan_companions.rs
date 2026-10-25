use super::{GenerationPreparation, push};
use velnor_actions_contract::config::ReleaseRegistry;
use velnor_actions_contract::{FRESHNESS_WORKFLOW_PATH, TOFU_APPLY_WORKFLOW_PATH};
use velnor_actions_workflow_renderer::release_tree::{RELEASE_CONFIG_PATH, RELEASE_TREE_PATHS};

pub(super) fn file_lines(out: &mut String, prep: &GenerationPreparation) {
    release_file_lines(out, prep);
    publish_file_lines(out, prep);
    freshness_file_lines(out, prep);
    owned_tool_file_lines(out, prep);
    tofu_apply_file_lines(out, prep);
    if prep.config.dco.is_some() {
        push(
            out,
            &format!("  {}", velnor_actions_contract::DCO_CONFIG_PATH),
        );
    }
    if prep
        .config
        .stacks
        .rust
        .as_ref()
        .is_some_and(|rust| rust.binary_release.preview.enabled)
        && prep
            .config
            .distribution
            .homebrew_tap
            .as_ref()
            .is_some_and(|tap| tap.enabled)
    {
        push(out, "  .github/workflows/preview-tap-reconcile.yml");
    }
    if prep.config.workflow.review_completion.is_some() {
        push(
            out,
            &format!(
                "  {}",
                velnor_actions_contract::review_completion::REVIEW_COMPLETION_WORKFLOW_PATH
            ),
        );
    }
}

fn release_file_lines(out: &mut String, prep: &GenerationPreparation) {
    let enabled = crate::release_emit::enabled_release(prep)
        .ok()
        .flatten()
        .is_some_and(|release| release.registry == ReleaseRegistry::CratesIo);
    if enabled {
        for path in RELEASE_TREE_PATHS {
            push(out, &format!("  {path}"));
        }
    }
}

fn publish_file_lines(out: &mut String, prep: &GenerationPreparation) {
    let git_tag = crate::release_emit::git_tag_release(prep)
        .ok()
        .flatten()
        .is_some();
    if git_tag {
        push(out, &format!("  {RELEASE_CONFIG_PATH}"));
    }
    if git_tag || !prep.config.publish.is_empty() {
        push(
            out,
            &format!(
                "  {}",
                velnor_actions_workflow_renderer::publish_render::PUBLISH_WORKFLOW_PATH
            ),
        );
    }
}

fn freshness_file_lines(out: &mut String, prep: &GenerationPreparation) {
    if crate::freshness_emit::freshness_enabled(prep) {
        push(out, &format!("  {FRESHNESS_WORKFLOW_PATH}"));
    }
    if crate::freshness_emit::upgrades::upgrades_enabled(prep) {
        push(
            out,
            &format!(
                "  {}",
                velnor_actions_contract::config::UPGRADES_WORKFLOW_PATH
            ),
        );
    }
}

fn tofu_apply_file_lines(out: &mut String, prep: &GenerationPreparation) {
    if crate::tofu_apply_emit::tofu_apply_enabled(prep) {
        push(out, &format!("  {TOFU_APPLY_WORKFLOW_PATH}"));
    }
}

fn owned_tool_file_lines(out: &mut String, prep: &GenerationPreparation) {
    if let Ok(files) = crate::owned_tool_publication::files(prep) {
        for file in files {
            push(out, &format!("  {}", file.path));
        }
    }
}
