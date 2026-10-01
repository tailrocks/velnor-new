//! P11 structured manifest predicates (section-aware, not substring).
//!
//! Parses just enough TOML for policy questions: `[dotted.sections]`,
//! `key = value` with string/bool/inline-table values, dotted keys, and
//! bracket-balanced multi-line values. Comment-aware (`#` outside quotes).
//! Fail-closed: malformed lines are `Err`, never silently skipped.

use std::error::Error;

/// One section: header plus raw key/value pairs in file order.
pub(crate) struct Section {
    /// Dotted header (`package`, `workspace.lints.rust`); `""` pre-section.
    pub(crate) name: String,
    /// Raw pairs; values keep quotes/tables for typed accessors.
    pub(crate) pairs: Vec<(String, String)>,
}

/// Parsed manifest: sections in file order.
pub(crate) struct Manifest {
    /// Sections in file order.
    pub(crate) sections: Vec<Section>,
}

/// Strip a `#` comment starting outside quotes.
fn uncomment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let (mut single, mut double) = (false, false);
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\'' if !double => single = !single,
            b'"' if !single => double = !double,
            b'#' if !single && !double => return line.split_at(index).0.trim_end(),
            _ => {}
        }
    }
    line
}

/// Remove one matching single/double quote pair.
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

/// Join bracket-unbalanced lines so multi-line arrays parse as one value.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth: usize = 0;
    for raw in text.lines() {
        let segment = uncomment(raw).trim_end();
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(segment);
        depth += segment.bytes().filter(|b| *b == b'[' || *b == b'{').count();
        depth = depth.saturating_sub(segment.bytes().filter(|b| *b == b']' || *b == b'}').count());
        if depth == 0 {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Parse a manifest into sections; `Err` on any malformed line.
pub(crate) fn parse(text: &str) -> Result<Manifest, Box<dyn Error>> {
    let mut sections = vec![Section {
        name: String::new(),
        pairs: Vec::new(),
    }];
    for line in logical_lines(text) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let name = header
                .strip_suffix(']')
                .ok_or_else(|| format!("bad section: {line}"))?;
            let name = name.strip_prefix('[').unwrap_or(name);
            sections.push(Section {
                name: name.to_owned(),
                pairs: Vec::new(),
            });
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("no equals: {line}"))?;
        let section = sections.last_mut().ok_or("no section")?;
        section
            .pairs
            .push((key.trim().to_owned(), value.trim().to_owned()));
    }
    Ok(Manifest { sections })
}

/// First section with an exact header match.
pub(crate) fn section<'a>(doc: &'a Manifest, name: &str) -> Option<&'a Section> {
    doc.sections.iter().find(|s| s.name == name)
}

/// Value of `key` in `section`, unquoted.
pub(crate) fn value(section: &Section, key: &str) -> Option<String> {
    section
        .pairs
        .iter()
        .find(|pair| pair.0 == key)
        .map(|pair| unquote(&pair.1))
}

/// Inner `key = value` pairs of one inline `{...}` table (single level).
/// Splits top-level commas only, so `["a", "b"]` arrays stay intact.
pub(crate) fn inline_pairs(raw: &str) -> Vec<(String, String)> {
    let Some(inner) = raw
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for chr in inner.chars() {
        match chr {
            '[' => {
                depth += 1;
                current.push(chr);
            }
            ']' => {
                depth -= 1;
                current.push(chr);
            }
            ',' if depth == 0 => {
                push_inline_pair(&mut out, &current);
                current.clear();
            }
            _ => current.push(chr),
        }
    }
    push_inline_pair(&mut out, &current);
    out
}

/// Push one `key = value` part; malformed parts fail the dep, not the parse.
fn push_inline_pair(out: &mut Vec<(String, String)>, part: &str) {
    if let Some((key, val)) = part.split_once(['=', ':']) {
        out.push((key.trim().to_owned(), unquote(val.trim())));
    } else if !part.trim().is_empty() {
        out.push((part.trim().to_owned(), String::new()));
    }
}

/// Root `[workspace.package].edition` must be exactly `2024`.
pub(crate) fn workspace_edition(root: &Manifest) -> Result<String, Box<dyn Error>> {
    let section = section(root, "workspace.package").ok_or("no [workspace.package]")?;
    value(section, "edition").ok_or_else(|| "no workspace edition".into())
}

/// Member inherits edition: `edition.workspace = true`, no literal edition.
pub(crate) fn member_edition_inherited(member: &Manifest) -> bool {
    let Some(package) = section(member, "package") else {
        return false;
    };
    value(package, "edition.workspace").as_deref() == Some("true")
        && value(package, "edition").is_none()
}

