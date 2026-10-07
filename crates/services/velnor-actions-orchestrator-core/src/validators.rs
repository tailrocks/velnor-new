//! Verb-specific Git argument validation for `diff` and `show` call paths.
//!
//! `GitRequest` allowlists verbs but passes arguments byte-exact, so
//! request-supplied revisions and paths need their own gate before they
//! reach `git`. Two revision contexts exist:
//!
//! * Strict (`validate_rev`, `validate_diff_args`, `validate_show_args`,
//!   `validate_git_args`): full 40-char hex SHAs only, mirroring the
//!   baseline-lookup rules (`BaselineLookup::new` in `shard.rs`,
//!   `baseline_artifact_name` in `cover.rs`). Short SHAs, symbolic refs,
//!   and flags all fail.
//! * PR-selection (`validate_diff_rev`, `validate_select_diff_args`,
//!   `validate_select_show_args`): short (4–40 hex) SHAs accepted because
//!   git resolves them at spawn; flags, symbolic refs, and paths stay
//!   gated exactly as in the strict context. Used by the `select.rs`
//!   `changed_files`, `added_files`, and `batch_manifests` call paths.

use std::ffi::OsString;
use std::fmt::{Display, Formatter, Result as FmtResult};

/// Full-length commit SHA: exactly 40 hex chars, never short.
const FULL_SHA_LEN: usize = 40;

/// Shortest unambiguous-ish SHA accepted in the PR-selection context.
const SHORT_SHA_MIN_LEN: usize = 4;

/// `git diff` flags the selection call paths emit; any other leading-dash
/// argument is rejected.
const DIFF_ALLOWED_FLAGS: [&str; 6] = [
    "--name-only",
    "--no-renames",
    "--diff-filter=A",
    "--diff-filter=D",
    "--cached",
    "--",
];

/// `git show` flags the batched-manifest call path emits, besides the
/// separately validated `--format=<payload>` flag.
const SHOW_ALLOWED_FLAGS: [&str; 2] = ["-s", "--"];

/// Prefix of the one parameterized `git show` flag.
const SHOW_FORMAT_PREFIX: &str = "--format=";

/// Rejected Git argument: fail closed, never spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitArgError {
    /// Argument starts with `-` outside the verb allowlist.
    LeadingDash {
        /// Rejected argument.
        arg: String,
    },
    /// Revision is not a full 40-char hex SHA.
    BadRevision {
        /// Rejected revision.
        value: String,
    },
    /// `diff` argument is neither an allowlisted flag nor a revision.
    BadDiffArg {
        /// Rejected argument.
        value: String,
    },
    /// `show` path escapes, is absolute, or carries control bytes.
    BadPath {
        /// Rejected path.
        value: String,
    },
    /// Argument is not valid UTF-8.
    NonUtf8Arg,
}

impl Display for GitArgError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::LeadingDash { arg } => write!(f, "git_arg_leading_dash:{arg}"),
            Self::BadRevision { value } => write!(f, "git_rev_must_be_full_sha:{value}"),
            Self::BadDiffArg { value } => write!(f, "git_diff_arg_rejected:{value}"),
            Self::BadPath { value } => write!(f, "git_path_rejected:{value}"),
            Self::NonUtf8Arg => f.write_str("git_arg_non_utf8"),
        }
    }
}

impl std::error::Error for GitArgError {}

/// Validate one request-supplied revision: full 40-char hex SHA.
///
/// Mirrors the baseline-lookup rules: short SHAs, symbolic refs (`HEAD`),
/// URLs, and flags all fail; case follows `BaselineLookup` (any hex case).
///
/// # Errors
///
/// Returns [`GitArgError::LeadingDash`] for a leading `-` and
/// [`GitArgError::BadRevision`] for anything that is not full hex.
pub fn validate_rev(rev: &str) -> Result<(), GitArgError> {
    check_rev(rev, false)
}

/// Validate one request-supplied revision in the PR-selection context.
///
/// Accepts full or short (4–40) hex SHAs, never a flag or symbolic ref;
/// anything else fails closed to broadening via the caller, never
/// reaching git. The `role` prefixes the error (`bad_base`, `bad_head`)
/// so warnings keep their existing tags.
///
/// # Errors
///
/// Returns `{role}_leading_dash` for a leading `-` and
/// `{role}_must_be_hex_sha` for anything that is not hex of length 4–40.
pub fn validate_diff_rev(rev: &str, role: &str) -> Result<(), String> {
    if rev.starts_with('-') {
        return Err(format!("{role}_leading_dash"));
    }
    let sha = (SHORT_SHA_MIN_LEN..=FULL_SHA_LEN).contains(&rev.len())
        && rev.bytes().all(|b| b.is_ascii_hexdigit());
    if sha {
        Ok(())
    } else {
        Err(format!("{role}_must_be_hex_sha"))
    }
}

/// Validate a `show` object path: relative, no traversal, no control bytes.
///
/// # Errors
///
/// Returns [`GitArgError::BadPath`] for empty, absolute, traversing
/// (`..`), colon-carrying, or control-byte paths, and
/// [`GitArgError::LeadingDash`] for a leading `-`.
pub fn validate_show_path(path: &str) -> Result<(), GitArgError> {
    if path.starts_with('-') {
        return Err(GitArgError::LeadingDash {
            arg: path.to_owned(),
        });
    }
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.split('/').any(|seg| seg == "..")
        || path.chars().any(char::is_control);
    if bad {
        return Err(GitArgError::BadPath {
            value: path.to_owned(),
        });
    }
    Ok(())
}

