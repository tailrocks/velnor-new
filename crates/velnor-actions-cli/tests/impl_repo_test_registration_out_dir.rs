//! Static binding for the one generated-source include form Cargo supports.
//!
//! `include!(concat!(env!("OUT_DIR"), "/<file>.rs"))` reads a file the package's
//! build script writes into Cargo's per-package output directory. That file is
//! build output, never a repository source, so it cannot be an unregistered
//! repository test file; the scan accepts the edge without following it. Cargo
//! sets `OUT_DIR` only for a package with a build script, so the edge is valid
//! only when Cargo metadata reports one. Any other non-literal include stays
//! fail-closed.

use std::error::Error;
use std::path::{Component, Path, PathBuf};

use proc_macro2::TokenStream;
use syn::parse::Parser;
use syn::{Expr, Lit, LitStr, MacroDelimiter};

use super::IncludePath;

type Outcome<T> = Result<T, Box<dyn Error>>;

/// Resolve a direct item-level `include!` argument.
pub(super) fn direct_include_path(tokens: &TokenStream, source: &Path) -> Outcome<IncludePath> {
    let expression: Expr = syn::parse2(tokens.clone())?;
    match expression {
        Expr::Lit(expression) => match expression.lit {
            Lit::Str(literal) => Ok(IncludePath::Relative(
                source
                    .parent()
                    .ok_or("include source has no parent")?
                    .join(literal.value()),
            )),
            _ => Err("include! source is not a string literal".into()),
        },
        Expr::Macro(expression)
            if expression.mac.path.is_ident("concat")
                && matches!(expression.mac.delimiter, MacroDelimiter::Paren(_)) =>
        {
            cargo_out_dir_suffix(&expression.mac.tokens).map(IncludePath::CargoOutDir)
        }
        _ => Err("include! source must be a literal or Cargo OUT_DIR concat".into()),
    }
}

fn cargo_out_dir_suffix(tokens: &TokenStream) -> Outcome<PathBuf> {
    let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
    let mut arguments = parser.parse2(tokens.clone())?.into_iter();
    let Some(Expr::Macro(directory)) = arguments.next() else {
        return Err("OUT_DIR concat must start with env!(\"OUT_DIR\")".into());
    };
    if !directory.mac.path.is_ident("env")
        || !matches!(directory.mac.delimiter, MacroDelimiter::Paren(_))
    {
        return Err("OUT_DIR concat must start with env!(\"OUT_DIR\")".into());
    }
    let name: LitStr = syn::parse2(directory.mac.tokens)?;
    if name.value() != "OUT_DIR" {
        return Err("OUT_DIR concat references an unexpected environment variable".into());
    }
    let mut suffix = String::new();
    let mut count = 0;
    for argument in arguments {
        let Expr::Lit(expression) = argument else {
            return Err("OUT_DIR concat suffix must contain only string literals".into());
        };
        let Lit::Str(literal) = expression.lit else {
            return Err("OUT_DIR concat suffix must contain only string literals".into());
        };
        suffix.push_str(&literal.value());
        count += 1;
    }
    if count == 0 {
        return Err("OUT_DIR concat has no relative suffix".into());
    }
    safe_out_dir_suffix(&suffix)
}

fn safe_out_dir_suffix(suffix: &str) -> Outcome<PathBuf> {
    let relative = suffix
        .strip_prefix('/')
        .filter(|relative| !relative.is_empty() && !relative.starts_with('/'))
        .ok_or("OUT_DIR concat suffix must start with one path separator")?;
    let path = Path::new(relative);
    if relative.contains(['\\', ':', '\0'])
        || relative.contains("//")
        || relative.ends_with('/')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || path.extension().is_none_or(|extension| extension != "rs")
    {
        return Err("OUT_DIR concat suffix is not a safe relative Rust source path".into());
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{IncludePath, Outcome, direct_include_path};

    fn parse_include(expression: &str) -> Outcome<IncludePath> {
        direct_include_path(&expression.parse()?, Path::new("tests/root.rs"))
    }

    #[test]
    fn direct_out_dir_concat_requires_a_safe_literal_suffix() -> Outcome<()> {
        assert!(matches!(
            parse_include("concat!(env!(\"OUT_DIR\"), \"/compile_identity.rs\")")?,
            IncludePath::CargoOutDir(path) if path == Path::new("compile_identity.rs")
        ));
        assert!(matches!(
            parse_include("concat!(env!(\"OUT_DIR\"), \"/nested/\", \"generated.rs\")")?,
            IncludePath::CargoOutDir(path) if path == Path::new("nested/generated.rs")
        ));
        for expression in [
            "concat!(env!(\"OUT_DIR\"), \"../escape.rs\")",
            "concat!(env!(\"OUT_DIR\"), \"/../escape.rs\")",
            "concat!(env!(\"OUT_DIR\"), env!(\"SOURCE\"))",
            "concat!(env!(\"OTHER\"), \"/generated.rs\")",
            "concat!(env!(\"OUT_DIR\"), \"/generated.txt\")",
            "concat!(env!(\"OUT_DIR\"), \"/generated.rs\", suffix)",
            "concat!(env!(\"OUT_DIR\"), \"//generated.rs\")",
            "concat!(env!(\"OUT_DIR\"))",
            "concat!{env!(\"OUT_DIR\"), \"/generated.rs\"}",
            "concat!(env!{\"OUT_DIR\"}, \"/generated.rs\")",
            "concat!(\"/generated.rs\")",
            "env!(\"OUT_DIR\")",
            "42",
        ] {
            assert!(parse_include(expression).is_err(), "accepted {expression}");
        }
        assert!(matches!(
            parse_include("\"included.rs\"")?,
            IncludePath::Relative(path) if path == Path::new("tests/included.rs")
        ));
        Ok(())
    }
}
