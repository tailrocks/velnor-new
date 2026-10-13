#[test]
fn rust_target_and_features_reject_shell_expressions() {
    use velnor_actions_contract::{ContractError, RustStackConfig};
    let file = ".velnor/config.toml";
    let stack = RustStackConfig::default_config();
    let mut good = stack.clone();
    good.configurations[0].target = "x86_64-unknown-linux-gnu".to_owned();
    good.configurations[0].features = vec![
        "default".to_owned(),
        "serde/std".to_owned(),
        "dep:tokio".to_owned(),
    ];
    assert_eq!(good.validate(file), Ok(()));
    for bad in ["", "x86_64;evil", "${{secrets.x}}", "$(evil)", "a/b/c"] {
        let mut named = stack.clone();
        named.configurations[0].target = bad.to_owned();
        let Err(ContractError::Config {
            key_path, problem, ..
        }) = named.validate(file)
        else {
            panic!("{bad:?} target must fail");
        };
        assert_eq!(key_path, "stacks.rust.configurations.target");
        assert!(problem.starts_with("bad_target:"), "got {problem}");
    }
    for bad in ["", "feat;evil", "${{secrets.x}}", "$(evil)", "a b"] {
        let mut named = stack.clone();
        named.configurations[0].features = vec![bad.to_owned()];
        let Err(ContractError::Config {
            key_path, problem, ..
        }) = named.validate(file)
        else {
            panic!("{bad:?} feature must fail");
        };
        assert_eq!(key_path, "stacks.rust.configurations.features");
        assert!(problem.starts_with("bad_feature:"), "got {problem}");
    }
}

