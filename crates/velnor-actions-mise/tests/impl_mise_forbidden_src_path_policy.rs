use std::collections::HashMap;

use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, Path, QSelf};

#[path = "impl_mise_forbidden_src_path_policy_imports.rs"]
mod imports;
#[path = "impl_mise_forbidden_src_path_policy_macros.rs"]
mod macros;
#[path = "impl_mise_forbidden_src_path_policy_qualified.rs"]
mod qualified;

const READ_ONLY_CALL: &str = "rustix::fs::open(path,rustix::fs::OFlags::RDONLY|rustix::fs::OFlags::NOFOLLOW|rustix::fs::OFlags::CLOEXEC,rustix::fs::Mode::empty(),)";

pub(super) fn check_path_openers(name: &str, source: &str, violations: &mut Vec<String>) {
    let syntax = match syn::parse_file(source) {
        Ok(syntax) => syntax,
        Err(error) => {
            violations.push(format!("{name}: Rust source parse failed: {error}"));
            return;
        }
    };
    let imports = imports::collect(&syntax);
    violations.extend(
        imports
            .violations
            .into_iter()
            .map(|problem| format!("{name}: {problem}")),
    );

    let mut audit = PathAudit {
        name,
        aliases: imports.aliases,
        ..PathAudit::default()
    };
    audit.visit_file(&syntax);
    if name == "check_file_read.rs" {
        let compact = super::code_of(source)
            .into_iter()
            .map(|(_, line)| line)
            .flat_map(|line| {
                line.split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect::<String>();
        if compact.matches(READ_ONLY_CALL).count() != 1 || audit.read_only_rustix_opens != 1 {
            audit
                .violations
                .push("expected one exact no-follow read-only Rustix file open".to_owned());
        }
    }
    violations.extend(
        audit
            .violations
            .into_iter()
            .map(|problem| format!("{name}: {problem}")),
    );
}

#[derive(Default)]
struct PathAudit<'a> {
    name: &'a str,
    aliases: HashMap<String, Vec<String>>,
    violations: Vec<String>,
    read_only_rustix_opens: usize,
}

impl PathAudit<'_> {
    fn inspect_call(&mut self, path: &Path, call: &ExprCall) {
        let canonical = resolve_path(path, &self.aliases);
        self.inspect_canonical_call(&canonical, exact_read_only_rustix_call(path, call));
    }

    fn inspect_qualified_call(&mut self, qself: &QSelf, path: &Path) {
        if let Some(canonical) = qualified::resolve_path(qself, path, &self.aliases) {
            self.inspect_canonical_call(&canonical, false);
        } else if qualified::is_file_operation(path) {
            self.violations
                .push("unresolved qualified file operation is forbidden".to_owned());
        }
    }

    fn inspect_canonical_call(&mut self, canonical: &[String], exact_read_only_rustix: bool) {
        if is_rustix_open(canonical) {
            if self.name == "check_file_read.rs" && exact_read_only_rustix {
                self.read_only_rustix_opens += 1;
            } else {
                self.violations.push(format!(
                    "path-opening API is not the exact checked read-only call: `{}`",
                    canonical.join("::")
                ));
            }
        } else if is_path_open_call(canonical) {
            if !is_standard_read_only_file_open(canonical) || self.name == "check_file_read.rs" {
                self.violations.push(format!(
                    "path-opening call is forbidden: `{}`",
                    canonical.join("::")
                ));
            }
        } else if is_dangerous_file_api(canonical) {
            self.violations.push(format!(
                "filesystem mutation call is forbidden: `{}`",
                canonical.join("::")
            ));
        } else if canonical.len() == 1
            && matches!(canonical[0].as_str(), "open" | "openat" | "openat2")
        {
            self.violations.push(format!(
                "unresolved path-opening call is forbidden: `{}`",
                canonical.join("::")
            ));
        }
    }

    fn inspect_path_reference(&mut self, path: &Path) {
        let canonical = resolve_path(path, &self.aliases);
        self.inspect_canonical_reference(&canonical);
    }

    fn inspect_qualified_reference(&mut self, qself: &QSelf, path: &Path) {
        if let Some(canonical) = qualified::resolve_path(qself, path, &self.aliases) {
            self.inspect_canonical_reference(&canonical);
        } else if qualified::is_file_operation(path) {
            self.violations
                .push("unresolved qualified file operation reference is forbidden".to_owned());
        }
    }

    fn inspect_canonical_reference(&mut self, canonical: &[String]) {
        if is_rustix_open(canonical)
            || (is_path_open_call(canonical) && !is_standard_read_only_file_open(canonical))
            || is_dangerous_file_api(canonical)
        {
            self.violations.push(format!(
                "path-opening or file-mutation function reference is forbidden: `{}`",
                canonical.join("::")
            ));
        }
    }
}

impl<'ast> Visit<'ast> for PathAudit<'_> {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(ExprPath { qself, path, .. }) = &*call.func {
            if let Some(qself) = qself {
                self.inspect_qualified_call(qself, path);
            } else {
                self.inspect_call(path, call);
            }
        } else {
            self.visit_expr(&call.func);
        }
        for argument in &call.args {
            self.visit_expr(argument);
        }
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        if let Some(qself) = &expression.qself {
            self.inspect_qualified_reference(qself, &expression.path);
        } else {
            self.inspect_path_reference(&expression.path);
        }
        visit::visit_expr_path(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if call.method == "open" {
            self.violations
                .push("method-based path open is forbidden".to_owned());
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let token_source = mac.tokens.to_string();
        if macros::contains_open_authority(&token_source, &self.aliases) {
            self.violations
                .push("macro tokens contain unknown file-opening authority".to_owned());
        }
        visit::visit_macro(self, mac);
    }
}

