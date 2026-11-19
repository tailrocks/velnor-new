// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::StepSignature;

const PIPE_REPLACED: [(&str, [&str; 2]); 1] = [(
    "g4-path-ok | grep -qx path-ok",
    [
        "g4-path-ok > \"$RUNNER_TEMP/g4-path-out\"",
        "grep -qx path-ok \"$RUNNER_TEMP/g4-path-out\"",
    ],
)];

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

fn replace_pipes(steps: Vec<StepSignature>) -> (Vec<StepSignature>, bool) {
    let mut replaced = false;
    let mut out = Vec::new();
    for step in steps {
        let hit = PIPE_REPLACED
            .iter()
            .find(|(from, _)| step.run.as_deref() == Some(from));
        match hit {
            Some((_, to)) => {
                replaced = true;
                for run in to {
                    out.push(StepSignature {
                        run: Some((*run).to_owned()),
                        ..step.clone()
                    });
                }
            }
            None => out.push(step),
        }
    }
    (out, replaced)
}

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
    let (flat, piped) = replace_pipes(flat);
    let (flat, ored) = replace_runs(flat, &OR_REPLACED);
    let (flat, worded) = replace_runs(flat, &MESSAGE_REPLACED);
    let (flat, escaped) = replace_runs(flat, &NEWLINE_REPLACED);
    let (flat, restored) = restore_only(id, flat);
    let kinds = [
        ("a pipe is written as two steps", piped),
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
