use super::*;

#[cfg(test)]
#[test]
fn protected_seed_selection_uses_only_closed_caller_fields() {
    let source = "a".repeat(40);
    let manifest = "b".repeat(64);
    let text = format!(
        "name: CI\njobs:\n  homebrew-adoption:\n    uses: tailrocks/velnor-new/.github/workflows/homebrew-adoption.yml@{source}\n    with:\n      transition_id: jackin-homebrew-seed-921858f1\n      sdk_version: \"0.1.53\"\n      sdk_source_sha: {source}\n      sdk_manifest_sha256: {manifest}\n    permissions:\n      contents: read\n"
    );
    let selected = parse_seed_selection(text.as_bytes())
        .expect("parse exact protected seed fields")
        .expect("seed selection");
    assert_eq!(selected.transition_id, "jackin-homebrew-seed-921858f1");
    assert_eq!(selected.version, "0.1.53");
    assert_eq!(selected.source_sha, source);
    assert_eq!(selected.manifest_sha256, manifest);
}

#[cfg(test)]
#[test]
fn protected_seed_selection_rejects_rebinding_or_duplicate_fields() {
    let source = "a".repeat(40);
    let manifest = "b".repeat(64);
    let workflow = |uses: &str, extra: &str| {
        format!(
            "jobs:\n  homebrew-adoption:\n    uses: {uses}\n    with:\n      transition_id: jackin-homebrew-seed-921858f1\n      sdk_version: 0.1.53\n      sdk_source_sha: {source}\n      sdk_manifest_sha256: {manifest}\n{extra}"
        )
    };
    let wrong_callee = workflow(
        &format!("attacker/workflow@{source}"),
        "    permissions:\n      contents: read\n",
    );
    assert!(parse_seed_selection(wrong_callee.as_bytes()).is_err());
    let duplicated = workflow(
        &format!("tailrocks/velnor-new/.github/workflows/homebrew-adoption.yml@{source}"),
        "      sdk_source_sha: cccccccccccccccccccccccccccccccccccccccc\n",
    );
    assert!(parse_seed_selection(duplicated.as_bytes()).is_err());
    assert_eq!(
        parse_seed_selection(b"name: ordinary CI\njobs:\n  test:\n")
            .expect("ordinary workflow is not a seed caller"),
        None
    );
}
