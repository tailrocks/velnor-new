use velnor_actions_ansible::{check_vault, is_playbook, top_level_playbooks};

const PLAY: &str = "---\n- name: Deploy\n  hosts: titan\n  tasks: []\n";

#[test]
fn plays_and_imports_are_playbooks_other_yaml_is_not() {
    assert!(is_playbook(PLAY));
    assert!(is_playbook("- hosts: all\n  tasks: []\n"));
    assert!(is_playbook("- import_playbook: other.yml\n"));
    assert!(!is_playbook("collections:\n  - name: a.b\n"));
    assert!(!is_playbook(
        "- name: task only\n  ansible.builtin.debug:\n    msg: x\n"
    ));
    assert!(!is_playbook(""));
}

#[test]
fn only_top_level_playbooks_of_the_root_are_listed_sorted() {
    let files = [
        ("ansible-configs/setup-titan.yml", PLAY),
        ("ansible-configs/install-base.yml", PLAY),
        ("ansible-configs/requirements.yaml", "collections: []\n"),
        ("ansible-configs/tasks/include.yml", PLAY),
        ("other/x.yml", PLAY),
        ("ansible-configs2/y.yml", PLAY),
    ];
    assert_eq!(
        top_level_playbooks("ansible-configs", &files),
        [
            "ansible-configs/install-base.yml",
            "ansible-configs/setup-titan.yml"
        ]
    );
}

#[test]
fn vault_files_fail() {
    let files = [
        ("a/secrets.yml", "$ANSIBLE_VAULT;1.1;AES256\n6162"),
        ("a/ok.yml", PLAY),
    ];
    let findings = check_vault(&files);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code(), "ansible_vault_in_ci:a/secrets.yml");
}
