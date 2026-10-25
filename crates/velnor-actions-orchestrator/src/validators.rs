use std::ffi::OsString;
use std::fmt::{Display, Formatter, Result as FmtResult};

const FULL_SHA_LEN: usize = 40;

const SHORT_SHA_MIN_LEN: usize = 4;

const DIFF_ALLOWED_FLAGS: [&str; 6] = [
    "--name-only",
    "--no-renames",
    "--diff-filter=A",
    "--diff-filter=D",
    "--cached",
    "--",
];

const SHOW_ALLOWED_FLAGS: [&str; 2] = ["-s", "--"];

const SHOW_FORMAT_PREFIX: &str = "--format=";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitArgError {
    LeadingDash { arg: String },
    BadRevision { value: String },
    BadDiffArg { value: String },
    BadPath { value: String },
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

pub fn validate_rev(rev: &str) -> Result<(), GitArgError> {
    check_rev(rev, false)
}

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

pub fn validate_diff_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_diff_arg(arg, false)?;
    }
    Ok(())
}

pub fn validate_select_diff_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_diff_arg(arg, true)?;
    }
    Ok(())
}

pub fn validate_show_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_show_arg(arg, false)?;
    }
    Ok(())
}

pub fn validate_select_show_args(args: &[OsString]) -> Result<(), GitArgError> {
    for arg in args {
        validate_show_arg(arg, true)?;
    }
    Ok(())
}

pub fn validate_git_args(verb: &str, args: &[OsString]) -> Result<(), GitArgError> {
    match verb {
        "diff" => validate_diff_args(args),
        "show" => validate_show_args(args),
        _ => Err(GitArgError::BadDiffArg {
            value: verb.to_owned(),
        }),
    }
}

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

fn arg_to_str(arg: &OsString) -> Result<&str, GitArgError> {
    arg.to_str().ok_or(GitArgError::NonUtf8Arg)
}
