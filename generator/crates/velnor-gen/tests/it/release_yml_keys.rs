// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use crate::release_fixture::{APP, Root, render};
use crate::release_yml_binaries::job;
use crate::yaml_tree::Node;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn a_binary_package_named_like_a_yaml_scalar_gets_a_quoted_output_key() -> TestResult {
    for name in ["null", "true", "false", "yes", "off"] {
        let package = Root {
            package: name,
            ..APP
        };
        let config = format!("[[binaries]]\npackage = \"{name}\"\n");
        let (text, tree) = render(&[package], Some(&config))?;
        assert!(
            text.contains(&format!(
                "      '{name}': '${{{{ steps.record.outputs.{name} }}}}'"
            )),
            "{name}"
        );
        let quoted = format!("'{name}'");
        let commit = format!("{name}_commit");
        assert_eq!(
            job(&tree, "tag")?.get("outputs").map(Node::keys),
            Some(vec![quoted.as_str(), commit.as_str()]),
            "{name}"
        );
    }
    Ok(())
}
