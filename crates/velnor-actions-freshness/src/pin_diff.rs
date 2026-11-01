// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod git;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;
use toml::Value as TomlValue;

use crate::context::FreshnessContext;
use crate::inventory::{INVENTORY_REL, POLICY_REL, parse_inventory, parse_policy};

use git::Blob;

const MAX_BASE_REF_LEN: usize = 200;
const ABSENT: &str = "<absent>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseRef(String);

impl BaseRef {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let allowed =
            |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'/' | b'-');
        if raw.is_empty() || raw.len() > MAX_BASE_REF_LEN {
            return Err(format!(
                "base ref must be 1 to {MAX_BASE_REF_LEN} characters"
            ));
        }
        if !raw.bytes().all(allowed) || raw.starts_with('-') || raw.contains("..") {
            return Err("base ref has unsupported characters".to_owned());
        }
        Ok(Self(raw.to_owned()))
    }

    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PinSet(BTreeMap<String, String>);

#[derive(Debug, PartialEq, Eq)]
enum PinChange {
    Added {
        subject: String,
        head: String,
    },
    Removed {
        subject: String,
        base: String,
    },
    Changed {
        subject: String,
        base: String,
        head: String,
    },
}

impl PinChange {
    fn subject(&self) -> &str {
        match self {
            Self::Added { subject, .. }
            | Self::Removed { subject, .. }
            | Self::Changed { subject, .. } => subject,
        }
    }

    fn detail(&self) -> String {
        match self {
            Self::Added { head, .. } => format!("base={ABSENT} head={head}"),
            Self::Removed { base, .. } => format!("base={base} head={ABSENT}"),
            Self::Changed { base, head, .. } => format!("base={base} head={head}"),
        }
    }
}

impl PinSet {
    fn project(policy: Option<&TomlValue>, inventory: &Value) -> Self {
        let mut pins = BTreeMap::new();
        let text = |value: &TomlValue| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        };
        if let Some(tools) = policy
            .and_then(|policy| policy.get("tools"))
            .and_then(TomlValue::as_table)
        {
            for (name, value) in tools {
                pins.insert(format!("policy tool {name}"), text(value));
            }
        }
        for entry in policy
            .and_then(|policy| policy.get("actions"))
            .and_then(TomlValue::as_array)
            .into_iter()
            .flatten()
        {
            let field = |key: &str| entry.get(key).and_then(TomlValue::as_str).unwrap_or("");
            pins.insert(
                format!("policy action {}", field("name")),
                format!("{}@{}", field("version"), field("sha")),
            );
        }
        if let Some(default) = policy
            .and_then(|policy| policy.get("github_runner_images"))
            .and_then(|images| images.get("linux_x64"))
            .and_then(|linux| linux.get("default"))
        {
            pins.insert("policy runner default".to_owned(), text(default));
        }
        for entry in json_entries(inventory, "tools") {
            let name = json_text(entry, "name");
            pins.insert(format!("inventory tool {name}"), json_text(entry, "pinned"));
        }
        for entry in json_entries(inventory, "actions") {
            pins.insert(
                format!("inventory action {}", json_text(entry, "key")),
                format!(
                    "{}@{}",
                    json_text(entry, "pinned_version"),
                    json_text(entry, "pinned_sha")
                ),
            );
        }
        Self(pins)
    }

    fn changes_from(&self, base: &Self) -> Vec<PinChange> {
        let subjects = self.0.keys().chain(base.0.keys()).collect::<BTreeSet<_>>();
        subjects
            .into_iter()
            .filter_map(|subject| match (base.0.get(subject), self.0.get(subject)) {
                (None, Some(head)) => Some(PinChange::Added {
                    subject: subject.clone(),
                    head: head.clone(),
                }),
                (Some(base), None) => Some(PinChange::Removed {
                    subject: subject.clone(),
                    base: base.clone(),
                }),
                (Some(base), Some(head)) if base != head => Some(PinChange::Changed {
                    subject: subject.clone(),
                    base: base.clone(),
                    head: head.clone(),
                }),
                _ => None,
            })
            .collect()
    }
}

fn json_entries<'a>(inventory: &'a Value, section: &str) -> impl Iterator<Item = &'a Value> {
    inventory
        .get(section)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn json_text(entry: &Value, key: &str) -> String {
    entry
        .get(key)
        .map(|value| {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        })
        .unwrap_or_default()
}

pub(crate) fn report(ctx: &mut FreshnessContext, base: &BaseRef) {
    let reference = base.as_str();
    let oid = match git::resolve_commit(&ctx.root, base) {
        Ok(oid) => oid,
        Err(error) => {
            ctx.fail_row(
                "pin-diff",
                reference,
                &format!("base ref unresolved ({error})"),
            );
            return;
        }
    };
    let base_pins = match base_pins(&ctx.root, &oid) {
        Ok(pins) => pins,
        Err(error) => {
            ctx.fail_row("pin-diff", reference, &format!("base unreadable ({error})"));
            return;
        }
    };
    let head_pins = PinSet::project(ctx.policy.as_ref(), &ctx.inv);
    let changes = head_pins.changes_from(&base_pins);
    if changes.is_empty() {
        ctx.info_row(
            "pin-diff",
            reference,
            &format!("unchanged vs {reference} ({oid})"),
        );
    }
    for change in changes {
        ctx.info_row("pin-diff", change.subject(), &change.detail());
    }
}

fn base_pins(root: &Path, oid: &str) -> Result<PinSet, String> {
    let policy = match git::read_blob(root, oid, POLICY_REL)? {
        Blob::Absent => None,
        Blob::Present(bytes) => {
            let text =
                String::from_utf8(bytes).map_err(|_| format!("{POLICY_REL} is not UTF-8"))?;
            Some(parse_policy(&text).map_err(|error| format!("{POLICY_REL}: {error}"))?)
        }
    };
    let inventory = match git::read_blob(root, oid, INVENTORY_REL)? {
        Blob::Absent => Value::Null,
        Blob::Present(bytes) => {
            parse_inventory(&bytes).map_err(|error| format!("{INVENTORY_REL}: {error}"))?
        }
    };
    Ok(PinSet::project(policy.as_ref(), &inventory))
}
