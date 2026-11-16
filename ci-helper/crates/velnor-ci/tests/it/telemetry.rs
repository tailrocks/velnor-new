// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::telemetry::configuration;

#[test]
fn configuration_ignores_command_purpose_and_normalizes_features() {
    let argv = |words: &[&str]| words.iter().map(ToString::to_string).collect::<Vec<_>>();
    assert_eq!(
        configuration(&argv(&["cargo", "clippy", "--features", "b,a"])),
        configuration(&argv(&[
            "cargo-nextest",
            "nextest",
            "run",
            "--features",
            "a,b"
        ]))
    );
    for words in [
        vec!["cargo", "check", "--features=a,b"],
        vec!["cargo", "check", "--features", "b", "--features", "a"],
    ] {
        assert_eq!(
            configuration(&argv(&words)),
            configuration(&argv(&["cargo", "check", "--features", "b,a"]))
        );
    }
    assert_eq!(
        configuration(&argv(&["cargo", "check", "--release"])),
        configuration(&argv(&["cargo", "check", "--profile=release"]))
    );
    assert_ne!(
        configuration(&argv(&["cargo", "check", "--release"])),
        configuration(&argv(&["cargo", "check"]))
    );
    assert_ne!(
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--target",
            "aarch64-unknown-linux-gnu"
        ])),
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "aarch64-unknown-linux-gnu"
        ]))
    );
    assert_eq!(
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--target",
            "aarch64-unknown-linux-gnu"
        ])),
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "aarch64-unknown-linux-gnu",
            "--target",
            "x86_64-unknown-linux-gnu"
        ]))
    );
    assert_eq!(
        configuration(&argv(&[
            "cargo",
            "check",
            "--target=x86_64-unknown-linux-gnu"
        ])),
        configuration(&argv(&[
            "cargo",
            "check",
            "--target",
            "x86_64-unknown-linux-gnu"
        ])),
    );
}

#[test]
fn configuration_ignores_test_arguments_after_the_separator() {
    let base = ["cargo", "test"].map(str::to_owned);
    for options in [
        vec!["--features", "extra"],
        vec!["--profile=release"],
        vec!["--target", "aarch64-unknown-linux-gnu"],
        vec!["--config=build.jobs=2"],
    ] {
        let options: Vec<_> = options.into_iter().map(str::to_owned).collect();
        let mut cargo = base.to_vec();
        cargo.extend(options.clone());
        assert_ne!(configuration(&base), configuration(&cargo));
        let mut test = base.to_vec();
        test.push("--".to_owned());
        test.extend(options);
        assert_eq!(configuration(&base), configuration(&test));
    }
}
