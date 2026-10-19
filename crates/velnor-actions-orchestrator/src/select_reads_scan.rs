//! Static scan of one Rust source file for reads outside its directory.
//!
//! Detected forms (each a [`SourceRead`]):
//!
//! - `include_str!`, `include_bytes!`, and `include!` whose argument is a
//!   string literal (relative to the file's directory) or
//!   `concat!(env!("CARGO_MANIFEST_DIR"), "...")` (relative to the
//!   package directory) resolve to an exact path; any other argument is
//!   unbounded. `env!("OUT_DIR")` includes read build output, not the
//!   repository, and are ignored.
//! - `#[path = "..."]` resolves relative to the file's directory.
//! - `concat!(env!("CARGO_MANIFEST_DIR"), "...")` anywhere resolves
//!   relative to the package directory.
//! - A file that names `CARGO_MANIFEST_DIR` or calls `current_dir` and
//!   also holds a string literal with a `..` component, or calls
//!   `parent`/`ancestors`, can reach any path: unbounded
//!   (`manifest_dir_escape`). A `--show-toplevel` literal (Git's repository
//!   root) is unbounded (`repository_root`).
//! - A literal whose first component is `..` passed directly to a file
//!   system call (`read`, `read_to_string`, `read_dir`, `open`,
//!   `metadata`, `symlink_metadata`, `canonicalize`, `copy`) resolves
//!   relative to the package directory (Cargo runs tests and build scripts
//!   there).
//!
//! Limits: reads through computed paths without any of these anchors
//! (an environment variable other than `CARGO_MANIFEST_DIR`, a path built
//! from a non-literal, a spawned process, a non-Rust file) are invisible.

use crate::select_reads_lex::{Token, tokenize};

/// Include macros whose argument names a file the compiler reads.
const INCLUDE_MACROS: &[&str] = &["include_str", "include_bytes", "include"];

/// File system calls whose first literal argument is a cwd-relative read.
const FS_CALLS: &[&str] = &[
    "read",
    "read_to_string",
    "read_dir",
    "open",
    "metadata",
    "symlink_metadata",
    "canonicalize",
    "copy",
];

/// Where one detected read lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadTarget {
    /// Repository-relative path the read resolves to.
    Path(String),
    /// The read resolves outside the repository: no change set reaches it.
    Outside,
    /// The read can reach paths the scan cannot bound; the reason token.
    Unbounded(&'static str),
}

/// One detected read in one source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceRead {
    /// Repository-relative source file holding the read.
    pub(crate) file: String,
    /// Where the read lands.
    pub(crate) target: ReadTarget,
}

/// Every read `text` (the file at `file`, inside `package_dir`) performs.
pub(crate) fn scan_source(file: &str, package_dir: &str, text: &str) -> Vec<SourceRead> {
    let tokens = tokenize(text);
    let file_dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut scan = Scan {
        tokens: &tokens,
        file_dir,
        package_dir,
        targets: Vec::new(),
        anchored: false,
        escapes: false,
    };
    scan.run();
    let mut targets = scan.targets;
    if scan.anchored && scan.escapes {
        targets.push(ReadTarget::Unbounded("manifest_dir_escape"));
    }
    targets
        .into_iter()
        .map(|target| SourceRead {
            file: file.to_owned(),
            target,
        })
        .collect()
}

/// Token walk state for one file.
struct Scan<'a> {
    tokens: &'a [Token],
    file_dir: &'a str,
    package_dir: &'a str,
    targets: Vec<ReadTarget>,
    anchored: bool,
    escapes: bool,
}

