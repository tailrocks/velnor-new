// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::Table;
use velnor_model::{MbxConflict, MbxDeclaration, MbxRule, mbx_declaration};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn declaration(texts: &[&str], rule: MbxRule) -> Result<MbxDeclaration, toml::de::Error> {
    let tables = texts
        .iter()
        .map(|text| text.parse::<Table>())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(mbx_declaration(&tables, rule))
}

fn declared(text: &str, rule: MbxRule) -> Result<bool, toml::de::Error> {
    Ok(declaration(&[text], rule)?.declared())
}

#[test]
fn the_strict_rule_reads_the_exact_wrapper_and_the_option_on_the_rust_tool_only() -> TestResult {
    for (name, text, strict, wide) in [
        (
            "exact wrapper",
            "[wrappers.cargo]\ncommand = \"mbx\"\n",
            true,
            true,
        ),
        (
            "wrapper with a path",
            "[wrappers.cargo]\ncommand = \"./bin/mbx\"\n",
            false,
            true,
        ),
        (
            "wrapper in another directory",
            "[wrappers.cargo]\ncommand = \"/tmp/evil/mbx\"\n",
            false,
            true,
        ),
        (
            "wrapper with arguments",
            "[wrappers.cargo]\ncommand = \"mbx --x\"\n",
            false,
            true,
        ),
        (
            "another program that only starts like mbx",
            "[wrappers.cargo]\ncommand = \"mbx-wrapper\"\n",
            false,
            false,
        ),
        (
            "option on rust",
            "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\n",
            true,
            true,
        ),
        (
            "option on core:rust",
            "[tools]\n\"core:rust\" = { version = \"1.99.1\", mr_boxington = true }\n",
            true,
            true,
        ),
        (
            "option on another tool",
            "[tools]\nnode = { version = \"24\", mr_boxington = true }\n",
            false,
            true,
        ),
        (
            "option as a list of requests on another tool",
            "[tools]\nnode = [{ version = \"24\", mr_boxington = true }]\n",
            false,
            true,
        ),
        (
            "option as a list of requests on rust",
            "[tools]\nrust = [{ version = \"1.99.1\", mr_boxington = true }]\n",
            false,
            true,
        ),
        (
            "list of versions without the option",
            "[tools]\nrust = [\"1.100.0\"]\n",
            false,
            false,
        ),
        (
            "option off",
            "[tools]\nrust = { version = \"1.99.1\", mr_boxington = false }\n",
            false,
            false,
        ),
        (
            "option as a string",
            "[tools]\nrust = { version = \"1.99.1\", mr_boxington = \"true\" }\n",
            false,
            false,
        ),
        (
            "pin alone",
            "[tools]\nmr-boxington = \"v1.30.0\"\n",
            false,
            false,
        ),
    ] {
        assert_eq!(declared(text, MbxRule::Strict)?, strict, "strict: {name}");
        assert_eq!(declared(text, MbxRule::Wide)?, wide, "wide: {name}");
    }
    Ok(())
}

#[test]
fn a_key_set_in_two_files_is_a_conflict_whichever_spelling_each_file_uses() -> TestResult {
    for (name, first, second, key) in [
        (
            "rust and rust",
            "[tools]\nrust = \"1.99.1\"\n",
            "[tools]\nrust = \"1.99.1\"\n",
            "tools.rust",
        ),
        (
            "rust and core:rust",
            "[tools]\nrust = \"1.99.1\"\n",
            "[tools]\n\"core:rust\" = \"1.99.1\"\n",
            "tools.rust",
        ),
        (
            "core:rust and rust",
            "[tools]\n\"core:rust\" = \"1.99.1\"\n",
            "[tools]\nrust = \"1.99.1\"\n",
            "tools.rust",
        ),
        (
            "wrapper and wrapper",
            "[wrappers.cargo]\ncommand = \"mbx\"\n",
            "[wrappers.cargo]\ncommand = \"sccache\"\n",
            "wrappers.cargo",
        ),
        (
            "pin and pin",
            "[tools]\nmr-boxington = \"v1.30.0\"\n",
            "[tools]\nmr-boxington = \"v1.30.0\"\n",
            "tools.mr-boxington",
        ),
    ] {
        for rule in [MbxRule::Strict, MbxRule::Wide] {
            assert_eq!(
                declaration(&[first, second], rule)?,
                MbxDeclaration::Conflicting(MbxConflict {
                    key,
                    first: 0,
                    second: 1
                }),
                "{name}"
            );
        }
    }
    Ok(())
}

#[test]
fn settings_in_one_file_and_unrelated_content_in_another_are_not_a_conflict() -> TestResult {
    let all = "[tools]\nrust = { version = \"1.99.1\", mr_boxington = true }\nmr-boxington = \"v1.30.0\"\n[wrappers.cargo]\ncommand = \"mbx\"\n";
    let unrelated = "[tools]\nnode = \"24\"\n[wrappers.node]\ncommand = \"x\"\n";
    for rule in [MbxRule::Strict, MbxRule::Wide] {
        assert_eq!(
            declaration(&[all, unrelated], rule)?,
            MbxDeclaration::Declared
        );
        assert_eq!(
            declaration(&[unrelated, all], rule)?,
            MbxDeclaration::Declared
        );
        assert_eq!(
            declaration(&[unrelated, unrelated], rule)?,
            MbxDeclaration::Absent
        );
    }
    Ok(())
}
