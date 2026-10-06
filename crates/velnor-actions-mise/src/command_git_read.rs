//! Closed argument admission for read-only Discovery Git commands.

use std::ffi::OsString;

use crate::MiseError;

use super::diff;

/// Discovery verb admitted by the read-only constructor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReadVerb {
    /// Repository topology or revision lookup.
    RevParse,
    /// Index and working-tree path enumeration.
    LsFiles,
    /// Tree entry enumeration.
    LsTree,
    /// Restricted comparison.
    Diff,
    /// Exact object or path-at-object read.
    Show,
    /// Exact remote-origin lookup.
    Config,
}

impl ReadVerb {
    /// Native Git subcommand spelling.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::RevParse => "rev-parse",
            Self::LsFiles => "ls-files",
            Self::LsTree => "ls-tree",
            Self::Diff => "diff",
            Self::Show => "show",
            Self::Config => "config",
        }
    }
}

/// Validated child arguments and repository requirements for one read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ReadInvocation {
    /// Complete child argv after the program name.
    pub(super) arguments: Vec<OsString>,
    /// Admitted verb.
    pub(super) verb: ReadVerb,
    /// Whether Git may read the repository index.
    pub(super) uses_index: bool,
    /// Whether native Git must be allowed to report a non-repository result.
    pub(super) allows_non_repo: bool,
    /// Full hexadecimal object IDs carried by this request.
    pub(super) full_oids: Vec<String>,
}

/// Admit one exact Discovery Git argv and add constructor-owned controls.
pub(super) fn prepare(arguments: &[OsString]) -> Result<ReadInvocation, MiseError> {
    let first = arguments
        .first()
        .ok_or_else(|| invalid("git_read_verb", "missing"))?;
    let verb = first
        .to_str()
        .ok_or_else(|| invalid("git_read_verb", "non_utf8"))?;
    match verb {
        "rev-parse" => prepare_rev_parse(arguments),
        "ls-files" => prepare_ls_files(arguments),
        "ls-tree" => prepare_ls_tree(arguments),
        "diff" => prepare_diff(arguments),
        "show" => prepare_show(arguments),
        "config" => prepare_config(arguments),
        _ => Err(MiseError::GitVerbRejected {
            verb: verb.to_owned(),
        }),
    }
}

fn prepare_rev_parse(arguments: &[OsString]) -> Result<ReadInvocation, MiseError> {
    let values = text_args(&arguments[1..])?;
    let allows_non_repo = match values.as_slice() {
        [value] if matches!(*value, "HEAD" | "HEAD~1" | "HEAD^1" | "HEAD^2") => false,
        [value] if matches!(*value, "--show-toplevel" | "--is-inside-work-tree") => true,
        [flag, value] if *flag == "--abbrev-ref" && *value == "origin/HEAD" => false,
        [value] => return Err(rejected_text(value)),
        _ => return Err(unsupported(&values, "rev-parse")),
    };
    Ok(simple(
        ReadVerb::RevParse,
        arguments,
        false,
        allows_non_repo,
    ))
}

fn prepare_ls_files(arguments: &[OsString]) -> Result<ReadInvocation, MiseError> {
    let values = text_args(&arguments[1..])?;
    let accepted = matches!(
        values.as_slice(),
        ["-z"] | ["--stage", "-z"] | ["--others", "--exclude-standard", "-z"]
    );
    if !accepted {
        return Err(unsupported(&values, "ls-files"));
    }
    Ok(simple(ReadVerb::LsFiles, arguments, true, false))
}

fn prepare_ls_tree(arguments: &[OsString]) -> Result<ReadInvocation, MiseError> {
    let values = text_args(&arguments[1..])?;
    let [recursive, nul, object] = values.as_slice() else {
        return Err(shape("ls-tree"));
    };
    if *recursive != "-r" || *nul != "-z" {
        return Err(rejected_text(if *recursive == "-r" {
            nul
        } else {
            recursive
        }));
    }
    if !full_oid(object) {
        return Err(invalid("git_read_object_id", object));
    }
    Ok(with_oids(
        ReadVerb::LsTree,
        arguments,
        false,
        false,
        vec![(*object).to_owned()],
    ))
}

fn prepare_diff(arguments: &[OsString]) -> Result<ReadInvocation, MiseError> {
    let invocation = diff::prepare(arguments)?;
    Ok(ReadInvocation {
        arguments: invocation.arguments,
        verb: ReadVerb::Diff,
        uses_index: invocation.uses_index,
        allows_non_repo: !invocation.uses_index,
        full_oids: if invocation.uses_index {
            text_args(&arguments[1..])?
                .into_iter()
                .take_while(|value| *value != "--")
                .filter(|value| !value.starts_with('-'))
                .flat_map(|value| value.split("..."))
                .map(str::to_owned)
                .collect()
        } else {
            Vec::new()
        },
    })
}

