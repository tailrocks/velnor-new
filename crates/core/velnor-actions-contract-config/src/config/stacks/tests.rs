use super::{RustConfiguration, is_valid_rust_target};
use crate::config::RustStackConfig;

#[test]
fn target_grammar_accepts_host_and_triples_only() {
    for target in [
        "host",
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
    ] {
        assert!(is_valid_rust_target(target), "{target}");
    }
    // Proven X2 PoC plus metacharacter and flag-shaped values.
    for target in [
        "",
        " ",
        "${{ secrets.CARGO_REGISTRY_TOKEN }}",
        "$TRIPLE",
        "`id`",
        "-foo",
        "HOST",
        "x86_64 unknown",
        "a/b",
    ] {
        assert!(!is_valid_rust_target(target), "{target:?}");
    }
}

#[test]
fn hostile_target_fails_validation() {
    let mut stack = RustStackConfig::default_config();
    stack.configurations = vec![RustConfiguration {
        name: "default".to_owned(),
        features: Vec::new(),
        target: "${{ secrets.CARGO_REGISTRY_TOKEN }}".to_owned(),
    }];
    let err = stack.validate("config.toml").expect_err("PoC target fails");
    assert!(err.to_string().contains("bad_target"), "{err}");
}
#[test]
fn ignore_admission_uses_typed_stack_eligibility() {
    use crate::config::StacksConfig;
    for ids in [
        vec!["rust".to_owned()],
        vec!["tofu".to_owned()],
        vec!["rust".to_owned(), "tofu".to_owned()],
    ] {
        let config = StacksConfig {
            ignore: ids,
            rust: None,
            tofu: None,
        };
        assert!(config.validate("config.toml").is_ok());
    }
    let explicit = StacksConfig {
        ignore: vec!["mise".to_owned()],
        rust: None,
        tofu: None,
    };
    let error = explicit
        .validate("config.toml")
        .expect_err("explicit checks cannot be ignored");
    assert!(error.to_string().contains("stack_not_ignorable:mise"));
    let unknown = StacksConfig {
        ignore: vec!["unknown".to_owned()],
        rust: None,
        tofu: None,
    };
    let error = unknown.validate("config.toml").expect_err("unknown stack");
    assert!(error.to_string().contains("unknown_stack_id:unknown"));
}
