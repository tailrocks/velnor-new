//! Empty-cache qualification. Not part of `features`.
//!
//! Each lane restores a key that does not exist, then saves and restores a
//! workspace path that contains a space. The key includes `github.job` so the
//! two lanes do not share one cache entry.

use super::super::features::run_step;
use super::steps::{mapping, run_env};
use super::{Extras, RunnerSpec, both};
use crate::yaml::Yaml;

const RESTORE: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const EMPTY_KEY: &str = "g4-empty-${{ github.run_id }}-${{ github.job }}";
const SPACE_KEY: &str = "g4-space-${{ github.run_id }}-${{ github.job }}";
const SPACE_PATH: &str = "g4 cache/note.txt";
const WRITE: &str = "mkdir -p \"g4 cache\" && printf '%s\n' cache-ok > \"g4 cache/note.txt\"";
const REMOVE: &str = "rm -f \"g4 cache/note.txt\"";
const MISS: &str = "test -z \"$HIT\" || test \"$HIT\" = false";
const HIT: &str = "grep -qx cache-ok \"g4 cache/note.txt\" && test \"$HIT\" = true";

/// Hosted and scale-set jobs for `inputs.mode == 'empty-cache'`.
pub(super) fn jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
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

#[cfg(test)]
mod tests {
    use super::{MISS, WRITE};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn cache_miss_accepts_unset_and_empty_outputs_only() -> std::io::Result<()> {
        for (hit, expected) in [
            (None, true),
            (Some(""), true),
            (Some("false"), true),
            (Some("true"), false),
            (Some("unexpected"), false),
        ] {
            let mut command = Command::new("sh");
            command.arg("-c").arg(MISS);
            match hit {
                Some(value) => {
                    command.env("HIT", value);
                }
                None => {
                    command.env_remove("HIT");
                }
            }
            assert_eq!(command.status()?.success(), expected, "HIT={hit:?}");
        }
        Ok(())
    }

    #[test]
    fn write_spaced_path_emits_a_line_terminated_payload() -> std::io::Result<()> {
        assert!(WRITE.contains("printf '%s\n' cache-ok"));

        let dir = std::env::temp_dir().join(format!(
            "velnor-empty-cache-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir)?;
        let status = Command::new("sh")
            .arg("-c")
            .arg(WRITE)
            .current_dir(&dir)
            .status();
        let contents = std::fs::read(dir.join("g4 cache/note.txt"));
        std::fs::remove_dir_all(&dir)?;

        assert!(status?.success());
        assert_eq!(contents?, b"cache-ok\n");
        Ok(())
    }
}
