//! Minimal `.alint.yml` structural parse for semantic policy checks.
//!
//! No YAML crate exists in `Cargo.lock` and manifests are frozen, so this
//! parses the alint subset in use instead of asserting on comment prose or
//! scraping `kind:` lines. Comment-aware (`#` outside quotes), indent-aware
//! (2-space norm; deeper nesting truncates generically), block and flow
//! (`[a, "b"]`) lists. Deliberate limits, all fail-closed via `Err`: no
//! backslash-escape processing, no multiline (`|`/`>`) scalars, `- key:`
//! items only at rule level, tab indents unsupported, duplicate keys keep first.

use std::error::Error;

/// Parsed rule: id plus dotted key/value pairs (`paths.include`, `command`).
pub(crate) struct AlintRule {
    /// Rule identifier from `- id:`.
    pub(crate) id: String,
    /// Dotted pairs; list items repeat their section key.
    pub(crate) pairs: Vec<(String, String)>,
}

/// Parsed config: schema version plus rule records in file order.
pub(crate) struct AlintConfig {
    /// Top-level `version` scalar.
    pub(crate) version: String,
    /// Rules in file order.
    pub(crate) rules: Vec<AlintRule>,
}

/// Expected policy row: exact kind, exact path sets, required pairs.
pub(crate) struct ExpectedRule {
    /// Rule identifier.
    pub(crate) id: &'static str,
    /// Required `kind`.
    pub(crate) kind: &'static str,
    /// Dotted path key plus its exact item set (order-insensitive).
    pub(crate) paths: &'static [(&'static str, &'static [&'static str])],
    /// Required exact pairs (limits, command markers).
    pub(crate) pairs: &'static [(&'static str, &'static str)],
}

impl AlintRule {
    /// Value of the `kind` scalar, empty when absent.
    pub(crate) fn kind(&self) -> &str {
        self.pairs
            .iter()
            .find(|pair| pair.0 == "kind")
            .map_or("", |pair| pair.1.as_str())
    }

    /// Value of the `level` scalar, empty when absent.
    pub(crate) fn level(&self) -> &str {
        self.pairs
            .iter()
            .find(|pair| pair.0 == "level")
            .map_or("", |pair| pair.1.as_str())
    }
}

/// Generic-only rule kinds. `command`, `file_max_size`, and `pair` stay
/// expressible so a future TOML-edition-style rule is not schema-locked out.
pub(crate) const ALLOWED_KINDS: [&str; 6] = [
    "command",
    "file_absent",
    "file_exists",
    "file_max_lines",
    "file_max_size",
    "pair",
];

/// Current policy snapshot: exact rule set plus per-rule pins.
pub(crate) const EXPECTED: [ExpectedRule; 7] = [
    ExpectedRule {
        id: "required-files",
        kind: "file_exists",
        paths: &[(
            "paths",
            &[
                "Cargo.toml",
                "Cargo.lock",
                "clippy.toml",
                "deny.toml",
                "rustfmt.toml",
                "CODEOWNERS",
                ".alint.yml",
                ".config/nextest.toml",
                "AGENTS.md",
            ],
        )],
        pairs: &[],
    },
    ExpectedRule {
        id: "claude-is-agents-symlink",
        kind: "command",
        paths: &[("paths", &["**/AGENTS.md"])],
        pairs: &[("command", "{path}")],
    },
    ExpectedRule {
        id: "agent-instructions-max-lines",
        kind: "file_max_lines",
        paths: &[("paths", &["**/AGENTS.md"])],
        pairs: &[("max_lines", "100")],
    },
    ExpectedRule {
        id: "agent-instructions-max-size",
        kind: "file_max_size",
        paths: &[("paths", &["**/AGENTS.md"])],
        pairs: &[("max_bytes", "16384")],
    },
    ExpectedRule {
        id: "crates-only",
        kind: "file_absent",
        paths: &[
            ("paths.include", &["**/*.rs"]),
            ("paths.exclude", &["crates/**"]),
        ],
        pairs: &[],
    },
    ExpectedRule {
        id: "rust-max-lines",
        kind: "file_max_lines",
        paths: &[
            (
                "paths.include",
                &[
                    "crates/velnor-actions-*/src/**/*.rs",
                    "crates/velnor-actions-*/tests/**/*.rs",
                    "crates/velnor-archive-guard/src/**/*.rs",
                    "crates/velnor-archive-guard/tests/**/*.rs",
                    "crates/velnor-runner/crates/*/src/**/*.rs",
                    "crates/velnor-runner/crates/*/tests/**/*.rs",
                ],
            ),
            ("paths.exclude", &["**/fixtures/**", "**/testdata/**"]),
        ],
        pairs: &[("max_lines", "400")],
    },
    ExpectedRule {
        id: "lib-main-max-lines",
        kind: "file_max_lines",
        paths: &[(
            "paths.include",
            &[
                "crates/velnor-actions-*/src/lib.rs",
                "crates/velnor-actions-*/src/main.rs",
                "crates/velnor-archive-guard/src/lib.rs",
                "crates/velnor-archive-guard/src/main.rs",
                "crates/velnor-runner/crates/*/src/lib.rs",
                "crates/velnor-runner/crates/*/src/main.rs",
            ],
        )],
        pairs: &[("max_lines", "150")],
    },
];