impl Scan<'_> {
    fn run(&mut self) {
        let mut at = 0;
        let mut attribute_depth = 0usize;
        while let Some(token) = self.tokens.get(at) {
            match token {
                Token::Ident(word) if self.is_macro(at) && INCLUDE_MACROS.contains(&word.as_str()) => {
                    let (args, next) = group(self.tokens, at + 2);
                    if let Some(target) = self.include_target(args) {
                        self.targets.push(target);
                    }
                    at = next;
                    continue;
                }
                Token::Ident(word) if word == "concat" && self.is_macro(at) => {
                    let (args, next) = group(self.tokens, at + 2);
                    if let Some(Some(target)) = self.concat_target(args) {
                        self.targets.push(target);
                        at = next;
                        continue;
                    }
                }
                Token::Ident(word) if FS_CALLS.contains(&word.as_str()) => self.fs_call(at),
                Token::Ident(word) if word == "path" && attribute_depth > 0 => self.path_attribute(at),
                Token::Ident(word) if word == "current_dir" => self.anchored = true,
                Token::Ident(word) if word == "parent" || word == "ancestors" => self.escapes = true,
                Token::Str(body) => self.literal(body),
                Token::Punct('[') if attribute_depth > 0 || self.opens_attribute(at) => {
                    attribute_depth += 1;
                }
                Token::Punct(']') if attribute_depth > 0 => attribute_depth -= 1,
                _ => {}
            }
            at += 1;
        }
    }

    /// True when the identifier at `at` is followed by `!(`.
    fn is_macro(&self, at: usize) -> bool {
        self.tokens.get(at + 1) == Some(&Token::Punct('!'))
            && self.tokens.get(at + 2) == Some(&Token::Punct('('))
    }

    /// True when the `[` at `at` follows `#` or `#!`.
    fn opens_attribute(&self, at: usize) -> bool {
        let before = |back: usize| at.checked_sub(back).and_then(|index| self.tokens.get(index));
        before(1) == Some(&Token::Punct('#'))
            || (before(1) == Some(&Token::Punct('!')) && before(2) == Some(&Token::Punct('#')))
    }

    /// Anchors and escape components carried by one literal.
    fn literal(&mut self, body: &str) {
        if body == "CARGO_MANIFEST_DIR" {
            self.anchored = true;
        } else if body.contains("--show-toplevel") {
            self.targets.push(ReadTarget::Unbounded("repository_root"));
        } else if body.split('/').any(|component| component == "..") {
            self.escapes = true;
        }
    }

    /// Target of an include macro's argument tokens; `None` when ignored.
    fn include_target(&self, args: &[Token]) -> Option<ReadTarget> {
        match args {
            [Token::Str(body)] => Some(resolve_literal(self.file_dir, body)),
            [Token::Ident(word), Token::Punct('!'), Token::Punct('('), inner @ .., Token::Punct(')')]
                if word == "concat" =>
            {
                self.concat_target(inner)
                    .unwrap_or(Some(ReadTarget::Unbounded("include_argument")))
            }
            _ => Some(ReadTarget::Unbounded("include_argument")),
        }
    }

    /// `concat!` parts: `Some(Some(target))` resolved, `Some(None)` build
    /// output, `None` when not anchored at the manifest directory.
    fn concat_target(&self, args: &[Token]) -> Option<Option<ReadTarget>> {
        let parts: Vec<&[Token]> = args.split(|token| *token == Token::Punct(',')).collect();
        let (first, rest) = parts.split_first()?;
        let base = match env_name(first)? {
            "CARGO_MANIFEST_DIR" => self.package_dir,
            "OUT_DIR" => return Some(None),
            _ => return None,
        };
        let mut tail = String::new();
        for part in rest {
            match part {
                [Token::Str(body)] => tail.push_str(body),
                [] => {}
                _ => return Some(Some(ReadTarget::Unbounded("include_argument"))),
            }
        }
        Some(Some(resolve(base, &tail)))
    }

    /// `path = "..."` inside an attribute.
    fn path_attribute(&mut self, at: usize) {
        if self.tokens.get(at + 1) == Some(&Token::Punct('=')) {
            if let Some(Token::Str(body)) = self.tokens.get(at + 2) {
                self.targets.push(resolve_literal(self.file_dir, body));
            }
        }
    }

    /// `read("../x")`-style call: a cwd-relative read from the package dir.
    fn fs_call(&mut self, at: usize) {
        if self.tokens.get(at + 1) != Some(&Token::Punct('(')) {
            return;
        }
        if let Some(Token::Str(body)) = self.tokens.get(at + 2) {
            if body.split('/').next() == Some("..") {
                self.targets.push(resolve(self.package_dir, body));
            }
        }
    }
}

/// Tokens inside the parenthesized group opening at `open`, plus the index after it.
fn group(tokens: &[Token], open: usize) -> (&[Token], usize) {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (tokens.get(open + 1..index).unwrap_or_default(), index + 1);
                }
            }
            _ => {}
        }
    }
    (tokens.get(open + 1..).unwrap_or_default(), tokens.len())
}

/// Variable name of an `env!("NAME")` part.
fn env_name(part: &[Token]) -> Option<&str> {
    match part {
        [
            Token::Ident(word),
            Token::Punct('!'),
            Token::Punct('('),
            Token::Str(name),
            Token::Punct(')'),
        ] if word == "env" => Some(name),
        _ => None,
    }
}

/// Resolve a literal path: absolute paths are outside the repository.
fn resolve_literal(dir: &str, literal: &str) -> ReadTarget {
    if literal.starts_with('/') {
        ReadTarget::Outside
    } else {
        resolve(dir, literal)
    }
}

/// Resolve `relative` against repository-relative directory `dir`.
///
/// Empty and `.` components are skipped, so a manifest-dir `concat!` tail
/// (`"/../x"`) joins like a relative path.
pub(crate) fn resolve(dir: &str, relative: &str) -> ReadTarget {
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for component in relative.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return ReadTarget::Outside;
                }
            }
            other => parts.push(other),
        }
    }
    ReadTarget::Path(parts.join("/"))
}

#[cfg(test)]
#[path = "select_reads_scan_tests.rs"]
mod tests;
