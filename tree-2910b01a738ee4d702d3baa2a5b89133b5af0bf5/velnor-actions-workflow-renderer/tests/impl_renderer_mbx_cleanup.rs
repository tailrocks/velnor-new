//! Runtime regression for the generated MBX pre-post cleanup command.

use super::impl_renderer_mbx_bundle::render_mbx;

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};

fn step_block<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("name: {name}"))?;
    let tail = &text[start..];
    let end = tail.find("\n      - name:").unwrap_or(tail.len());
    Some(&tail[..end])
}

#[cfg(unix)]
fn decode_yaml_double_quoted(value: &str) -> std::io::Result<String> {
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or_else(|| std::io::Error::other("expected a double-quoted YAML scalar"))?;
    let mut decoded = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let escaped = chars
            .next()
            .ok_or_else(|| std::io::Error::other("trailing YAML escape"))?;
        decoded.push(match escaped {
            '\\' => '\\',
            '"' => '"',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            other => {
                return Err(std::io::Error::other(format!(
                    "unsupported YAML escape: \\{other}"
                )));
            }
        });
    }
    Ok(decoded)
}

#[cfg(unix)]
fn set_mode(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(mode);
    std::fs::set_permissions(path, permissions)
}

#[cfg(unix)]
fn mode(path: &std::path::Path) -> std::io::Result<u32> {
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
}

#[cfg(unix)]
struct CleanupFixture {
    runner_temp: std::path::PathBuf,
    out_dirs: std::path::PathBuf,
    readonly: std::path::PathBuf,
    nested: std::path::PathBuf,
    readonly_file: std::path::PathBuf,
    deep_directories: Vec<std::path::PathBuf>,
    external: std::path::PathBuf,
    bundle: std::path::PathBuf,
    untrusted_cache: std::path::PathBuf,
}

#[cfg(unix)]
fn make_cleanup_fixture(scratch: &std::path::Path) -> std::io::Result<CleanupFixture> {
    let runner_temp = scratch.join("runner-temp");
    std::fs::create_dir_all(&runner_temp)?;
    let store = runner_temp.join("mbx-github-objects-store-test");
    let out_dirs = store.join("store").join("out-dirs");
    let readonly = out_dirs.join("readonly");
    let nested = readonly.join("nested");
    std::fs::create_dir_all(&nested)?;
    let readonly_file = nested.join("payload");
    std::fs::write(&readonly_file, b"cached object")?;
    set_mode(&readonly_file, 0o444)?;

    let mut deep_directories = Vec::new();
    let mut deep_path = out_dirs.join("deep");
    std::fs::create_dir(&deep_path)?;
    for _ in 0..100 {
        deep_directories.push(deep_path.clone());
        deep_path = deep_path.join("d");
        std::fs::create_dir(&deep_path)?;
    }
    deep_directories.push(deep_path);

    let external = scratch.join("outside-target");
    std::fs::create_dir_all(&external)?;
    symlink(&external, out_dirs.join("external-link"))?;
    let bundle = runner_temp.join("mbx-github-objects-bundle-v1");
    std::fs::create_dir(&bundle)?;
    let untrusted_cache = scratch.join("untrusted-cache-dir");
    std::fs::create_dir(&untrusted_cache)?;

    for directory in [
        &nested,
        &readonly,
        &out_dirs,
        &external,
        &bundle,
        &untrusted_cache,
    ] {
        set_mode(directory, 0o555)?;
    }
    for directory in &deep_directories {
        set_mode(directory, 0o555)?;
    }
    Ok(CleanupFixture {
        runner_temp,
        out_dirs,
        readonly,
        nested,
        readonly_file,
        deep_directories,
        external,
        bundle,
        untrusted_cache,
    })
}

#[cfg(unix)]
#[test]
fn rendered_hosted_cleanup_repairs_only_action_owned_read_only_directories()
-> Result<(), Box<dyn std::error::Error>> {
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let text = render_mbx("cleanup-script", false)?;
    let normalize = step_block(&text, "Normalize MBX directories before action post")
        .ok_or_else(|| std::io::Error::other("rendered step is missing"))?;
    let run_scalar = normalize
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or_else(|| std::io::Error::other("rendered step has no run command"))?;
    let command = decode_yaml_double_quoted(run_scalar)?;
    assert!(
        command.contains("python3 -c") && command.contains("exec(\"\"\""),
        "{command}"
    );

    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let scratch = std::env::temp_dir().canonicalize()?.join(format!(
        "velnor-mbx-cleanup-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&scratch)?;
    let fixture = make_cleanup_fixture(&scratch)?;
    assert_eq!(mode(&fixture.out_dirs)?, 0o555);
    assert_eq!(mode(&fixture.deep_directories[100])?, 0o555);

    let output = Command::new("bash")
        .arg("-c")
        .arg(format!("ulimit -n 64; {command}"))
        .env("RUNNER_TEMP", &fixture.runner_temp)
        .env("MBX_CACHE_DIR", &fixture.untrusted_cache)
        .output()?;
    assert!(
        output.status.success(),
        "rendered command failed: {output:?}"
    );
    assert_eq!(mode(&fixture.out_dirs)?, 0o755);
    assert_eq!(mode(&fixture.readonly)?, 0o755);
    assert_eq!(mode(&fixture.nested)?, 0o755);
    assert_eq!(mode(&fixture.readonly_file)?, 0o444);
    for directory in &fixture.deep_directories {
        assert_eq!(mode(directory)?, 0o755);
    }
    assert_eq!(mode(&fixture.external)?, 0o555, "symlink was followed");
    assert_eq!(mode(&fixture.bundle)?, 0o555, "bundle was modified");
    assert_eq!(mode(&fixture.untrusted_cache)?, 0o555, "cache env was used");

    for directory in [&fixture.external, &fixture.bundle, &fixture.untrusted_cache] {
        set_mode(directory, 0o755)?;
    }
    std::fs::remove_dir_all(scratch)?;
    Ok(())
}
