//! P11 resolver-grounded policy: `cargo metadata` assertions.
//!
//! The resolver — not manifest substrings — is the source of truth here:
//! effective edition and MSRV per package (inheritance resolved), the
//! exact eight-member set, and the locked dependency graph (registry-only
//! sources, exact requirements). Runs fully offline: any unlocked input
//! fails the command instead of fetching.

use std::error::Error;

const WORKSPACE_MANIFESTS: [&str; 2] = ["Cargo.toml", "crates/velnor-runner/Cargo.toml"];
pub(super) const RUNNER_MEMBERS: [(&str, &str); 8] = [
    (
        "crates/tools/velnor-runner-apparmor",
        "velnor-runner-apparmor",
    ),
    ("crates/tools/velnor-runner-cli", "velnor-runner-cli"),
    ("crates/tools/velnor-runner-core", "velnor-runner-core"),
    ("crates/tools/velnor-runner-github", "velnor-runner-github"),
    ("crates/tools/velnor-runner-host", "velnor-runner-host"),
    (
        "crates/tools/velnor-runner-journal",
        "velnor-runner-journal",
    ),
    ("crates/tools/velnor-runner-launch", "velnor-runner-launch"),
    (
        "crates/tools/velnor-runner-launch-slot",
        "velnor-runner-launch-slot",
    ),
];

/// Minimal JSON value: enough for `cargo metadata` documents.
#[derive(Debug)]
pub(crate) enum Json {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// Double-quoted string with escapes resolved.
    Str(String),
    /// Any numeric literal, kept raw.
    Num(String),
    /// Ordered array.
    Arr(Vec<Json>),
    /// Object pairs in document order.
    Obj(Vec<(String, Json)>),
}

/// Byte-cursor recursive-descent parser; `Err` on any malformed input.
struct Parser<'a> {
    /// Remaining input.
    rest: &'a str,
}

impl Parser<'_> {
    /// Consume ASCII whitespace.
    fn space(&mut self) {
        self.rest = self.rest.trim_start();
    }

    /// Consume one expected byte.
    fn byte(&mut self, want: u8) -> Result<(), Box<dyn Error>> {
        if self.rest.as_bytes().first() == Some(&want) {
            self.rest = &self.rest[1..];
            Ok(())
        } else {
            Err(format!("want {want:?} in {}", &self.rest[..32.min(self.rest.len())]).into())
        }
    }

    /// Parse any JSON value.
    fn value(&mut self) -> Result<Json, Box<dyn Error>> {
        self.space();
        let byte = self.rest.as_bytes().first().ok_or("empty value")?;
        match byte {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => Ok(Json::Str(self.string()?)),
            b't' if self.rest.starts_with("true") => {
                self.rest = &self.rest[4..];
                Ok(Json::Bool(true))
            }
            b'f' if self.rest.starts_with("false") => {
                self.rest = &self.rest[5..];
                Ok(Json::Bool(false))
            }
            b'n' if self.rest.starts_with("null") => {
                self.rest = &self.rest[4..];
                Ok(Json::Null)
            }
            _ => self.number(),
        }
    }

    /// Parse a `{...}` object.
    fn object(&mut self) -> Result<Json, Box<dyn Error>> {
        self.byte(b'{')?;
        let mut pairs = Vec::new();
        self.space();
        if self.rest.starts_with('}') {
            self.rest = &self.rest[1..];
            return Ok(Json::Obj(pairs));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.byte(b':')?;
            pairs.push((key, self.value()?));
            self.space();
            if self.rest.starts_with(',') {
                self.rest = &self.rest[1..];
                continue;
            }
            self.byte(b'}')?;
            return Ok(Json::Obj(pairs));
        }
    }

    /// Parse a `[...]` array.
    fn array(&mut self) -> Result<Json, Box<dyn Error>> {
        self.byte(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.rest.starts_with(']') {
            self.rest = &self.rest[1..];
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            if self.rest.starts_with(',') {
                self.rest = &self.rest[1..];
                continue;
            }
            self.byte(b']')?;
            return Ok(Json::Arr(items));
        }
    }

    /// Parse a double-quoted string with escape resolution.
    fn string(&mut self) -> Result<String, Box<dyn Error>> {
        self.byte(b'"')?;
        let mut out = String::new();
        let mut iter = self.rest.char_indices();
        while let Some((index, chr)) = iter.next() {
            match chr {
                '"' => {
                    self.rest = &self.rest[index + 1..];
                    return Ok(out);
                }
                '\\' => {
                    let (_, esc) = iter.next().ok_or("short escape")?;
                    match esc {
                        '"' | '\\' | '/' => out.push(esc),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => return Err("unicode escape unsupported".into()),
                        _ => return Err(format!("bad escape {esc:?}").into()),
                    }
                }
                _ => out.push(chr),
            }
        }
        Err("unterminated string".into())
    }

    /// Parse a numeric literal, kept raw.
    fn number(&mut self) -> Result<Json, Box<dyn Error>> {
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_digit() && !"+-.eE".contains(c))
            .unwrap_or(self.rest.len());
        if end == 0 {
            return Err("bad number".into());
        }
        let raw = self.rest[..end].to_owned();
        self.rest = &self.rest[end..];
        Ok(Json::Num(raw))
    }
}

