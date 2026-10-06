//! Closed discovery diff operands and constructor-owned native Git controls.

use std::ffi::OsString;

use crate::MiseError;

pub(super) struct DiffInvocation {
    pub(super) arguments: Vec<OsString>,
    pub(super) uses_index: bool,
}

pub(super) fn prepare(arguments: &[OsString]) -> Result<DiffInvocation, MiseError> {
    let mut no_index = false;
    let mut cached = false;
    let mut revisions = 0;
    let mut separator = None;
    for (position, argument) in arguments.iter().enumerate().skip(1) {
        let text = argument.to_str().ok_or_else(|| rejected(argument))?;
        match text {
            "--" => {
                separator = Some(position);
                break;
            }
            "--no-index" => no_index = true,
            "--cached" => cached = true,
            "-z" | "--name-only" | "--no-renames" | "--exit-code" | "--diff-filter=A"
            | "--diff-filter=D" => {}
            value if revision(value) => revisions += 1,
            _ => return Err(rejected(argument)),
        }
    }
    let paths = separator.map_or(&[][..], |position| &arguments[position + 1..]);
    if revisions > 2
        || (no_index && (cached || revisions != 0 || paths.len() != 2))
        || paths.iter().any(|path| path.is_empty())
        || (!no_index && !paths.is_empty())
    {
        return Err(MiseError::InvalidStepInput {
            field: "git_diff_operands".to_owned(),
            value: "unsupported_shape".to_owned(),
        });
    }
    let mut owned = vec![
        "--no-pager".into(),
        "-c".into(),
        "core.splitIndex=false".into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        "core.hooksPath=/dev/null".into(),
        "diff".into(),
        "--no-ext-diff".into(),
        "--no-textconv".into(),
    ];
    owned.extend(arguments.iter().skip(1).cloned());
    Ok(DiffInvocation {
        arguments: owned,
        uses_index: !no_index,
    })
}

fn revision(value: &str) -> bool {
    if let Some((base, head)) = value.split_once("...") {
        hex_revision(base) && hex_revision(head)
    } else {
        hex_revision(value)
    }
}

fn hex_revision(value: &str) -> bool {
    (4..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn rejected(argument: &OsString) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_diff_argument".to_owned(),
        value: argument.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::prepare;
    use std::ffi::OsString;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn all_discovery_shapes_keep_owned_controls() -> Result<(), crate::MiseError> {
        for values in [
            vec![
                "diff",
                "-z",
                "--name-only",
                "--no-renames",
                "1234",
                "abcd",
                "--",
            ],
            vec![
                "diff",
                "-z",
                "--name-only",
                "--cached",
                "--no-renames",
                "--",
            ],
            vec![
                "diff",
                "--name-only",
                "--diff-filter=A",
                "1234...abcd",
                "--",
            ],
            vec![
                "diff",
                "--name-only",
                "--diff-filter=D",
                "1234",
                "abcd",
                "--",
            ],
        ] {
            let invocation = prepare(&args(&values))?;
            assert!(invocation.uses_index);
            assert!(
                invocation
                    .arguments
                    .iter()
                    .any(|arg| arg == "core.splitIndex=false")
            );
            assert!(
                invocation
                    .arguments
                    .iter()
                    .any(|arg| arg == "core.hooksPath=/dev/null")
            );
        }
        Ok(())
    }

    #[test]
    fn no_index_paths_are_literal_and_do_not_need_repository_index() -> Result<(), crate::MiseError>
    {
        let invocation = prepare(&args(&[
            "diff",
            "--no-index",
            "--exit-code",
            "--",
            "--output=literal-file",
            "other",
        ]))?;
        assert!(!invocation.uses_index);
        Ok(())
    }

    #[test]
    fn effects_and_unknown_operands_never_reach_git() {
        for flag in [
            "--output=target",
            "--output",
            "--ext-diff",
            "--textconv",
            "--no-prefix",
            "--no-index=x",
            "--cached=x",
            "--diff-filter=E",
            "HEAD",
            "-c",
        ] {
            assert!(prepare(&args(&["diff", flag, "--"])).is_err(), "{flag}");
        }
        for values in [
            vec!["diff", "--name-only", "--", "path"],
            vec!["diff", "--no-index", "--cached", "--", "a", "b"],
            vec!["diff", "--no-index", "--", "a"],
            vec!["diff", "1234", "abcd", "5678", "--"],
        ] {
            assert!(prepare(&args(&values)).is_err());
        }
    }
}
