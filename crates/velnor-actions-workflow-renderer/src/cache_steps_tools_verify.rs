// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, yaml::Yaml};

const INSTALLS_VERIFY_SCRIPT: &str = include_str!("cache_steps_tools_installs_verify.sh");

pub(crate) const INSTALLS_VERIFY_NAME: &str = "Re-verify restored Mise installs";

pub(super) fn installs_verify_step(tool_pins: &[String]) -> Result<Yaml, RenderError> {
    verify_step(tool_pins, false)
}

pub(super) fn producer_verify_step(tool_pins: &[String]) -> Result<Yaml, RenderError> {
    verify_step(tool_pins, true)
}

fn verify_step(tool_pins: &[String], producer: bool) -> Result<Yaml, RenderError> {
    for pin in tool_pins {
        let fields: Vec<&str> = pin.split(' ').collect();
        let ok = fields.len() == 5
            && fields.iter().all(|field| {
                !field.is_empty()
                    && field
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-:/@+".contains(&b))
            })
            && fields[3].starts_with("https://")
            && fields[4].len() == 64;
        if !ok {
            return Err(RenderError::InvalidWorkflow(format!("bad_tool_pin:{pin}")));
        }
    }
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(INSTALLS_VERIFY_NAME)),
        ("id".to_owned(), Yaml::str("verify")),
        ("shell".to_owned(), Yaml::str(crate::VERIFY_VERIFIER_SHELL)),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "VELNOR_TOOL_PINS".to_owned(),
                    Yaml::str(tool_pins.join("\n")),
                ),
                (
                    "VELNOR_TOOL_PRODUCER".to_owned(),
                    Yaml::str(producer.to_string()),
                ),
            ]),
        ),
        ("run".to_owned(), Yaml::str(INSTALLS_VERIFY_SCRIPT)),
    ]))
}
