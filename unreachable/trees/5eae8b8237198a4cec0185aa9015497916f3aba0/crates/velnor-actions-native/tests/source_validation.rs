//! Independent inventories exercise closed Ruby, Shell and REUSE proposals.

#[test]
fn ruby_uses_compilation_only_and_paths_after_separator()
-> Result<(), velnor_actions_contract::ContractError> {
    let paths = vec!["Formula/orbit.rb".to_owned(), "Casks/nebula.rb".to_owned()];
    let argv = velnor_actions_native::ruby::syntax(&paths)?;
    assert_eq!(argv[0], "ruby");
    assert_eq!(argv[3], "--");
    assert_eq!(&argv[4..], &paths);
    assert_eq!(
        argv[2],
        "ARGV.each { |path| RubyVM::InstructionSequence.compile_file(path) }"
    );
    Ok(())
}

#[test]
fn shell_retains_explicit_selected_source_inventory()
-> Result<(), velnor_actions_contract::ContractError> {
    let paths = vec!["scripts/orbit.sh".to_owned(), "bin/nebula".to_owned()];
    assert_eq!(
        velnor_actions_native::shell::check(&paths)?,
        ["shellcheck", "scripts/orbit.sh", "bin/nebula"]
    );
    Ok(())
}

#[test]
fn ruby_rejects_option_injection_and_escaping_paths() {
    for path in ["-e", "../escape.rb", "/tmp/injected.rb", "a\n.rb"] {
        assert!(
            velnor_actions_native::ruby::syntax(&[path.to_owned()]).is_err(),
            "{path}"
        );
    }
}

#[test]
fn shell_rejects_option_injection_and_escaping_paths() {
    for path in [
        "--external-sources",
        "../escape.sh",
        "/tmp/injected.sh",
        "a\n.sh",
    ] {
        assert!(
            velnor_actions_native::shell::check(&[path.to_owned()]).is_err(),
            "{path}"
        );
    }
}

#[test]
fn empty_source_inventory_never_claims_validation() {
    assert!(velnor_actions_native::ruby::syntax(&[]).is_err());
    assert!(velnor_actions_native::shell::check(&[]).is_err());
}

#[test]
fn reuse_fixed_policy_has_no_caller_command_selector() {
    assert_eq!(velnor_actions_native::reuse::lint(), ["reuse", "lint"]);
}
