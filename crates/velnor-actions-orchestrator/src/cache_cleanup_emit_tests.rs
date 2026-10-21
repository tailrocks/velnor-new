use velnor_actions_contract::PullRequestCachePolicy;
use velnor_actions_workflow_renderer::cache_cleanup::CACHE_CLEANUP_PATH;

use super::files_for;

#[test]
fn the_workflow_exists_only_where_pull_requests_write_scoped_entries() {
    assert!(
        files_for(PullRequestCachePolicy::ReadOnly, "ubuntu-26.04")
            .expect("read-only")
            .is_empty(),
        "read-only pull requests write nothing to clean"
    );
    let scoped =
        files_for(PullRequestCachePolicy::SameRepositoryScoped, "ubuntu-26.04").expect("scoped");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].path, CACHE_CLEANUP_PATH);
    assert!(
        scoped[0]
            .bytes
            .contains("Delete closed pull request cache entries")
    );
}
