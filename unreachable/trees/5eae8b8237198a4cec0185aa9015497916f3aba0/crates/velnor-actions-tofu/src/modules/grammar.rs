//! S6 module-source grammar: declarations, classification, errors.
//!
//! A literal source is local if and only if it starts with `./` or
//! `../` (strict: bare `.`/`..` never count — `OpenTofu` requires
//! the slashed prefixes for local paths). Absolute paths are
//! external package copies, any other literal is a recorded remote,
//! and non-literal or missing sources are dynamic findings.

use std::fmt;

/// One `module` block declaration with its parsed `source`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleDecl {
    /// Module block label (empty when labels misbehave; units checks shape).
    pub name: String,
    /// Parsed `source` attribute.
    pub source: ModuleSource,
}

/// Parsed `source` attribute of one `module` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleSource {
    /// String-literal source, verbatim.
    Literal(String),
    /// Non-literal or missing source (`hcl-rs` rejects repeats as syntax).
    Dynamic {
        /// Stable reason (`missing_source`, `template_source`,
        /// `dynamic_source`).
        reason: String,
    },
}

/// S6 classification of one module source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceClass {
    /// Literal `./`/`../` source: a local edge.
    Local,
    /// Absolute-path literal: external package copy (finding, never edge).
    External,
    /// Any other literal: recorded remote (finding, never edge).
    Remote(RemoteKind),
    /// Non-literal or missing source (finding, never edge).
    Dynamic,
}

/// Remote-source kind, from the literal spelling only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteKind {
    /// `git::` sources and `github.com`/`bitbucket.org` shorthands.
    Git,
    /// Registry addresses (`namespace/name/system`, host-prefixed included).
    Registry,
    /// Any other remote (`https://`, `s3::`, `hg::`, ...).
    Other,
}

/// One module reference: file-stamped declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRef {
    /// Repo-relative caller file.
    pub file: String,
    /// Module block label.
    pub name: String,
    /// Parsed `source` attribute.
    pub source: ModuleSource,
}

/// Typed module-boundary failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleError {
    /// A local source escapes the repository (lexically or via symlink).
    Escape {
        /// Offending `caller:source` rendering or side path.
        target: String,
    },
    /// A local target is absent or holds no effective config.
    MissingTarget {
        /// Offending target path.
        target: String,
    },
    /// A directed module cycle.
    Cycle {
        /// Cycle path with the repeated start node last.
        path: Vec<String>,
    },
    /// A module target cannot be read.
    Unreadable {
        /// Offending target path (empty for the repository root).
        target: String,
    },
    /// A module target walk exceeds the file cap.
    TooManyFiles {
        /// Observed file count.
        count: usize,
    },
}

impl fmt::Display for ModuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Escape { target } => write!(f, "module_escape:{target}"),
            Self::MissingTarget { target } => write!(f, "missing_target:{target}"),
            Self::Cycle { path } => write!(f, "module_cycle:{}", path.join(">")),
            Self::Unreadable { target } if target.is_empty() => {
                write!(f, "unreadable_module_root")
            }
            Self::Unreadable { target } => write!(f, "unreadable_module:{target}"),
            Self::TooManyFiles { count } => write!(f, "module_too_many_files:{count}"),
        }
    }
}

impl std::error::Error for ModuleError {}

/// `source` of one native `module` block: the single direct attribute.
pub(crate) fn source_from_native(block: &hcl::Block) -> ModuleSource {
    let mut sources = block
        .body()
        .attributes()
        .filter(|attribute| attribute.key.as_str() == "source");
    let Some(first) = sources.next() else {
        return ModuleSource::Dynamic {
            reason: "missing_source".to_owned(),
        };
    };
    debug_assert!(
        sources.next().is_none(),
        "hcl-rs rejects repeated attributes at parse"
    );
    match &first.expr {
        hcl::Expression::String(literal) => ModuleSource::Literal(literal.clone()),
        hcl::Expression::TemplateExpr(_) => ModuleSource::Dynamic {
            reason: "template_source".to_owned(),
        },
        _ => ModuleSource::Dynamic {
            reason: "dynamic_source".to_owned(),
        },
    }
}

/// `source` of one JSON `module` entry body.
pub(crate) fn source_from_json(body: &serde_json::Value) -> ModuleSource {
    let Some(object) = body.as_object() else {
        return ModuleSource::Dynamic {
            reason: "dynamic_source".to_owned(),
        };
    };
    match object.get("source") {
        None => ModuleSource::Dynamic {
            reason: "missing_source".to_owned(),
        },
        Some(serde_json::Value::String(literal)) => ModuleSource::Literal(literal.clone()),
        Some(_) => ModuleSource::Dynamic {
            reason: "dynamic_source".to_owned(),
        },
    }
}

/// Classify one literal source (never [`SourceClass::Dynamic`]).
#[must_use]
pub fn classify_literal(source: &str) -> SourceClass {
    if source.starts_with("./") || source.starts_with("../") {
        SourceClass::Local
    } else if is_absolute(source) {
        SourceClass::External
    } else {
        SourceClass::Remote(remote_kind(source))
    }
}

/// True for absolute-path spellings (POSIX, UNC, or drive-led).
fn is_absolute(source: &str) -> bool {
    if source.starts_with('/') || source.starts_with('\\') {
        return true;
    }
    let bytes = source.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// Remote kind from the literal spelling only.
fn remote_kind(source: &str) -> RemoteKind {
    if source.starts_with("git::")
        || source.starts_with("github.com/")
        || source.starts_with("bitbucket.org/")
    {
        RemoteKind::Git
    } else if source.contains("::") || source.contains("://") {
        RemoteKind::Other
    } else {
        RemoteKind::Registry
    }
}
