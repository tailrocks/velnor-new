use std::collections::BTreeSet;
use velnor_actions_ansible::affected_roots;

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

#[test]
fn root_directory_and_shared_files_select_roots() {
    let roots = vec!["ansible-configs".to_owned(), "other".to_owned()];
    let shared = vec!["tools/requirements.txt".to_owned()];
    assert_eq!(
        affected_roots(&roots, &shared, &set(&["ansible-configs/a.yml"])),
        set(&["ansible-configs"])
    );
    assert_eq!(
        affected_roots(&roots, &shared, &set(&["tools/requirements.txt"])).len(),
        2
    );
    assert_eq!(
        affected_roots(&roots, &shared, &set(&["README.md", "ansible-configs2/x"])).len(),
        0
    );
}
