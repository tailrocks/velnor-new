//! Closed native object admission before callback-capable discovery reads.
use std::ffi::OsString;

use super::owned_context::OwnedNativeContext;
use super::read::{ReadInvocation, ReadVerb};
use super::{Bounds, IsolatedCommand};
use crate::MiseError;

pub(super) fn admit(
    owner: &IsolatedCommand,
    invocation: &ReadInvocation,
    private: &OwnedNativeContext<'_>,
    has_head: bool,
    bounds: &Bounds<'_>,
) -> Result<Option<super::super::process::Outcome>, MiseError> {
    if invocation.verb == ReadVerb::Show {
        if let Some(refused) = admit_show(owner, private, bounds)? {
            return Ok(Some(refused));
        }
        if !owner
            .args
            .get(1)
            .is_some_and(|spec| spec.to_str().is_some_and(|text| text.contains(':')))
        {
            return Ok(None);
        }
    }
    if invocation.verb != ReadVerb::LsTree
        && invocation.verb != ReadVerb::Show
        && !(invocation.verb == ReadVerb::Diff && invocation.uses_index)
    {
        return Ok(None);
    }
    let mut trees = invocation
        .full_oids
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    if invocation.verb == ReadVerb::Diff && has_head {
        trees.push("HEAD".into());
    }
    let range = owner
        .args
        .iter()
        .take_while(|argument| *argument != "--")
        .any(|argument| argument.to_str().is_some_and(|text| text.contains("...")));
    if range {
        let [left, right] = invocation.full_oids.as_slice() else {
            return Err(invalid("merge_base_operands_invalid"));
        };
        let outcome = query(
            private,
            vec![
                "merge-base".into(),
                "--all".into(),
                "--".into(),
                left.into(),
                right.into(),
            ],
            bounds,
        );
        match outcome.result.as_ref() {
            Ok(output) if output.success && output.stderr.is_empty() => {
                trees.extend(base_ids(&output.stdout, left.len())?);
            }
            _ => return Ok(Some(refuse(outcome, "merge_base_not_proved"))),
        }
    }
    Ok(admit_trees(trees, private, bounds))
}

fn admit_trees(
    trees: Vec<OsString>,
    private: &OwnedNativeContext<'_>,
    bounds: &Bounds<'_>,
) -> Option<super::super::process::Outcome> {
    for tree in trees {
        let outcome = query(
            private,
            vec![
                "ls-tree".into(),
                "--full-tree".into(),
                "-r".into(),
                "-z".into(),
                "--format=%(objectmode)".into(),
                tree,
            ],
            bounds,
        );
        if !outcome.result.as_ref().is_ok_and(|output| {
            output.success && output.stderr.is_empty() && modes_admitted(&output.stdout)
        }) {
            return Some(refuse(outcome, "revision_tree_unsupported"));
        }
    }
    None
}

fn admit_show(
    owner: &IsolatedCommand,
    private: &OwnedNativeContext<'_>,
    bounds: &Bounds<'_>,
) -> Result<Option<super::super::process::Outcome>, MiseError> {
    let spec = owner
        .args
        .get(1)
        .ok_or_else(|| invalid("show_spec_missing"))?;
    let outcome = query(
        private,
        vec!["cat-file".into(), "-t".into(), "--".into(), spec.clone()],
        bounds,
    );
    Ok(if succeeds(&outcome, b"blob\n") {
        None
    } else {
        Some(refuse(outcome, "show_object_not_proved_blob"))
    })
}

fn query(
    private: &OwnedNativeContext<'_>,
    tail: Vec<OsString>,
    bounds: &Bounds<'_>,
) -> super::super::process::Outcome {
    let prepared = (|| {
        let mut arguments = [
            "--no-pager",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.splitIndex=false",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
        arguments.extend(tail);
        let query = bounds.native.command(arguments);
        let mut command = query.command()?;
        super::owned_config::apply_private_config_controls(&mut command);
        private
            .apply(&mut command)
            .map_err(|error| super::index_error(&error))?;
        Ok((query, command, bounds.remaining()?))
    })();
    match prepared {
        Ok((query, command, remaining)) => {
            super::super::process::run(&query, command, bounds.cap, remaining, bounds.cancel)
        }
        Err(error) => super::super::process::Outcome {
            result: Err(error),
            safe_to_cleanup: true,
        },
    }
}

fn succeeds(outcome: &super::super::process::Outcome, expected: &[u8]) -> bool {
    outcome
        .result
        .as_ref()
        .is_ok_and(|output| output.success && output.stderr.is_empty() && output.stdout == expected)
}

fn modes_admitted(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    bytes.last() == Some(&0)
        && bytes[..bytes.len() - 1]
            .split(|byte| *byte == 0)
            .all(|mode| matches!(mode, b"100644" | b"100755" | b"120000"))
}

fn base_ids(bytes: &[u8], width: usize) -> Result<Vec<OsString>, MiseError> {
    if bytes.last() != Some(&b'\n') {
        return Err(invalid("merge_base_output_invalid"));
    }
    let mut values = Vec::new();
    for line in bytes[..bytes.len() - 1].split(|byte| *byte == b'\n') {
        if values.len() >= 2048 || line.len() != width || !line.iter().all(u8::is_ascii_hexdigit) {
            return Err(invalid("merge_base_output_invalid"));
        }
        values.push(OsString::from(
            std::str::from_utf8(line).map_err(|_| invalid("merge_base_output_invalid"))?,
        ));
    }
    Ok(values)
}

fn refuse(
    mut outcome: super::super::process::Outcome,
    code: &'static str,
) -> super::super::process::Outcome {
    if outcome.result.is_ok() {
        outcome.result = Err(invalid(code));
    }
    outcome
}

fn invalid(code: &'static str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "git_object_admission".to_owned(),
        value: code.to_owned(),
    }
}
