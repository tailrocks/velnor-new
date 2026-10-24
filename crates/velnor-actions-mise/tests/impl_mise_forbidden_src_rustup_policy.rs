pub(super) fn token_allowed_in_scoped_fixture(name: &str, token: &str) -> bool {
    (matches!(
        name,
        "requests_executable_lookup.rs" | "requests_executable_lookup_tests.rs"
    ) && token == "symlink")
        || (name == "requests_executable_lookup_tests.rs"
            && matches!(
                token,
                "fs::write" | "create_dir" | "create_new" | "set_permissions"
            ))
}

pub(super) fn rustup_literal_allowed(name: &str) -> bool {
    name == "requests_executable_lookup.rs" || name == "requests_executable_lookup_tests.rs"
}

pub(super) fn has_pinned_mise_rustup_payload(source: &str) -> bool {
    let compact = super::code_of(source)
        .into_iter()
        .map(|(_, line)| line.split_whitespace().collect::<String>())
        .collect::<String>();
    compact.matches("OsString::from(\"rustup\")").count() == 1
        && compact.matches("\"rustup\"").count() == 1
        && compact.matches("IsolatedCommand::mise_exec(").count() == 1
        && compact.contains("letpayload=[OsString::from(\"rustup\"),OsString::from(\"which\"),OsString::from(\"--toolchain\"),OsString::from(version),OsString::from(component),];")
        && compact.contains("IsolatedCommand::mise_exec(&[selector.to_owned()],&payload)?")
        && !compact.contains("Command::new(\"rustup\")")
        && !compact.contains("Command::new(OsString::from(\"rustup\"))")
}
