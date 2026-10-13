//! Empty-cache qualification. Not part of `features`.
//!
//! Each lane restores a key that does not exist, then saves and restores a
//! workspace path that contains a space. The key includes `github.job` so the
//! two lanes do not share one cache entry.

use super::super::features::run_step;
use super::steps::{mapping, run_env};
use super::{Extras, both};
use crate::yaml::Yaml;

const RESTORE: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const EMPTY_KEY: &str = "g4-empty-${{ github.run_id }}-${{ github.job }}";
const SPACE_KEY: &str = "g4-space-${{ github.run_id }}-${{ github.job }}";
const SPACE_PATH: &str = "g4 cache/note.txt";
const WRITE: &str = "mkdir -p \"g4 cache\" && printf '%s\\n' cache-ok > \"g4 cache/note.txt\"";
const REMOVE: &str = "rm -f \"g4 cache/note.txt\"";
const MISS: &str = "test \"$HIT\" = false";
const HIT: &str = "grep -qx cache-ok \"g4 cache/note.txt\" && test \"$HIT\" = true";

/// Hosted and scale-set jobs for `inputs.mode == 'empty-cache'`.
pub(super) fn jobs(hosted: &Yaml, scale: &Yaml) -> Vec<(String, Yaml)> {
    both(
        "empty-cache",
        "Empty cache",
        hosted,
        scale,
        steps(),
        Extras {
            permissions: Some(mapping(&[("contents", "read"), ("actions", "write")])),
            ..Extras::default()
        },
    )
}

fn steps() -> Vec<Yaml> {
    vec![
        restore(
            "Restore absent cache",
            "empty",
            EMPTY_KEY,
            "g4-cache.txt",
            false,
        ),
        run_env(
            "Require cache miss",
            &[("HIT", "${{ steps.empty.outputs.cache-hit }}")],
            MISS,
        ),
        run_step("Write spaced path", WRITE),
        save_space(),
        run_step("Remove spaced path", REMOVE),
        restore("Restore spaced path", "space", SPACE_KEY, SPACE_PATH, true),
        run_env(
            "Require spaced restore",
            &[("HIT", "${{ steps.space.outputs.cache-hit }}")],
            HIT,
        ),
    ]
}

fn restore(name: &str, id: &str, key: &str, path: &str, fail_on_miss: bool) -> Yaml {
    let miss = if fail_on_miss { "true" } else { "false" };
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("uses".to_owned(), Yaml::str(RESTORE)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("path".to_owned(), path_value(path)),
                ("key".to_owned(), Yaml::str(key)),
                ("fail-on-cache-miss".to_owned(), Yaml::str(miss)),
            ]),
        ),
    ])
}

fn save_space() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Save spaced path")),
        ("uses".to_owned(), Yaml::str(SAVE)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("path".to_owned(), path_value(SPACE_PATH)),
                ("key".to_owned(), Yaml::str(SPACE_KEY)),
            ]),
        ),
    ])
}

fn path_value(path: &str) -> Yaml {
    if path.contains(' ') {
        Yaml::quoted(path)
    } else {
        Yaml::str(path)
    }
}
