use velnor_actions_ansible::check_ansible_cfg;

fn codes(text: &str) -> Vec<String> {
    check_ansible_cfg(text)
        .iter()
        .map(|error| error.code().to_owned())
        .collect()
}

#[test]
fn inert_settings_are_admitted() {
    let text = "# comment\n[defaults]\ninventory = hosts.ini, inventory/hosts.yml\nroles_path = roles : ./vendor/roles : roles/common\nhost_key_checking = False ; inline\nretry_files_enabled = False\n\n[ssh_connection]\npipelining = True\n";
    assert_eq!(codes(text), Vec::<String>::new());
    let clean = "[defaults]\ninventory = hosts.ini\nforks: 10\n[ssh_connection]\nretries = 3\n";
    assert_eq!(codes(clean), Vec::<String>::new());
}

#[test]
fn path_values_are_literal_and_use_their_key_specific_delimiter() {
    for key in ["inventory", "roles_path"] {
        for value in [
            "~/roles",
            "$HOME/roles",
            "${HOME}/roles",
            "{{CWD}}/roles",
            "\"roles\"",
            "'roles'",
            r"roles\vendor",
            "role name",
            "rôle",
            "",
        ] {
            let cfg = format!("[defaults]\n{key} = {value}\n");
            assert_eq!(
                codes(&cfg),
                [format!("ansible_cfg_path_invalid:{key}")],
                "{key} = {value}"
            );
        }
        for value in ["../roles", "roles/../../outside", "/etc/ansible/hosts"] {
            let cfg = format!("[defaults]\n{key} = {value}\n");
            assert_eq!(
                codes(&cfg),
                [format!("ansible_cfg_path_escapes_root:{key}")],
                "{key} = {value}"
            );
        }
    }
    for (key, delimiter, wrong) in [
        ("inventory", ',', "hosts.ini:other.ini"),
        ("roles_path", ':', "roles,other-roles"),
    ] {
        assert_eq!(
            codes(&format!("[defaults]\n{key} = {wrong}\n")),
            [format!("ansible_cfg_path_invalid:{key}")],
            "wrong delimiter for {key}"
        );
        for value in [
            format!("{delimiter}roles"),
            format!("roles{delimiter}"),
            format!("roles{delimiter}{delimiter}other"),
        ] {
            assert_eq!(
                codes(&format!("[defaults]\n{key} = {value}\n")),
                [format!("ansible_cfg_path_invalid:{key}")],
                "empty component for {key}: {value}"
            );
        }
    }
}

#[test]
fn code_loading_and_secret_keys_are_refused() {
    let text = "[defaults]\ncallback_plugins = plugins\nvault_password_file = ~/.vault\nlibrary = lib\n[privilege_escalation]\nbecome = True\n";
    assert_eq!(
        codes(text),
        [
            "ansible_cfg_key_forbidden:defaults.callback_plugins",
            "ansible_cfg_key_forbidden:defaults.library",
            "ansible_cfg_key_forbidden:defaults.vault_password_file",
            "ansible_cfg_key_forbidden:privilege_escalation.become",
        ]
    );
    assert_eq!(
        codes("not an ini line"),
        ["ansible_cfg_unparsed:not an ini line"]
    );
    assert_eq!(
        codes("[defaults]\ninventory = /etc/ansible/hosts\n"),
        ["ansible_cfg_path_escapes_root:inventory"]
    );
}
