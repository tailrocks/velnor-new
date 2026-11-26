// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/job_message.rs

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::context_data::member;
use super::template_token::token_literal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionStepKind {
    Action,
    BackgroundStepControl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionReferenceType {
    Repository,
    ContainerRegistry,
    Script,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Discriminator {
    Number(i64),
    Name(String),
}

impl Discriminator {
    fn matches(&self, number: i64, name: &str) -> bool {
        match self {
            Self::Number(value) => *value == number,
            Self::Name(value) => value.eq_ignore_ascii_case(name),
        }
    }
}

impl<'de> Deserialize<'de> for ActionStepKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Discriminator::deserialize(deserializer)?;
        if value.matches(4, "action") {
            Ok(Self::Action)
        } else if value.matches(5, "backgroundStepControl") {
            Ok(Self::BackgroundStepControl)
        } else {
            Err(serde::de::Error::custom("unknown step type"))
        }
    }
}

impl<'de> Deserialize<'de> for ActionReferenceType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Discriminator::deserialize(deserializer)?;
        if value.matches(1, "repository") {
            Ok(Self::Repository)
        } else if value.matches(2, "containerRegistry") {
            Ok(Self::ContainerRegistry)
        } else if value.matches(3, "script") {
            Ok(Self::Script)
        } else {
            Err(serde::de::Error::custom("unknown action reference type"))
        }
    }
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionStep {
    #[serde(rename = "type", alias = "Type")]
    pub kind: ActionStepKind,
    #[serde(default, alias = "Id")]
    pub id: Option<String>,
    #[serde(default, alias = "Name")]
    pub name: Option<String>,
    #[serde(default, alias = "DisplayName")]
    pub display_name: Option<String>,
    #[serde(default, alias = "DisplayNameToken")]
    pub display_name_token: Option<Value>,
    #[serde(default = "enabled_by_default", alias = "Enabled")]
    pub enabled: bool,
    #[serde(default, alias = "Condition")]
    pub condition: Option<String>,
    #[serde(default, alias = "ContinueOnError")]
    pub continue_on_error: Option<Value>,
    #[serde(default, alias = "TimeoutInMinutes")]
    pub timeout_in_minutes: Option<Value>,
    #[serde(default, alias = "ParallelGroupId")]
    pub parallel_group_id: Option<String>,
    #[serde(default, alias = "ContextName")]
    pub context_name: Option<String>,
    #[serde(default, alias = "Background")]
    pub background: bool,
    #[serde(default, alias = "ControlType")]
    pub control_type: Option<String>,
    #[serde(default, alias = "StepIds")]
    pub step_ids: Vec<String>,
    #[serde(default, alias = "Reference")]
    pub reference: Option<ActionStepDefinitionReference>,
    #[serde(default, alias = "Environment")]
    pub environment: Option<Value>,
    #[serde(default, alias = "Inputs")]
    pub inputs: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionStepDefinitionReference {
    #[serde(rename = "type", alias = "Type")]
    pub kind: ActionReferenceType,
    #[serde(default, alias = "Name")]
    pub name: Option<String>,
    #[serde(default, rename = "ref", alias = "Ref")]
    pub git_ref: Option<String>,
    #[serde(default, alias = "RepositoryType")]
    pub repository_type: Option<String>,
    #[serde(default, alias = "Path")]
    pub path: Option<String>,
    #[serde(default, alias = "Image")]
    pub image: Option<String>,
}

impl ActionStep {
    #[must_use]
    pub fn display_name_template(&self) -> Option<String> {
        self.display_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                self.display_name_token
                    .as_ref()
                    .and_then(display_token_template)
            })
            .filter(|name| !name.is_empty())
    }

    #[must_use]
    pub fn input_literal(&self, name: &str) -> Option<&str> {
        let pairs = member(self.inputs.as_ref()?.as_object()?, "map")?.as_array()?;
        pairs.iter().find_map(|pair| {
            let pair = pair.as_object()?;
            let key = token_literal(member(pair, "key")?)?;
            if key.eq_ignore_ascii_case(name) {
                token_literal(member(pair, "value")?)
            } else {
                None
            }
        })
    }

    #[must_use]
    pub fn script_text(&self) -> Option<&str> {
        let is_script = self
            .reference
            .as_ref()
            .is_some_and(|reference| reference.kind == ActionReferenceType::Script);
        if is_script {
            self.input_literal("script")
        } else {
            None
        }
    }
}

fn display_token_template(token: &Value) -> Option<String> {
    match token {
        Value::String(value) => Some(value.clone()),
        Value::Object(object) => {
            if let Some(expression) = member(object, "expr").and_then(Value::as_str) {
                return Some(format!("${{{{ {expression} }}}}"));
            }
            member(object, "lit").and_then(display_token_template)
        }
        _ => None,
    }
}
