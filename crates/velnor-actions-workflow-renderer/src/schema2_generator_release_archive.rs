//! Strict checksum parsing and bounded validation of raw release artifacts.

const SIDECAR_AWK: &str = "NR == 1 { if (NF != 2 || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ || $2 != expected) exit 1; print $1; next } { exit 1 } END { if (NR != 1) exit 1 }";

/// Emit one AWK command that accepts only the candidate's exact checksum row.
pub(super) fn sidecar_digest_command(sidecar: &str, binary: &str) -> String {
    format!("awk -v expected='{binary}' '{SIDECAR_AWK}' '{sidecar}'")
}

/// Validate the complete payload after the pinned artifact action extracts its ZIP.
pub(super) fn raw_artifact_validation_script(
    directory: &str,
    binary: &str,
    sidecar: &str,
    provenance: &str,
) -> String {
    format!(
        r#"set -euo pipefail
dir='{directory}'
test -d "$dir"
test ! -L "$dir"
shopt -s dotglob nullglob
entries=("$dir"/*)
test "${{#entries[@]}}" -eq 3
total=0
for path in "${{entries[@]}}"; do
  case "$path" in
    "$dir/{binary}"|"$dir/{sidecar}"|"$dir/{provenance}") ;;
    *) echo 'unexpected release artifact entry' >&2; exit 1 ;;
  esac
  test -f "$path"
  test ! -L "$path"
  test -s "$path"
  test ! -x "$path"
  links="$(find "$path" -type f -links 1 -print -quit)"
  test "$links" = "$path"
  size="$(wc -c < "$path")"
  size="${{size//[[:space:]]/}}"
  [[ "$size" =~ ^[0-9]+$ ]] || exit 1
  test "$size" -le 268435456
  total=$((total + size))
  test "$total" -le 268435456
done"#
    )
}

#[cfg(test)]
mod tests {
    use super::{SIDECAR_AWK, raw_artifact_validation_script, sidecar_digest_command};
    use std::error::Error;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Result<Self, Box<dyn Error>> {
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir().join(format!(
                "velnor-raw-release-artifact-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn validation_command(scratch: &Scratch) -> Command {
        let script = raw_artifact_validation_script("assets", "cli", "cli.sha256", "cli.json");
        let mut command = Command::new("bash");
        command.args(["-c", &script]).current_dir(&scratch.0);
        command
    }

    fn prepare_payload(scratch: &Scratch) -> Result<PathBuf, Box<dyn Error>> {
        let directory = scratch.0.join("assets");
        fs::create_dir(&directory)?;
        for name in ["cli", "cli.sha256", "cli.json"] {
            fs::write(directory.join(name), b"valid payload\n")?;
        }
        Ok(directory)
    }

    #[test]
    fn raw_artifact_accepts_only_three_regular_single_link_nonempty_files()
    -> Result<(), Box<dyn Error>> {
        let scratch = Scratch::new()?;
        let directory = prepare_payload(&scratch)?;
        assert!(validation_command(&scratch).status()?.success());

        fs::remove_file(directory.join("cli.json"))?;
        assert!(!validation_command(&scratch).status()?.success());
        fs::write(directory.join("cli.json"), b"valid payload\n")?;

        fs::write(directory.join("cli.sha256"), b"")?;
        assert!(!validation_command(&scratch).status()?.success());
        fs::write(directory.join("cli.sha256"), b"valid payload\n")?;

        fs::set_permissions(directory.join("cli"), fs::Permissions::from_mode(0o755))?;
        assert!(!validation_command(&scratch).status()?.success());
        fs::set_permissions(directory.join("cli"), fs::Permissions::from_mode(0o644))?;

        fs::write(directory.join(".extra"), b"extra\n")?;
        assert!(!validation_command(&scratch).status()?.success());
        fs::remove_file(directory.join(".extra"))?;

        let nested = directory.join("nested");
        fs::create_dir(&nested)?;
        fs::write(nested.join("extra"), b"nested\n")?;
        assert!(!validation_command(&scratch).status()?.success());
        fs::remove_dir_all(nested)?;

        let link = directory.join("cli.json");
        fs::remove_file(&link)?;
        std::os::unix::fs::symlink(directory.join("cli"), &link)?;
        assert!(!validation_command(&scratch).status()?.success());
        fs::remove_file(link)?;

        let fifo = directory.join("cli.json");
        assert!(Command::new("mkfifo").arg(&fifo).status()?.success());
        assert!(!validation_command(&scratch).status()?.success());
        fs::remove_file(&fifo)?;

        fs::hard_link(directory.join("cli"), directory.join("cli.json"))?;
        assert!(!validation_command(&scratch).status()?.success());
        Ok(())
    }

    #[test]
    fn raw_artifact_rejects_failed_size_read_and_total_overflow() -> Result<(), Box<dyn Error>> {
        let scratch = Scratch::new()?;
        let directory = prepare_payload(&scratch)?;
        let fake_bin = scratch.0.join("fake-bin");
        fs::create_dir(&fake_bin)?;
        let fake_wc = fake_bin.join("wc");
        fs::write(&fake_wc, "#!/bin/sh\nexit 9\n")?;
        fs::set_permissions(&fake_wc, fs::Permissions::from_mode(0o755))?;
        let mut command = validation_command(&scratch);
        let mut paths = vec![fake_bin];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").ok_or("missing PATH")?,
        ));
        command.env("PATH", std::env::join_paths(paths)?);
        assert!(
            !command.status()?.success(),
            "failed wc must fail validation"
        );

        for name in ["cli", "cli.sha256", "cli.json"] {
            fs::File::options()
                .write(true)
                .open(directory.join(name))?
                .set_len(90_000_000)?;
        }
        assert!(
            !validation_command(&scratch).status()?.success(),
            "payload over aggregate byte limit must fail validation"
        );
        Ok(())
    }

    #[test]
    fn checksum_sidecar_must_name_the_exact_binary() -> Result<(), Box<dyn Error>> {
        for (row, accepted) in [
            (format!("{}  cli\n", "a".repeat(64)), true),
            (format!("{}  another-binary\n", "a".repeat(64)), false),
            (format!("{}  cli\n", "A".repeat(64)), false),
            (format!("{}  cli\nsecond-row\n", "a".repeat(64)), false),
        ] {
            let mut child = Command::new("awk")
                .args(["-v", "expected=cli", SIDECAR_AWK])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .spawn()?;
            child
                .stdin
                .take()
                .ok_or("missing AWK stdin")?
                .write_all(row.as_bytes())?;
            assert_eq!(child.wait()?.success(), accepted, "row: {row:?}");
        }
        assert!(sidecar_digest_command("cli.sha256", "cli").contains("expected='cli'"));
        Ok(())
    }
}