/// Parse one document; trailing content is an error.
pub(crate) fn parse_json(text: &str) -> Result<Json, Box<dyn Error>> {
    let mut parser = Parser { rest: text };
    let value = parser.value()?;
    parser.space();
    if parser.rest.is_empty() {
        Ok(value)
    } else {
        Err("trailing content".into())
    }
}

impl Json {
    /// Object member lookup.
    fn get(&self, key: &str) -> Option<&Json> {
        if let Json::Obj(pairs) = self {
            pairs.iter().find(|pair| pair.0 == key).map(|pair| &pair.1)
        } else {
            None
        }
    }

    /// Borrow as string.
    fn as_str(&self) -> Option<&str> {
        if let Json::Str(text) = self {
            Some(text)
        } else {
            None
        }
    }

    /// Borrow as array.
    fn as_arr(&self) -> Option<&Vec<Json>> {
        if let Json::Arr(items) = self {
            Some(items)
        } else {
            None
        }
    }
}

/// Run locked offline metadata; offline proves the lockfile is complete.
pub(crate) fn metadata() -> Result<Json, Box<dyn Error>> {
    metadata_for("Cargo.toml")
}

/// Run locked offline metadata for one workspace manifest.
fn metadata_for(manifest: &str) -> Result<Json, Box<dyn Error>> {
    let output = std::process::Command::new("cargo")
        .args([
            "metadata",
            "--manifest-path",
            manifest,
            "--locked",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(super::repo_root())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    parse_json(&String::from_utf8(output.stdout)?)
}

/// Workspace packages (path-owned) with resolved identity fields.
fn workspace_packages(doc: &Json) -> Result<Vec<&Json>, Box<dyn Error>> {
    let members = doc
        .get("workspace_members")
        .and_then(Json::as_arr)
        .ok_or("no workspace_members")?;
    let packages = doc
        .get("packages")
        .and_then(Json::as_arr)
        .ok_or("no packages")?;
    let ids: Vec<&str> = members.iter().filter_map(Json::as_str).collect();
    Ok(packages
        .iter()
        .filter(|pkg| {
            pkg.get("id")
                .and_then(Json::as_str)
                .is_some_and(|id| ids.contains(&id))
        })
        .collect())
}

#[test]
fn json_parser_reads_every_shape() -> Result<(), Box<dyn Error>> {
    let doc = parse_json(r#"{"t": true, "f": false, "n": null, "i": 42, "s": "x"}"#)?;
    assert!(matches!(doc.get("t"), Some(Json::Bool(true))));
    assert!(matches!(doc.get("f"), Some(Json::Bool(false))));
    assert!(matches!(doc.get("n"), Some(Json::Null)));
    assert!(matches!(doc.get("i"), Some(Json::Num(raw)) if raw == "42"));
    assert_eq!(doc.get("s").and_then(Json::as_str), Some("x"));
    assert!(parse_json("{oops").is_err(), "malformed JSON accepted");
    Ok(())
}

#[test]
fn metadata_members_match_products_and_archive_guard() -> Result<(), Box<dyn Error>> {
    let doc = metadata()?;
    let mut names: Vec<&str> = workspace_packages(&doc)?
        .iter()
        .filter_map(|pkg| pkg.get("name").and_then(Json::as_str))
        .collect();
    names.sort_unstable();
    let mut want: Vec<&str> = super::MEMBERS
        .iter()
        .map(|dir| super::package(dir))
        .collect();
    want.push("velnor-archive-guard");
    want.sort_unstable();
    assert_eq!(names, want, "product and archive guard member set drift");
    Ok(())
}

#[test]
fn metadata_runner_members_match_four() -> Result<(), Box<dyn Error>> {
    let doc = metadata_for("crates/velnor-runner/Cargo.toml")?;
    let mut names: Vec<&str> = workspace_packages(&doc)?
        .iter()
        .filter_map(|pkg| pkg.get("name").and_then(Json::as_str))
        .collect();
    names.sort_unstable();
    let mut want: Vec<&str> = RUNNER_MEMBERS.iter().map(|(_, name)| *name).collect();
    want.sort_unstable();
    assert_eq!(names, want, "runner resolver member set drift");
    Ok(())
}

#[test]
fn metadata_effective_edition_is_2024() -> Result<(), Box<dyn Error>> {
    for manifest in WORKSPACE_MANIFESTS {
        for package in workspace_packages(&metadata_for(manifest)?)? {
            let name = package.get("name").and_then(Json::as_str).unwrap_or("?");
            let edition = package.get("edition").and_then(Json::as_str);
            assert_eq!(
                edition,
                Some("2024"),
                "{manifest}: {name} effective edition"
            );
        }
    }
    Ok(())
}

#[test]
fn metadata_msrv_resolves() -> Result<(), Box<dyn Error>> {
    for manifest in WORKSPACE_MANIFESTS {
        for package in workspace_packages(&metadata_for(manifest)?)? {
            let name = package.get("name").and_then(Json::as_str).unwrap_or("?");
            let rust_version = package.get("rust_version").and_then(Json::as_str);
            assert_eq!(
                rust_version,
                Some("1.98"),
                "{manifest}: {name} effective rust-version"
            );
        }
    }
    Ok(())
}

#[test]
fn metadata_locked_graph_is_registry_only() -> Result<(), Box<dyn Error>> {
    for manifest in WORKSPACE_MANIFESTS {
        assert_locked_graph_is_registry_only(&metadata_for(manifest)?, manifest)?;
    }
    Ok(())
}

fn assert_locked_graph_is_registry_only(doc: &Json, manifest: &str) -> Result<(), Box<dyn Error>> {
    const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";
    let resolve = doc
        .get("resolve")
        .ok_or_else(|| format!("no resolve graph in {manifest}: {doc:?}"))?;
    let nodes = resolve
        .get("nodes")
        .and_then(Json::as_arr)
        .ok_or("no nodes")?;
    assert!(!nodes.is_empty(), "{manifest}: empty locked graph");
    for node in nodes {
        let id = node.get("id").and_then(Json::as_str).unwrap_or("?");
        assert!(!id.contains("git+"), "{manifest}: {id} resolves from git");
    }
    for package in workspace_packages(doc)? {
        let name = package.get("name").and_then(Json::as_str).unwrap_or("?");
        let deps = package
            .get("dependencies")
            .and_then(Json::as_arr)
            .ok_or("no dependencies")?;
        for dep in deps {
            let source = dep.get("source").and_then(Json::as_str);
            let req = dep.get("req").and_then(Json::as_str).unwrap_or("?");
            assert!(req.starts_with('='), "{manifest}: {name} inexact req {req}");
            if let Some(source) = source {
                assert_eq!(source, REGISTRY, "{manifest}: {name} non-registry source");
            }
        }
    }
    Ok(())
}
