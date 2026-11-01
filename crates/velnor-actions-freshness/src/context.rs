// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use syn::{Item, Lit, Type};

use crate::repo_file::read_regular;

pub(crate) const RUST_SOURCE_CAP: usize = 256 * 1024;
pub(crate) const FETCH_CAP: usize = 512 * 1024;

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
    pub(crate) cooldown_days: i64,
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
            cooldown_days: crate::inventory::DEFAULT_COOLDOWN_DAYS,
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

    pub(crate) fn rust_const(&mut self, path: &str, name: &str) -> Option<String> {
        self.rust_source(path, name, |file| {
            find_const(file, name).and_then(|item| extract_const_literal(item, name))
        })
    }

    pub(crate) fn rust_usize_const(&mut self, path: &str, name: &str) -> Option<String> {
        self.rust_source(path, name, |file| {
            find_const(file, name).and_then(|item| extract_usize_literal(item, name))
        })
    }

    pub(crate) fn rust_str_array_const(&mut self, path: &str, name: &str) -> Option<Vec<String>> {
        self.rust_source(path, name, |file| {
            find_const(file, name).and_then(|item| extract_str_array(item, name))
        })
    }

    pub(crate) fn rust_const_names(&mut self, path: &str) -> Option<Vec<String>> {
        self.rust_source(path, "*", |file| {
            Ok(file
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Const(item) => Some(item.ident.to_string()),
                    _ => None,
                })
                .collect())
        })
    }

    fn rust_source<T>(
        &mut self,
        path: &str,
        name: &str,
        read: impl FnOnce(&syn::File) -> Result<T, String>,
    ) -> Option<T> {
        match read_regular(&self.root, path, RUST_SOURCE_CAP)
            .and_then(|source| parse_source(&source))
            .and_then(|file| read(&file))
        {
            Ok(value) => Some(value),
            Err(error) => {
                self.fail_row("local-pin", &format!("{path}::{name}"), &error);
                None
            }
        }
    }

    pub(crate) fn finish(self, label: &str) -> i32 {
        print!("{}", self.output);
        if self.failures.is_empty() {
            println!("{label}: PASS");
            0
        } else {
            eprintln!("{label}: FAIL");
            for failure in self.failures {
                eprintln!("  - {failure}");
            }
            1
        }
    }
}

fn parse_source(source: &[u8]) -> Result<syn::File, String> {
    let text = std::str::from_utf8(source)
        .map_err(|error| format!("unsupported Rust source ({error})"))?;
    let file =
        syn::parse_file(text).map_err(|error| format!("unsupported Rust source ({error})"))?;
    if file.attrs.iter().any(is_conditional_attribute) {
        return Err("unsupported Rust source (conditional crate attribute)".to_owned());
    }
    Ok(file)
}

fn find_const<'a>(file: &'a syn::File, name: &str) -> Result<&'a syn::ItemConst, String> {
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
    Ok(item)
}

fn check_authority(item: &syn::ItemConst, name: &str) -> Result<(), String> {
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
    Ok(())
}

fn extract_const_literal(item: &syn::ItemConst, name: &str) -> Result<String, String> {
    check_authority(item, name)?;
    if !is_str_reference(item.ty.as_ref()) {
        return Err(format!("unsupported const {name} declaration"));
    }
    let syn::Expr::Lit(expression) = item.expr.as_ref() else {
        return Err(format!("const {name} must use one string literal"));
    };
    plain_string_literal(expression, name)
}

fn extract_str_array(item: &syn::ItemConst, name: &str) -> Result<Vec<String>, String> {
    check_authority(item, name)?;
    let Type::Array(array) = item.ty.as_ref() else {
        return Err(format!("unsupported const {name} declaration"));
    };
    if !is_str_reference(array.elem.as_ref()) {
        return Err(format!("unsupported const {name} declaration"));
    }
    let syn::Expr::Array(elements) = item.expr.as_ref() else {
        return Err(format!("const {name} must be an array of string literals"));
    };
    elements
        .elems
        .iter()
        .map(|element| match element {
            syn::Expr::Lit(expression) => plain_string_literal(expression, name),
            _ => Err(format!("const {name} must be an array of string literals")),
        })
        .collect()
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

fn plain_string_literal(expression: &syn::ExprLit, name: &str) -> Result<String, String> {
    let Lit::Str(value) = &expression.lit else {
        return Err(format!("const {name} must use string literals"));
    };
    Ok(value.value())
}

fn extract_usize_literal(item: &syn::ItemConst, name: &str) -> Result<String, String> {
    let invalid = || format!("const {name} must be one public decimal usize literal");
    if !matches!(&item.vis, syn::Visibility::Public(_))
        || item
            .attrs
            .iter()
            .any(|attribute| !attribute.path().is_ident("doc"))
    {
        return Err(invalid());
    }
    let Type::Path(path) = item.ty.as_ref() else {
        return Err(invalid());
    };
    if path.qself.is_some() || path.path.segments.len() != 1 || !path.path.is_ident("usize") {
        return Err(invalid());
    }
    let syn::Expr::Lit(expression) = item.expr.as_ref() else {
        return Err(invalid());
    };
    let Lit::Int(value) = &expression.lit else {
        return Err(invalid());
    };
    if !value.suffix().is_empty()
        || !value
            .to_string()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_')
    {
        return Err(invalid());
    }
    value
        .base10_parse::<usize>()
        .map(|parsed| parsed.to_string())
        .map_err(|_| invalid())
}

fn is_conditional_attribute(attribute: &syn::Attribute) -> bool {
    attribute.path().segments.first().is_some_and(|segment| {
        let name = segment.ident.to_string();
        matches!(name.trim_start_matches("r#"), "cfg" | "cfg_attr")
    })
}

pub(crate) use crate::time::{
    iso_date, iso_timestamp, norm_version, parse_iso_date, parse_timestamp,
};