/// Strip a `#` comment starting outside quotes (YAML needs space before it).
fn uncomment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let (mut single, mut double) = (false, false);
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b'#' if !single
                && !double
                && (index == 0 || bytes[index - 1].is_ascii_whitespace()) =>
            {
                return line.split_at(index).0.trim_end();
            }
            _ => {}
        }
    }
    line
}

/// Remove one matching single/double quote pair; leave the rest untouched.
fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    for quote in ['\'', '"'] {
        if let Some(inner) = trimmed
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner.to_owned();
        }
    }
    trimmed.to_owned()
}

/// Split `key: value` on the first colon; values may hold further colons.
fn split_pair(line: &str) -> Result<(String, String), Box<dyn Error>> {
    let (key, value) = line
        .split_once(':')
        .ok_or_else(|| format!("no colon: {line}"))?;
    Ok((key.trim().to_owned(), unquote(value.trim())))
}

/// Split a `[...]` flow list on commas outside quotes; unquotes each item.
fn flow_items(text: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let inner = text
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| format!("bad flow list: {text}"))?;
    if inner.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    let mut current = String::new();
    let (mut single, mut double) = (false, false);
    for chr in inner.chars() {
        match chr {
            '\'' if !double => {
                single = !single;
                current.push(chr);
            }
            '"' if !single => {
                double = !double;
                current.push(chr);
            }
            ',' if !single && !double => {
                items.push(unquote(current.trim()));
                current.clear();
            }
            _ => current.push(chr),
        }
    }
    items.push(unquote(current.trim()));
    Ok(items)
}

/// Parse version plus rule records; non-`rules` sections are skipped.
pub(crate) fn parse(text: &str) -> Result<AlintConfig, Box<dyn Error>> {
    let mut version = String::new();
    let mut rules: Vec<AlintRule> = Vec::new();
    let mut section = String::new();
    let mut path: Vec<String> = Vec::new();
    for raw in text.lines() {
        let code = uncomment(raw);
        let indent = code.len() - code.trim_start().len();
        let line = code.trim();
        if line.is_empty() {
            continue;
        }
        if indent == 0 {
            let (key, value) = split_pair(line)?;
            section = key;
            path.clear();
            if section == "version" {
                version = value;
            }
            continue;
        }
        // Filtered `extends` nests past indent 2. `ignore` stays flat:
        // a deeper `ignore` entry is a parse error, not a silent skip.
        if section != "rules" {
            if section != "extends" && indent > 2 {
                return Err(format!("bad indent: {line}").into());
            }
            continue;
        }
        if indent == 2 {
            let Some(rest) = line.strip_prefix("- ") else {
                return Err(format!("bad indent: {line}").into());
            };
            path.clear();
            let (key, value) = split_pair(rest)?;
            if key != "id" {
                return Err(format!("expected `- id:`, saw {line}").into());
            }
            rules.push(AlintRule {
                id: value,
                pairs: Vec::new(),
            });
            continue;
        }
        if indent < 4 {
            return Err(format!("bad indent: {line}").into());
        }
        while path.len() + 2 > indent / 2 {
            path.pop();
        }
        let rule = rules
            .last_mut()
            .ok_or_else(|| format!("stray line: {line}"))?;
        if let Some(rest) = line.strip_prefix("- ") {
            if path.is_empty() {
                return Err(format!("stray item: {line}").into());
            }
            rule.pairs.push((path.join("."), unquote(rest.trim())));
        } else {
            let (key, value) = split_pair(line)?;
            let mut dotted = path.join(".");
            if !dotted.is_empty() {
                dotted.push('.');
            }
            dotted.push_str(&key);
            if value.is_empty() {
                path.push(key);
            } else if value.starts_with('[') {
                for entry in flow_items(&value)? {
                    rule.pairs.push((dotted.clone(), entry));
                }
            } else {
                rule.pairs.push((dotted, value));
            }
        }
    }
    Ok(AlintConfig { version, rules })
}