fn exact_read_only_rustix_call(path: &Path, call: &ExprCall) -> bool {
    path_is(path, &["rustix", "fs", "open"])
        && call.args.len() == 3
        && expr_path_is(&call.args[0], &["path"])
        && exact_read_only_flags(&call.args[1])
        && empty_mode_call(&call.args[2])
}

fn exact_read_only_flags(expression: &Expr) -> bool {
    let mut flags = Vec::new();
    collect_flag_paths(expression, &mut flags);
    flags
        == vec![
            vec!["rustix", "fs", "OFlags", "RDONLY"],
            vec!["rustix", "fs", "OFlags", "NOFOLLOW"],
            vec!["rustix", "fs", "OFlags", "CLOEXEC"],
        ]
}

fn collect_flag_paths(expression: &Expr, flags: &mut Vec<Vec<String>>) {
    match expression {
        Expr::Binary(binary) if matches!(binary.op, syn::BinOp::BitOr(_)) => {
            collect_flag_paths(&binary.left, flags);
            collect_flag_paths(&binary.right, flags);
        }
        Expr::Path(path) if path.qself.is_none() => {
            if path_is(&path.path, &["rustix", "fs", "OFlags", "RDONLY"]) {
                flags.push(
                    vec!["rustix", "fs", "OFlags", "RDONLY"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                );
            } else if path_is(&path.path, &["rustix", "fs", "OFlags", "NOFOLLOW"]) {
                flags.push(
                    vec!["rustix", "fs", "OFlags", "NOFOLLOW"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                );
            } else if path_is(&path.path, &["rustix", "fs", "OFlags", "CLOEXEC"]) {
                flags.push(
                    vec!["rustix", "fs", "OFlags", "CLOEXEC"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                );
            } else {
                flags.push(vec!["invalid".to_owned()]);
            }
        }
        _ => flags.push(vec!["invalid".to_owned()]),
    }
}

fn empty_mode_call(expression: &Expr) -> bool {
    let Expr::Call(call) = expression else {
        return false;
    };
    call.args.is_empty()
        && matches!(&*call.func, Expr::Path(path)
            if path.qself.is_none()
                && path_is(&path.path, &["rustix", "fs", "Mode", "empty"]))
}

fn expr_path_is(expression: &Expr, expected: &[&str]) -> bool {
    matches!(expression, Expr::Path(path)
        if path.qself.is_none() && path_is(&path.path, expected))
}

fn path_is(path: &Path, expected: &[&str]) -> bool {
    path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected)
}

fn resolve_path(path: &Path, aliases: &HashMap<String, Vec<String>>) -> Vec<String> {
    resolve_names(
        path.segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect(),
        aliases,
    )
}

fn resolve_names(mut path: Vec<String>, aliases: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut visited = Vec::new();
    for _ in 0..16 {
        let Some(first) = path.first() else {
            break;
        };
        if visited.contains(first) {
            break;
        }
        let Some(alias) = aliases.get(first) else {
            break;
        };
        visited.push(first.clone());
        let mut resolved = alias.clone();
        resolved.extend(path.iter().skip(1).cloned());
        path = resolved;
    }
    path
}

fn is_external_file_api_root(path: &[String]) -> bool {
    path.first()
        .is_some_and(|root| matches!(root.as_str(), "rustix" | "libc" | "nix"))
}

fn is_std_fs_path(path: &[String]) -> bool {
    path.len() >= 2 && path[0] == "std" && path[1] == "fs"
}

fn is_rustix_open(path: &[String]) -> bool {
    path.len() == 3
        && path[0] == "rustix"
        && path[1] == "fs"
        && matches!(path[2].as_str(), "open" | "openat" | "openat2")
}

fn is_path_open_call(path: &[String]) -> bool {
    if path.len() >= 2 && path[0] == "libc" && matches!(path[1].as_str(), "open" | "openat") {
        return true;
    }
    if path.len() >= 3
        && path[0] == "nix"
        && matches!(path[1].as_str(), "fcntl" | "unistd")
        && matches!(path[2].as_str(), "open" | "openat")
    {
        return true;
    }
    if is_std_fs_path(path) {
        return path
            .iter()
            .any(|segment| matches!(segment.as_str(), "write" | "openat" | "openat2"))
            || (path.len() >= 3
                && path[2] == "File"
                && path.get(3).is_some_and(|method| method == "create"));
    }
    path.len() >= 2
        && path
            .last()
            .is_some_and(|name| matches!(name.as_str(), "open" | "openat" | "openat2"))
}

fn is_standard_read_only_file_open(path: &[String]) -> bool {
    path.len() == 4 && path[0] == "std" && path[1] == "fs" && path[2] == "File" && path[3] == "open"
}

fn is_dangerous_file_api(path: &[String]) -> bool {
    if !is_std_fs_path(path) {
        return false;
    }
    path.iter().any(|segment| {
        matches!(
            segment.as_str(),
            "OpenOptions"
                | "write"
                | "create_dir"
                | "create_dir_all"
                | "rename"
                | "remove_file"
                | "remove_dir"
                | "remove_dir_all"
                | "copy"
                | "hard_link"
                | "set_permissions"
        )
    }) || (path.len() >= 4 && path[2] == "File" && matches!(path[3].as_str(), "create" | "options"))
}
