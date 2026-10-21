use velnor_actions_workflow_renderer::cache_prune::CACHE_PRUNE_PATH;

use super::files_for;

#[test]
fn the_workflow_exists_only_where_a_cache_family_can_be_written() {
    assert!(
        files_for(false, "ubuntu-26.04", "main")
            .expect("none")
            .is_empty(),
        "a repository without cache writers gets no prune workflow"
    );
    let files = files_for(true, "ubuntu-26.04", "trunk").expect("emitted");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, CACHE_PRUNE_PATH);
    assert!(files[0].bytes.contains("      - trunk\n"));
}