/// Find a rule by id.
pub(crate) fn rule<'a>(config: &'a AlintConfig, id: &str) -> Option<&'a AlintRule> {
    config.rules.iter().find(|rule| rule.id == id)
}

/// Read a per-rule fixture (`tests/fixtures/alint_<rule>_<verdict>.yml`).
pub(crate) fn fixture(rule: &str, verdict: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(format!(
        "{}/tests/fixtures/alint_{rule}_{verdict}.yml",
        env!("CARGO_MANIFEST_DIR")
    ))?)
}

/// Generic shape every rule must satisfy, independent of the id snapshot.
pub(crate) fn check_rule_shape(rule: &AlintRule) -> Result<(), Box<dyn Error>> {
    if rule.id.trim().is_empty() {
        return Err("rule without id".into());
    }
    if !ALLOWED_KINDS.contains(&rule.kind()) {
        return Err(format!("{} has kind {}", rule.id, rule.kind()).into());
    }
    if rule.level() != "error" {
        return Err(format!("{} level is {}", rule.id, rule.level()).into());
    }
    let needs = match rule.kind() {
        "file_max_lines" => "max_lines",
        "file_max_size" => "max_bytes",
        "command" => "command",
        _ => "",
    };
    if !needs.is_empty() && !rule.pairs.iter().any(|pair| pair.0 == needs) {
        return Err(format!("{} misses {needs}", rule.id).into());
    }
    if !rule
        .pairs
        .iter()
        .any(|pair| pair.0 == "paths" || pair.0.starts_with("paths."))
    {
        return Err(format!("{} has no paths", rule.id).into());
    }
    Ok(())
}

/// Full policy: schema version, exact rule set, and per-rule pins.
pub(crate) fn check_policy(config: &AlintConfig) -> Result<(), Box<dyn Error>> {
    if config.version != "1" {
        return Err(format!("version is {}, want 1", config.version).into());
    }
    let mut ids: Vec<&str> = config.rules.iter().map(|rule| rule.id.as_str()).collect();
    ids.sort_unstable();
    let mut want: Vec<&str> = EXPECTED.iter().map(|row| row.id).collect();
    want.sort_unstable();
    if ids != want {
        return Err(format!("rule ids {ids:?}, want {want:?}").into());
    }
    for rule in &config.rules {
        check_rule_shape(rule)?;
        let row = EXPECTED
            .iter()
            .find(|row| row.id == rule.id)
            .ok_or_else(|| format!("unknown rule {}", rule.id))?;
        if rule.kind() != row.kind {
            return Err(format!("{} kind is {}, want {}", rule.id, rule.kind(), row.kind).into());
        }
        for (key, values) in row.paths {
            let mut found: Vec<&str> = rule
                .pairs
                .iter()
                .filter(|pair| pair.0 == *key)
                .map(|pair| pair.1.as_str())
                .collect();
            found.sort_unstable();
            let mut want = values.to_vec();
            want.sort_unstable();
            if found != want {
                return Err(format!("{} {key} is {found:?}, want {want:?}", rule.id).into());
            }
        }
        for (key, value) in row.pairs {
            if !rule
                .pairs
                .iter()
                .any(|pair| pair.0 == *key && pair.1 == *value)
            {
                return Err(format!("{} misses {key} = {value}", rule.id).into());
            }
        }
    }
    Ok(())
}
