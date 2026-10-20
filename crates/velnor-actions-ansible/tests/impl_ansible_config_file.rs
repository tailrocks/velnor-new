use velnor_actions_ansible::check_ansible_cfg;

fn codes(text: &str) -> Vec<String> {
    check_ansible_cfg(text)
        .iter()
        .map(|error| error.code().to_owned())
        .collect()
}

#[test]
fn inert_settings_are_admitted() {
    let text = "# comment\n[defaults]\ninventory = hosts.ini\nroles_path = roles:../shared/roles\nhost_key_checking = False ; inline\nretry_files_enabled = False\n\n[ssh_connection]\npipelining = True\n";
    // `../shared/roles` leaves the root, so only that path is reported.
    assert_eq!(codes(text), ["ansible_cfg_path_escapes_root:roles_path"]);
    let clean = "[defaults]\ninventory = hosts.ini\nforks: 10\n[ssh_connection]\nretries = 3\n";
    assert_eq!(codes(clean), Vec::<String>::new());
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
