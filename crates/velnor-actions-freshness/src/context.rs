//! Shared state, strict time parsing, row output, and literal pin extraction.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use syn::{Item, Lit, Type};

pub(crate) const RUST_SOURCE_CAP: usize = 256 * 1024;
pub(crate) const FETCH_CAP: usize = 512 * 1024;

/// One policy run over a repository or fixture root.
#[derive(Debug)]
pub(crate) struct FreshnessContext {
    pub(crate) root: PathBuf,
    pub(crate) check_upstream: bool,
    pub(crate) with_advisories: bool,
    pub(crate) now: i64,
    pub(crate) output: String,
    pub(crate) failures: Vec<String>,
    pub(crate) inv: Value,
    pub(crate) interval: i64,
    pub(crate) max_days: i64,
    pub(crate) top_checked: Option<String>,
    pub(crate) policy: Option<toml::Value>,
    pub(crate) tools: Vec<Value>,
    pub(crate) action_pinned: BTreeMap<String, Value>,
    pub(crate) tool_pinned: BTreeMap<String, Value>,
    pub(crate) runner: Value,
    pub(crate) supported: Vec<String>,
    pub(crate) locked: Vec<toml::Value>,
    pub(crate) member_names: BTreeSet<String>,
    pub(crate) workspace_roots: Vec<(String, toml::Value)>,
    pub(crate) holds: Vec<Value>,
    pub(crate) hold_keys: BTreeSet<String>,
}

impl FreshnessContext {
    /// Create a context with the repository's current UTC time.
    #[must_use]
    pub(crate) fn new(root: PathBuf, check_upstream: bool, with_advisories: bool) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
            });
        Self {
            root,
            check_upstream,
            with_advisories,
            now,
            output: String::new(),
            failures: Vec::new(),
            inv: Value::Null,
            interval: 24,
            max_days: 14,
            top_checked: None,
            policy: None,
            tools: Vec::new(),
            action_pinned: BTreeMap::new(),
            tool_pinned: BTreeMap::new(),
            runner: Value::Null,
            supported: Vec::new(),
            locked: Vec::new(),
            member_names: BTreeSet::new(),
            workspace_roots: Vec::new(),
            holds: Vec::new(),
            hold_keys: BTreeSet::new(),
        }
    }

    /// Resolve a repository-relative path.
    #[must_use]
    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub(crate) fn row(&mut self, check: &str, subject: &str, status: &str, detail: &str) {
        let mut fields = Map::new();
        fields.insert("check".to_owned(), json!(check));
        fields.insert("detail".to_owned(), json!(detail));
        fields.insert("status".to_owned(), json!(status));
        fields.insert("subject".to_owned(), json!(subject));
        if let Ok(payload) = serde_json::to_string(&Value::Object(fields)) {
            self.output.push_str("row: ");
            self.output.push_str(&payload);
            self.output.push('\n');
        }
    }

    pub(crate) fn fail_row(&mut self, check: &str, subject: &str, detail: &str) {
        self.row(check, subject, "fail", detail);
        self.failures.push(format!("{check} {subject}: {detail}"));
    }

    pub(crate) fn pass_row(&mut self, check: &str, subject: &str, detail: &str) {
        self.row(check, subject, "pass", detail);
        self.output.push_str("ok: ");
        self.output.push_str(check);
        self.output.push(' ');
        self.output.push_str(subject);
        self.output.push(' ');
        self.output.push_str(detail);
        self.output.push('\n');
    }

    pub(crate) fn info_row(&mut self, check: &str, subject: &str, detail: &str) {
        self.row(check, subject, "info", detail);
    }

    /// Extract a single unconditional `pub const NAME: &str = "..."`.
    pub(crate) fn rust_const(&mut self, path: &str, name: &str) -> Option<String> {
        let relative = format!("{path}::{name}");
        match read_capped(&self.path(path), RUST_SOURCE_CAP)
            .and_then(|source| parse_rust_const(&source, name))
        {
            Ok(value) => Some(value),
            Err(error) => {
                self.fail_row("local-pin", &relative, &error);
                None
            }
        }
    }

    /// Print accumulated machine rows and human summary; return process code.
    pub(crate) fn finish(self) -> i32 {
        print!("{}", self.output);
        if self.failures.is_empty() {
            println!("check-freshness: PASS");
            0
        } else {
            eprintln!("check-freshness: FAIL");
            for failure in self.failures {
                eprintln!("  - {failure}");
            }
            1
        }
    }
}

fn parse_rust_const(source: &[u8], name: &str) -> Result<String, String> {
    if source.len() > RUST_SOURCE_CAP {
        return Err(format!("source exceeds {RUST_SOURCE_CAP} bytes"));
    }
    let text = std::str::from_utf8(source)
        .map_err(|error| format!("unsupported Rust source ({error})"))?;
    let file =
        syn::parse_file(text).map_err(|error| format!("unsupported Rust source ({error})"))?;
    if file.attrs.iter().any(is_conditional_attribute) {
        return Err("unsupported Rust source (conditional crate attribute)".to_owned());
    }
    let mut constants = file.items.iter().filter_map(|item| match item {
        Item::Const(item) if item.ident == name => Some(item),
        _ => None,
    });
    let item = constants
        .next()
        .ok_or_else(|| format!("missing const {name}"))?;
    if constants.next().is_some() {
        return Err(format!("duplicate const {name}"));
    }
    extract_const_literal(item, name)
}

fn extract_const_literal(item: &syn::ItemConst, name: &str) -> Result<String, String> {
    if !matches!(&item.vis, syn::Visibility::Public(_)) {
        return Err(format!("unsupported const {name} visibility"));
    }
    if item
        .attrs
        .iter()
        .any(|attribute| !attribute.path().is_ident("doc"))
    {
        return Err("unsupported authority attribute".to_owned());
    }
    if !is_str_reference(item.ty.as_ref()) {
        return Err(format!("unsupported const {name} declaration"));
    }
    plain_string_literal(item, name)
}

fn is_str_reference(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    if reference.lifetime.is_some() || reference.mutability.is_some() {
        return false;
    }
    let Type::Path(path) = reference.elem.as_ref() else {
        return false;
    };
    path.qself.is_none() && path.path.segments.len() == 1 && path.path.is_ident("str")
}

fn plain_string_literal(item: &syn::ItemConst, name: &str) -> Result<String, String> {
    let invalid = || format!("const {name} must use one string literal");
    let syn::Expr::Lit(expression) = item.expr.as_ref() else {
        return Err(invalid());
    };
    let Lit::Str(value) = &expression.lit else {
        return Err(invalid());
    };
    Ok(value.value())
}

fn is_conditional_attribute(attribute: &syn::Attribute) -> bool {
    attribute.path().segments.first().is_some_and(|segment| {
        let name = segment.ident.to_string();
        matches!(name.trim_start_matches("r#"), "cfg" | "cfg_attr")
    })
}

fn read_capped(path: &Path, cap: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("unreadable ({error})"))?;
    let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("unreadable ({error})"))?;
    Ok(bytes)
}

pub(crate) use crate::time::{
    iso_date, iso_timestamp, norm_version, parse_iso_date, parse_timestamp,
};
