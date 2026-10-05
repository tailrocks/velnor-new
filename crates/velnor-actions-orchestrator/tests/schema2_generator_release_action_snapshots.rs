use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::RenderedTree;

pub(super) type Actions = BTreeMap<&'static str, String>;

const ACTION_SNAPSHOTS: [(&str, &str); 8] = [
    (
        "generator-release-attest-manifest",
        include_str!("snapshots/generator-release-attest-manifest.yml"),
    ),
    (
        "generator-release-build-linux",
        include_str!("snapshots/generator-release-build-linux.yml"),
    ),
    (
        "generator-release-build-macos",
        include_str!("snapshots/generator-release-build-macos.yml"),
    ),
    (
        "generator-release-attest-linux",
        include_str!("snapshots/generator-release-attest-linux.yml"),
    ),
    (
        "generator-release-qualify-linux",
        include_str!("snapshots/generator-release-qualify-linux.yml"),
    ),
    (
        "generator-release-attest-macos",
        include_str!("snapshots/generator-release-attest-macos.yml"),
    ),
    (
        "generator-release-qualify-macos",
        include_str!("snapshots/generator-release-qualify-macos.yml"),
    ),
    (
        "generator-release-publish",
        include_str!("snapshots/generator-release-publish.yml"),
    ),
];

pub(super) fn rendered_actions(tree: &RenderedTree) -> Result<Actions, Box<dyn std::error::Error>> {
    let mut actions = Actions::new();
    for (name, snapshot) in ACTION_SNAPSHOTS {
        let path = action_path(name);
        let body = tree.get(&path).ok_or("missing generated local action")?;
        assert_eq!(body, &super::super::marked(snapshot), "{path}");
        assert!(body.lines().count() < 400, "{path} has too many lines");
        assert!(
            !body.contains("${{ needs."),
            "composite action uses caller-only needs context: {path}"
        );
        actions.insert(name, body.to_owned());
    }
    Ok(actions)
}

pub(super) fn committed_actions(
    root: &std::path::Path,
) -> Result<Actions, Box<dyn std::error::Error>> {
    let mut actions = Actions::new();
    for (name, snapshot) in ACTION_SNAPSHOTS {
        let path = root.join(action_path(name));
        let body = std::fs::read_to_string(&path)?;
        assert_eq!(body, super::super::marked(snapshot), "{}", path.display());
        assert!(
            body.lines().count() < 400,
            "{} has too many lines",
            path.display()
        );
        assert!(
            !body.contains("${{ needs."),
            "composite action uses caller-only needs context: {}",
            path.display()
        );
        actions.insert(name, body);
    }
    Ok(actions)
}

fn action_path(name: &str) -> String {
    format!(".github/actions/{name}/action.yml")
}

pub(super) fn action_text(actions: &Actions) -> String {
    actions
        .values()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn action<'a>(
    actions: &'a Actions,
    name: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    actions
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("missing release action {name}").into())
}
