use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::RenderedTree;

pub(super) type Actions = BTreeMap<&'static str, String>;

const ACTION_SNAPSHOTS: [(&str, &str); 7] = [
    (
        "generator-release-build-linux",
        include_str!("snapshots/generator-release-build-linux.yml"),
    ),
    (
        "generator-release-build-macos",
        include_str!("snapshots/generator-release-build-macos.yml"),
    ),
    (
        "generator-release-build-macos-intel",
        include_str!("snapshots/generator-release-build-macos-intel.yml"),
    ),
    (
        "generator-release-qualify-linux",
        include_str!("snapshots/generator-release-qualify-linux.yml"),
    ),
    (
        "generator-release-qualify-macos",
        include_str!("snapshots/generator-release-qualify-macos.yml"),
    ),
    (
        "generator-release-qualify-macos-intel",
        include_str!("snapshots/generator-release-qualify-macos-intel.yml"),
    ),
    (
        "generator-release-qualify-homebrew",
        include_str!("snapshots/generator-release-qualify-homebrew.yml"),
    ),
];

pub(super) fn rendered_actions(tree: &RenderedTree) -> Result<Actions, Box<dyn std::error::Error>> {
    assert_action_inventory(tree);
    let mut actions = Actions::new();
    for (name, snapshot) in ACTION_SNAPSHOTS {
        let path = action_path(name);
        let body = tree.get(&path).ok_or("missing generated local action")?;
        assert_eq!(body, &super::super::marked(snapshot), "{path}");
        assert!(
            !body.contains("${{ needs."),
            "composite action uses caller-only needs context: {path}"
        );
        actions.insert(name, body.to_owned());
    }
    Ok(actions)
}

pub(super) fn committed_actions_matching_tree(
    root: &std::path::Path,
    tree: &RenderedTree,
) -> Result<Actions, Box<dyn std::error::Error>> {
    assert_action_inventory(tree);
    let mut actions = Actions::new();
    for (name, _) in ACTION_SNAPSHOTS {
        let relative = action_path(name);
        let rendered = tree
            .get(&relative)
            .ok_or_else(|| format!("missing generated local action: {relative}"))?;
        let path = root.join(&relative);
        let body = std::fs::read_to_string(&path)?;
        assert_eq!(&body, rendered, "{}", path.display());
        assert!(
            !body.contains("${{ needs."),
            "composite action uses caller-only needs context: {}",
            path.display()
        );
        actions.insert(name, body);
    }
    Ok(actions)
}

fn assert_action_inventory(tree: &RenderedTree) {
    let mut actual = tree
        .files
        .iter()
        .filter(|file| {
            file.path.starts_with(".github/actions/generator-release-")
                && file.path.ends_with("/action.yml")
        })
        .map(|file| file.path.as_str())
        .collect::<Vec<_>>();
    actual.sort_unstable();
    let mut expected = ACTION_SNAPSHOTS
        .iter()
        .map(|(name, _)| format!(".github/actions/{name}/action.yml"))
        .collect::<Vec<_>>();
    for retired in [
        "generator-release-attest-linux",
        "generator-release-attest-macos",
        "generator-release-attest-macos-intel",
        "generator-release-attest-manifest",
        "generator-release-publish",
    ] {
        assert!(
            !actual.contains(&format!(".github/actions/{retired}/action.yml").as_str()),
            "retired privileged action must not be generated: {retired}"
        );
    }
    expected.sort_unstable();
    let expected = expected.iter().map(String::as_str).collect::<Vec<_>>();
    assert_eq!(
        actual, expected,
        "only the seven real build and qualification composites are emitted"
    );
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
