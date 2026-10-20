use std::collections::BTreeMap;
use velnor_actions_ansible::{collection_url, parse_requirements_yaml, verify_collections};
use velnor_actions_contract::config::AnsibleCollection;

const YAML: &str = "collections:\n  - name: community.general\n    version: 13.4.0\n  - name: ansible.posix\n    version: 2.2.2\n  # fixture\n";

fn pin(name: &str, version: &str) -> AnsibleCollection {
    AnsibleCollection {
        name: name.to_owned(),
        version: version.to_owned(),
        sha256: "a".repeat(64),
    }
}

#[test]
fn requirements_yaml_parses_names_and_versions() {
    let parsed = parse_requirements_yaml(YAML).expect("yaml");
    assert_eq!(
        parsed,
        BTreeMap::from([
            ("ansible.posix".to_owned(), "2.2.2".to_owned()),
            ("community.general".to_owned(), "13.4.0".to_owned()),
        ])
    );
}

#[test]
fn unversioned_and_garbled_entries_fail_closed() {
    let error = parse_requirements_yaml("collections:\n  - name: a.b\n").expect_err("unversioned");
    assert_eq!(error.code(), "ansible_collection_unversioned:a.b");
    let error = parse_requirements_yaml("collections:\n  - nonsense\n").expect_err("garbled");
    assert!(error.code().starts_with("ansible_requirements_unparsed"));
    assert_eq!(
        parse_requirements_yaml("roles: []\n")
            .expect("no collections")
            .len(),
        0
    );
}

#[test]
fn pins_and_requirements_must_agree_both_ways() {
    let parsed = parse_requirements_yaml(YAML).expect("yaml");
    let ok = [
        pin("ansible.posix", "2.2.2"),
        pin("community.general", "13.4.0"),
    ];
    assert_eq!(verify_collections(&ok, &parsed).len(), 0);
    let drift = [
        pin("ansible.posix", "2.2.3"),
        pin("community.general", "13.4.0"),
    ];
    assert_eq!(
        verify_collections(&drift, &parsed)[0].code(),
        "ansible_collections_mismatch:ansible.posix"
    );
    let missing = [pin("ansible.posix", "2.2.2")];
    assert_eq!(
        verify_collections(&missing, &parsed)[0].code(),
        "ansible_collections_mismatch:community.general"
    );
    let extra = [
        pin("ansible.posix", "2.2.2"),
        pin("community.general", "13.4.0"),
        pin("community.postgresql", "4.2.0"),
    ];
    assert_eq!(
        verify_collections(&extra, &parsed)[0].code(),
        "ansible_collections_mismatch:community.postgresql"
    );
}

#[test]
fn urls_follow_the_single_galaxy_template() {
    assert_eq!(
        collection_url(&pin("community.general", "13.4.0")),
        "https://galaxy.ansible.com/api/v3/plugin/ansible/content/published/collections/artifacts/community-general-13.4.0.tar.gz"
    );
}
