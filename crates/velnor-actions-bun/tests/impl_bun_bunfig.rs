use velnor_actions_bun::velnor_bunfig;

fn scopes() -> Vec<String> {
    vec!["@chainargos".to_owned()]
}

#[test]
fn allowlisted_keys_are_copied_and_the_rest_dropped() {
    let repository = "# comment\n[install]\nlinker = \"isolated\"\nexact = true\ncache = { dir = \"/x\" }\ndev = false\n\n[run]\nbun = true\n";
    let text = velnor_bunfig(Some(repository), &scopes(), true).expect("bunfig");
    assert_eq!(
        text,
        "[install]\nlinker = \"isolated\"\nexact = true\n\n[install.scopes.\"@chainargos\"]\nurl = \"https://npm.pkg.github.com/\"\ntoken = \"$VELNOR_NPM_READ_TOKEN\"\n"
    );
}

#[test]
fn lifecycle_phase_has_no_token_and_absent_repository_config_is_fine() {
    assert_eq!(
        velnor_bunfig(None, &scopes(), false).expect("bunfig"),
        "[install]\n"
    );
    assert!(
        !velnor_bunfig(Some("[install]\nlinker = \"hoisted\"\n"), &scopes(), false)
            .expect("bunfig")
            .contains("TOKEN")
    );
}

#[test]
fn registry_redirects_fail_generation() {
    for repository in [
        "[install]\nregistry = \"https://evil.example/\"\n",
        "[install.scopes.\"@chainargos\"]\nurl = \"https://evil.example/\"\n",
        "[install]\ncafile = \"ca.pem\"\n",
        "[install]\nca = \"x\"\n",
        "[install]\ntoken = \"x\"\n",
    ] {
        assert_eq!(
            velnor_bunfig(Some(repository), &scopes(), true)
                .expect_err(repository)
                .code(),
            "bunfig_registry_override",
            "{repository}"
        );
    }
}

#[test]
fn invalid_text_and_values_fail_closed() {
    assert_eq!(
        velnor_bunfig(Some("[install"), &[], false)
            .expect_err("toml")
            .code(),
        "bunfig_invalid"
    );
    assert_eq!(
        velnor_bunfig(Some("[install]\nlinker = \"flat\"\n"), &[], false)
            .expect_err("linker")
            .code(),
        "bunfig_value:install.linker"
    );
    assert_eq!(
        velnor_bunfig(Some("[install]\nexact = \"yes\"\n"), &[], false)
            .expect_err("exact")
            .code(),
        "bunfig_value:install.exact"
    );
}

#[test]
fn scope_names_cannot_inject_toml() {
    for scope in ["chainargos", "@", "@a\"b", "@a\nb", "@A"] {
        assert!(
            velnor_bunfig(None, &[scope.to_owned()], true).is_err(),
            "{scope:?}"
        );
    }
}