fn prepare_show(arguments: &[OsString]) -> Result<ReadInvocation, MiseError> {
    let values = text_args(&arguments[1..])?;
    let [spec] = values.as_slice() else {
        return Err(shape("show"));
    };
    let object = show_object(spec).ok_or_else(|| invalid("git_read_show_spec", spec))?;
    Ok(with_oids(
        ReadVerb::Show,
        arguments,
        false,
        false,
        vec![object.to_owned()],
    ))
}

fn prepare_config(arguments: &[OsString]) -> Result<ReadInvocation, MiseError> {
    let values = text_args(&arguments[1..])?;
    if values.as_slice() != ["--get", "remote.origin.url"] {
        return Err(unsupported(&values, "config"));
    }
    Ok(simple(ReadVerb::Config, arguments, false, false))
}

fn simple(
    verb: ReadVerb,
    arguments: &[OsString],
    uses_index: bool,
    allows_non_repo: bool,
) -> ReadInvocation {
    ReadInvocation {
        arguments: fixed_arguments(verb, &arguments[1..]),
        verb,
        uses_index,
        allows_non_repo,
        full_oids: Vec::new(),
    }
}

fn with_oids(
    verb: ReadVerb,
    arguments: &[OsString],
    uses_index: bool,
    allows_non_repo: bool,
    full_oids: Vec<String>,
) -> ReadInvocation {
    ReadInvocation {
        arguments: fixed_arguments(verb, &arguments[1..]),
        verb,
        uses_index,
        allows_non_repo,
        full_oids,
    }
}

fn fixed_arguments(verb: ReadVerb, tail: &[OsString]) -> Vec<OsString> {
    let mut arguments = vec![
        "--no-pager".into(),
        "-c".into(),
        "core.splitIndex=false".into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        "core.hooksPath=/dev/null".into(),
        verb.as_str().into(),
    ];
    arguments.extend(tail.iter().cloned());
    arguments
}

fn text_args(arguments: &[OsString]) -> Result<Vec<&str>, MiseError> {
    arguments.iter().map(text).collect()
}

fn text(argument: &OsString) -> Result<&str, MiseError> {
    argument
        .to_str()
        .ok_or_else(|| invalid("git_read_argument", "non_utf8"))
}

fn show_object(spec: &str) -> Option<&str> {
    if full_oid(spec) {
        return Some(spec);
    }
    let (object, path) = spec.split_once(':')?;
    (full_oid(object) && safe_path(path)).then_some(object)
}

fn full_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.eq_ignore_ascii_case(".git")
        })
}

fn rejected_text(value: &str) -> MiseError {
    invalid("git_read_argument", value)
}

fn shape(verb: &str) -> MiseError {
    invalid("git_read_operands", verb)
}

fn unsupported(values: &[&str], verb: &str) -> MiseError {
    values
        .iter()
        .find(|value| value.starts_with('-'))
        .map_or_else(|| shape(verb), |value| rejected_text(value))
}

fn invalid(field: &str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadVerb, prepare};
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn admits_current_read_shapes() {
        let tree_oid = "a".repeat(40);
        let blob_oid = "b".repeat(40);
        let path_spec = format!("{}:Cargo.toml", "c".repeat(40));
        let cases = vec![
            args(&["rev-parse", "HEAD"]),
            args(&["rev-parse", "HEAD~1"]),
            args(&["rev-parse", "HEAD^1"]),
            args(&["rev-parse", "HEAD^2"]),
            args(&["rev-parse", "--show-toplevel"]),
            args(&["rev-parse", "--is-inside-work-tree"]),
            args(&["rev-parse", "--abbrev-ref", "origin/HEAD"]),
            args(&["ls-files", "-z"]),
            args(&["ls-files", "--stage", "-z"]),
            args(&["ls-files", "--others", "--exclude-standard", "-z"]),
            vec!["ls-tree".into(), "-r".into(), "-z".into(), tree_oid.into()],
            vec!["show".into(), blob_oid.into()],
            vec!["show".into(), path_spec.into()],
            args(&["config", "--get", "remote.origin.url"]),
        ];
        for values in cases {
            assert!(prepare(&values).is_ok(), "{values:?}");
        }
    }

    #[test]
    fn delegates_diff_and_marks_no_index_non_repository() {
        let invocation = prepare(&args(&[
            "diff",
            "--no-index",
            "--exit-code",
            "--",
            "left",
            "right",
        ]))
        .expect("diff shape");
        assert_eq!(invocation.verb, ReadVerb::Diff);
        assert!(!invocation.uses_index);
        assert!(invocation.allows_non_repo);
        assert_eq!(invocation.arguments[0], "--no-pager");
    }

    #[test]
    fn rejects_unowned_flags_and_short_object_ids() {
        for values in [
            args(&["rev-parse", "--verify", "HEAD"]),
            args(&["ls-files", "--cached"]),
            args(&["ls-tree", "-r", "-z", "abcd"]),
            args(&["show", "abcd"]),
            args(&["config", "--list"]),
        ] {
            assert!(prepare(&values).is_err(), "{values:?}");
        }
    }
}