/// Validate full `git diff` arguments: allowlisted flags plus revisions.
///
/// Accepted: `--name-only`, `--no-renames`, `--diff-filter=A/D`, the
/// `--` separator, `base...head` ranges of full SHAs, and bare full SHAs.
/// Anything else — flags, symbolic refs, short SHAs, paths — is rejected.
///
/// # Errors
///
/// Returns [`GitArgError`] for the first rejected argument.
pub fn validate_diff_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_diff_arg(arg, false)?;
    }
    Ok(())
}

/// Validate `git diff` arguments in the PR-selection context.
///
/// Same shape as [`validate_diff_args`], but range sides and bare
/// revisions may be short (4–40 hex) SHAs; git resolves them at spawn.
/// Flags, symbolic refs, and paths stay rejected.
///
/// # Errors
///
/// Returns [`GitArgError`] for the first rejected argument.
pub fn validate_select_diff_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_diff_arg(arg, true)?;
    }
    Ok(())
}

/// Validate full `git show` arguments: allowlisted flags plus specs.
///
/// Accepted: `-s`, `--`, `--format=<safe payload>`, bare full-SHA
/// separators, and `base:path` specs with a full-SHA base and a
/// [`validate_show_path`] path. Anything else is rejected.
///
/// # Errors
///
/// Returns [`GitArgError`] for the first rejected argument.
pub fn validate_show_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_show_arg(arg, false)?;
    }
    Ok(())
}

/// Validate `git show` arguments in the PR-selection context.
///
/// Same shape as [`validate_show_args`], but bare bases and spec bases
/// may be short (4–40 hex) SHAs; git resolves them at spawn. Flags,
/// format payloads, and paths stay gated exactly as in strict mode.
///
/// # Errors
///
/// Returns [`GitArgError`] for the first rejected argument.
pub fn validate_select_show_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_show_arg(arg, true)?;
    }
    Ok(())
}

/// Validate arguments for one verb; only `diff` and `show` qualify.
///
/// # Errors
///
/// Returns [`GitArgError::BadDiffArg`] for verbs outside `diff`/`show`,
/// else the verb-specific validation result.
pub fn validate_git_args(verb: &str, args: &[OsString]) -> Result<(), GitArgError> {
    match verb {
        "diff" => validate_diff_args(args),
        "show" => validate_show_args(args),
        _ => Err(GitArgError::BadDiffArg {
            value: verb.to_owned(),
        }),
    }
}

/// Shared revision gate: leading dash rejected, then the hex-length rule.
///
/// Strict callers pass `allow_short = false` (full 40-char SHAs only);
/// PR-selection callers pass `true` (4–40 hex SHAs).
fn check_rev(rev: &str, allow_short: bool) -> Result<(), GitArgError> {
    if rev.starts_with('-') {
        return Err(GitArgError::LeadingDash {
            arg: rev.to_owned(),
        });
    }
    let len_ok = if allow_short {
        (SHORT_SHA_MIN_LEN..=FULL_SHA_LEN).contains(&rev.len())
    } else {
        rev.len() == FULL_SHA_LEN
    };
    if len_ok && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(GitArgError::BadRevision {
            value: rev.to_owned(),
        })
    }
}

/// Validate one `diff` argument against flags, ranges, and revisions.
fn validate_diff_arg(arg: &OsString, allow_short: bool) -> Result<(), GitArgError> {
    let text = arg_to_str(arg)?;
    if DIFF_ALLOWED_FLAGS.contains(&text) {
        return Ok(());
    }
    if text.starts_with('-') {
        return Err(GitArgError::LeadingDash {
            arg: text.to_owned(),
        });
    }
    if let Some((base, head)) = text.split_once("...") {
        check_rev(base, allow_short)?;
        return check_rev(head, allow_short);
    }
    if check_rev(text, allow_short).is_ok() {
        return Ok(());
    }
    Err(GitArgError::BadDiffArg {
        value: text.to_owned(),
    })
}

/// Validate one `show` argument against flags, separators, and specs.
fn validate_show_arg(arg: &OsString, allow_short: bool) -> Result<(), GitArgError> {
    let text = arg_to_str(arg)?;
    if SHOW_ALLOWED_FLAGS.contains(&text) {
        return Ok(());
    }
    if let Some(payload) = text.strip_prefix(SHOW_FORMAT_PREFIX) {
        return validate_format_payload(payload);
    }
    if text.starts_with('-') {
        return Err(GitArgError::LeadingDash {
            arg: text.to_owned(),
        });
    }
    if let Some((base, path)) = text.split_once(':') {
        check_rev(base, allow_short)?;
        return validate_show_path(path);
    }
    check_rev(text, allow_short)
}

/// Validate a `--format=` payload: tight charset, no flags or whitespace.
fn validate_format_payload(payload: &str) -> Result<(), GitArgError> {
    let safe = !payload.is_empty()
        && payload
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'%' | b'-' | b'_'));
    if safe {
        Ok(())
    } else {
        Err(GitArgError::BadDiffArg {
            value: format!("{SHOW_FORMAT_PREFIX}{payload}"),
        })
    }
}

/// Lossy-free argument text; non-UTF-8 fails closed.
fn arg_to_str(arg: &OsString) -> Result<&str, GitArgError> {
    arg.to_str().ok_or(GitArgError::NonUtf8Arg)
}
