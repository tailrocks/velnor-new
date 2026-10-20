use std::collections::HashMap;

use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprMethodCall, ExprPath, Path, QSelf};

#[path = "impl_mise_forbidden_src_path_policy_executable.rs"]
mod executable;
#[path = "impl_mise_forbidden_src_path_policy_imports.rs"]
mod imports;
#[path = "impl_mise_forbidden_src_path_policy_macros.rs"]
mod macros;
#[path = "impl_mise_forbidden_src_path_policy_qualified.rs"]
mod qualified;

const READ_ONLY_CALL: &str = "rustix::fs::open(path,rustix::fs::OFlags::RDONLY|rustix::fs::OFlags::NOFOLLOW|rustix::fs::OFlags::CLOEXEC,rustix::fs::Mode::empty(),)";
const EXECUTABLE_LOOKUP: &str = "requests_executable_lookup.rs";

pub(super) fn check_path_openers(name: &str, source: &str, violations: &mut Vec<String>) {
    let syntax = match syn::parse_file(source) {
        Ok(syntax) => syntax,
        Err(error) => {
            violations.push(format!("{name}: Rust source parse failed: {error}"));
            return;
        }
    };
    let imports = imports::collect(name, &syntax);
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
    if name == EXECUTABLE_LOOKUP
        && (audit.executable_lookup_opens != 1
            || audit.executable_lookup_directory_openats != 2
            || audit.executable_lookup_file_openats != 1)
    {
        audit.violations.push(
            "The scan must find one open of the pinned root and three exact no-follow opens of the executable path."
                .to_owned(),
        );
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
    executable_lookup_opens: usize,
    executable_lookup_directory_openats: usize,
    executable_lookup_file_openats: usize,
}

impl PathAudit<'_> {
    fn inspect_call(&mut self, path: &Path, call: &ExprCall) {
        let canonical = resolve_path(path, &self.aliases);
        let checked_call = executable::exact_read_only_rustix_call(self.name, &canonical, call);
        if self.name == EXECUTABLE_LOOKUP
            && executable::canonical_is(&canonical, &["rustix", "fs", "openat"])
        {
            if executable::exact_openat_with_flags(
                call,
                &["RDONLY", "DIRECTORY", "NOFOLLOW", "CLOEXEC"],
            ) {
                self.executable_lookup_directory_openats += 1;
            } else if executable::exact_openat_with_flags(
                call,
                &["RDONLY", "NOFOLLOW", "NONBLOCK", "CLOEXEC"],
            ) {
                self.executable_lookup_file_openats += 1;
            }
        }
        self.inspect_canonical_call(&canonical, checked_call);
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
            } else if self.name == EXECUTABLE_LOOKUP && exact_read_only_rustix {
                match canonical[2].as_str() {
                    "open" => self.executable_lookup_opens += 1,
                    "openat" => {}
                    _ => self.violations.push(
                        "The scanner does not allow this Rustix open operation for executable lookup.".to_owned(),
                    ),
                }
            } else {
                self.violations.push(format!(
                    "path-opening API is not the exact checked read-only call: `{}`",
                    canonical.join("::")
                ));
            }
        } else if executable::is_test_fixture_mutation(self.name, canonical) {
            // This source file creates temporary inputs for the tests.
        } else if is_path_open_call(canonical) {
            if !is_standard_read_only_file_open(canonical)
                || matches!(self.name, "check_file_read.rs" | EXECUTABLE_LOOKUP)
            {
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
        if executable::is_test_fixture_mutation(self.name, canonical) {
            return;
        }
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
    if is_standard_read_only_file_open(path) {
        return true;
    }
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
    if path
        .last()
        .is_some_and(|name| matches!(name.as_str(), "symlink" | "symlinkat"))
    {
        return true;
    }
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