/// Member inherits lints purely: `[lints] workspace = true`, no local table.
pub(crate) fn member_lints_inherited(member: &Manifest) -> bool {
    let Some(lints) = section(member, "lints") else {
        return false;
    };
    value(lints, "workspace").as_deref() == Some("true")
        && !member
            .sections
            .iter()
            .any(|s| s.name.starts_with("lints.") || s.name.starts_with("workspace.lints"))
}

/// One dependency entry is supported: exact pin, registry, no git shape.
fn dep_supported(raw: &str) -> bool {
    let val = raw.trim();
    if val.starts_with('{') {
        let inner = inline_pairs(val);
        if inner.iter().any(|pair| {
            pair.1.is_empty() || matches!(pair.0.as_str(), "git" | "branch" | "rev" | "tag")
        }) {
            return false;
        }
        return inner
            .iter()
            .all(|pair| pair.0 != "version" || pair.1.starts_with('=') || pair.1 == "workspace")
            || inner
                .iter()
                .any(|pair| pair.0 == "workspace" || pair.0 == "path");
    }
    if val.starts_with('"') || val.starts_with('\'') {
        return unquote(val).starts_with('=');
    }
    false
}

/// Every dependency entry in every dep section is supported.
pub(crate) fn member_deps_supported(member: &Manifest) -> bool {
    member
        .sections
        .iter()
        .filter(|s| {
            matches!(
                s.name.as_str(),
                "dependencies" | "dev-dependencies" | "build-dependencies"
            )
        })
        .flat_map(|s| s.pairs.iter())
        .all(|pair| dep_supported(&pair.1))
}

/// Read a repo-relative fixture file under this crate's `tests/fixtures/`.
pub(crate) fn fixture(name: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))?)
}

#[test]
fn workspace_edition_is_2024() -> Result<(), Box<dyn Error>> {
    let root = parse(&super::read("Cargo.toml")?)?;
    assert_eq!(workspace_edition(&root)?, "2024");
    Ok(())
}

#[test]
fn members_inherit_edition_without_literal() -> Result<(), Box<dyn Error>> {
    for (dir, _) in super::MEMBERS {
        let member = parse(&super::manifest(dir)?)?;
        assert!(
            member_edition_inherited(&member),
            "{dir} sets literal edition"
        );
    }
    Ok(())
}

#[test]
fn members_inherit_lints_purely() -> Result<(), Box<dyn Error>> {
    for (dir, _) in super::MEMBERS {
        let member = parse(&super::manifest(dir)?)?;
        assert!(
            member_lints_inherited(&member),
            "{dir} weakens lint inheritance"
        );
    }
    Ok(())
}

#[test]
fn members_use_supported_deps_only() -> Result<(), Box<dyn Error>> {
    for (dir, _) in super::MEMBERS {
        let member = parse(&super::manifest(dir)?)?;
        assert!(member_deps_supported(&member), "{dir} uses unsupported dep");
    }
    Ok(())
}

#[test]
fn reject_wrong_edition_fails() -> Result<(), Box<dyn Error>> {
    let doc = parse(&fixture("p11_manifest_edition_fail.toml")?)?;
    let package = section(&doc, "package").ok_or("no [package]")?;
    assert_eq!(value(package, "edition").as_deref(), Some("2021"));
    assert!(!member_edition_inherited(&doc), "wrong edition accepted");
    Ok(())
}

#[test]
fn reject_weakened_lints_fails() -> Result<(), Box<dyn Error>> {
    let doc = parse(&fixture("p11_manifest_lints_fail.toml")?)?;
    assert!(
        section(&doc, "lints.clippy").is_some(),
        "no local lint table"
    );
    assert!(!member_lints_inherited(&doc), "weakened lints accepted");
    Ok(())
}

#[test]
fn reject_git_dep_fails() -> Result<(), Box<dyn Error>> {
    let doc = parse(&fixture("p11_manifest_git_fail.toml")?)?;
    let deps = section(&doc, "dependencies").ok_or("no [dependencies]")?;
    assert!(deps.pairs.iter().any(|pair| pair.1.contains("git")));
    assert!(!member_deps_supported(&doc), "git dep accepted");
    Ok(())
}

#[test]
fn reject_wildcard_dep_fails() -> Result<(), Box<dyn Error>> {
    let doc = parse(&fixture("p11_manifest_wildcard_fail.toml")?)?;
    let deps = section(&doc, "dependencies").ok_or("no [dependencies]")?;
    assert!(deps.pairs.iter().any(|pair| unquote(&pair.1) == "*"));
    assert!(!member_deps_supported(&doc), "wildcard dep accepted");
    Ok(())
}
