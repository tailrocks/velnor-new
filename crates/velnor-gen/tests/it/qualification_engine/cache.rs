// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::{StepSignature, condition, set};

const OR_REPLACED: [(&str, &str); 1] = [(
    "test -z \"$HIT\" || test \"$HIT\" = false",
    "test \"$HIT\" != true",
)];

const NEWLINE_REPLACED: [(&str, &str); 1] = [(
    "printf '%s\n' cache-ok > \"g4 cache/note.txt\"",
    "printf '%s\\n' cache-ok > \"g4 cache/note.txt\"",
)];

const RESTORE_ONLY: (&str, &str, &str) = (
    "cache-hosted",
    "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
);

const MESSAGE_REPLACED: [(&str, &str); 1] = [(
    "echo '::error title=cache_qualification_requires_non_default_branch::Dispatch cache qualification modes from a non-default branch; every cache job is skipped on the default branch.'",
    "echo '::error title=cache_qualification_requires_non_default_branch::Dispatch cache qualification modes from a non-default branch. Every cache job is skipped on the default branch.'",
)];

pub(super) fn unparenthesised(condition: &str) -> Option<String> {
    let (mode, gate) = condition
        .strip_prefix('(')?
        .strip_suffix(')')?
        .split_once(") && (")?;
    Some(format!("{mode} && {gate}"))
}

fn restore_only(id: &str, steps: Vec<StepSignature>) -> (Vec<StepSignature>, bool) {
    let (job, from, to) = RESTORE_ONLY;
    let mut restored = false;
    let steps = steps
        .into_iter()
        .map(|step| {
            if id == job && step.uses.as_deref() == Some(from) {
                restored = true;
                StepSignature {
                    uses: Some(to.to_owned()),
                    ..step
                }
            } else {
                step
            }
        })
        .collect();
    (steps, restored)
}

fn replace_runs(steps: Vec<StepSignature>, table: &[(&str, &str)]) -> (Vec<StepSignature>, bool) {
    let mut replaced = false;
    let steps = steps
        .into_iter()
        .map(|step| {
            let hit = table
                .iter()
                .find(|(from, _)| step.run.as_deref() == Some(from));
            match hit {
                Some((_, to)) => {
                    replaced = true;
                    StepSignature {
                        run: Some((*to).to_owned()),
                        ..step
                    }
                }
                None => step,
            }
        })
        .collect();
    (steps, replaced)
}

pub(super) fn apply(id: &str, flat: Vec<StepSignature>) -> (Vec<StepSignature>, Vec<&'static str>) {
    let (flat, ored) = replace_runs(flat, &OR_REPLACED);
    let (flat, worded) = replace_runs(flat, &MESSAGE_REPLACED);
    let (flat, escaped) = replace_runs(flat, &NEWLINE_REPLACED);
    let (flat, restored) = restore_only(id, flat);
    let kinds = [
        ("an || is written as one test", ored),
        ("a semicolon in a message is a full stop", worded),
        ("a line feed in a printf format is written as \\n", escaped),
        (
            "a restore-only actions/cache is actions/cache/restore",
            restored,
        ),
    ];
    (
        flat,
        kinds
            .into_iter()
            .filter_map(|(kind, happened)| happened.then_some(kind))
            .collect(),
    )
}

pub(super) fn head(
    id: &str,
    [engine, engine_workflow, generated]: [&Value; 3],
    note: &mut impl FnMut(&'static str),
    unexpected: &mut Vec<String>,
) {
    if condition(engine) != condition(generated) {
        if parenthesised(engine, generated) {
            note("a case condition is parenthesised");
        } else {
            unexpected.push(format!(
                "{id}: if: engine {:?}, generated {:?}",
                condition(engine),
                condition(generated)
            ));
        }
    }
    if engine.get("env") != generated.get("env") {
        unexpected.push(format!(
            "{id}: env: engine {:?}, generated {:?}",
            engine.get("env"),
            generated.get("env")
        ));
    }
    let engine_mode = engine.get("cache-mode");
    let inherited = engine_mode.or_else(|| engine_workflow.get("cache-mode"));
    let differs = match generated.get("cache-mode") {
        Some(mode) => Some(mode) != inherited,
        None => engine_mode.is_some(),
    };
    if differs {
        unexpected.push(format!(
            "{id}: cache-mode: engine {inherited:?}, generated {:?}",
            generated.get("cache-mode")
        ));
    }
}

fn parenthesised(engine: &Value, generated: &Value) -> bool {
    let generated = condition(generated);
    condition(engine) == generated.as_deref().and_then(unparenthesised)
}

pub(super) fn assert_notes(kinds: &BTreeMap<&'static str, BTreeSet<String>>) {
    let listed = |kind: &str| kinds.get(kind).cloned().unwrap_or_default();
    let empty = set(&["empty-cache-hosted", "empty-cache-scale-set"]);
    assert_eq!(listed("an || is written as one test"), empty);
    assert_eq!(listed("a case condition is parenthesised"), empty);
    assert_eq!(
        listed("a line feed in a printf format is written as \\n"),
        empty
    );
    assert_eq!(
        listed("a restore-only actions/cache is actions/cache/restore"),
        set(&["cache-hosted"])
    );
    assert_eq!(
        listed("a semicolon in a message is a full stop"),
        set(&["cache-default-branch-guard"])
    );
}

#[test]
fn the_mbx_script_parses_and_refuses_an_unknown_phase() -> super::TestResult {
    let script = super::repo("qualification/mbx-cache-roundtrip.sh");
    let parsed = std::process::Command::new("/bin/bash")
        .arg("-n")
        .arg(&script)
        .status()?;
    assert!(parsed.success());
    let unknown = std::process::Command::new("/bin/bash")
        .arg(&script)
        .arg("no-such-phase")
        .output()?;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("no-such-phase"));
    Ok(())
}
