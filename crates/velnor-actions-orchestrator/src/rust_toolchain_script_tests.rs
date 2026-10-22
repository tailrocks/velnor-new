//! The toolchain verification script, executed against a fixture Rust
//! release: three component tarballs behind a fake `curl`, a channel
//! manifest that pins their SHA-256, and an installed toolchain laid out the
//! way rustup lays it out.

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{NOT_CATALOGUED_CODE, ToolchainPhase, toolchain_script};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

static NEXT: AtomicU64 = AtomicU64::new(0);

const VERSION: &str = "1.0.0";
const HOST: &str = "x86_64-unknown-linux-gnu";
const DIST: &str = "https://static.rust-lang.org/dist/2026-01-01";

/// `(path, contents)` of one component file.
type File = (&'static str, &'static str);

/// `(rustup component, tarball subdirectory, files)`.
const COMPONENTS: [(&str, &str, &[File]); 3] = [
    (
        "rustc",
        "rustc",
        &[
            ("bin/rustc", "#!/bin/sh\necho genuine rustc\n"),
            ("lib/librustc_driver.so", "driver bytes\n"),
        ],
    ),
    (
        "cargo",
        "cargo",
        &[("bin/cargo", "#!/bin/sh\necho genuine cargo\n")],
    ),
    (
        "rust-std",
        "rust-std-x86_64-unknown-linux-gnu",
        &[(
            "lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd.rlib",
            "std\n",
        )],
    ),
];

/// Fake `curl`: serves `--output <file> <url>` from `$FIXTURE_DIST` by the
/// URL's basename and logs each URL.
const FAKE_CURL: &str = r#"#!/bin/sh
out=; url=
while [ $# -gt 0 ]; do
  case "$1" in --output) out=$2; shift ;; https://*) url=$1 ;; esac
  shift
done
printf '%s\n' "$url" >> "$FIXTURE_LOG"
cp "$FIXTURE_DIST/${url##*/}" "$out"
"#;

struct Fixture {
    root: PathBuf,
    digest: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.root));
    }
}

fn sha256(path: &Path) -> TestResult<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg("if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi")
        .arg("sh")
        .arg(path)
        .output()?;
    let text = String::from_utf8(output.stdout)?;
    Ok(text
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned())
}

fn write(path: &Path, text: &str) -> TestResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

impl Fixture {
    fn new(name: &str) -> TestResult<Self> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "velnor-rust-verify-{name}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        for dir in ["bin", "dist", "work", "runner-temp", "cargo/bin"] {
            fs::create_dir_all(root.join(dir))?;
        }
        let curl = root.join("bin/curl");
        fs::write(&curl, FAKE_CURL)?;
        fs::set_permissions(&curl, fs::Permissions::from_mode(0o755))?;
        let mut channel = String::from("manifest-version = \"2\"\n");
        let toolchain = root.join(format!("rustup/toolchains/{VERSION}-{HOST}"));
        let mut listed = String::new();
        for (component, sub, files) in COMPONENTS {
            let stem = format!("{component}-{VERSION}-{HOST}");
            let top = root.join("work").join(&stem);
            let mut manifest_in = String::new();
            for (path, text) in files {
                write(&top.join(sub).join(path), text)?;
                write(&toolchain.join(path), text)?;
                writeln!(manifest_in, "file:{path}")?;
            }
            write(&top.join(sub).join("manifest.in"), &manifest_in)?;
            write(&top.join("components"), &format!("{sub}\n"))?;
            write(&top.join("rust-installer-version"), "3\n")?;
            let tarball = root.join("dist").join(format!("{stem}.tar.xz"));
            let status = Command::new("tar")
                .arg("-cJf")
                .arg(&tarball)
                .arg("-C")
                .arg(root.join("work"))
                .arg(&stem)
                .status()?;
            if !status.success() {
                return Err("tar -cJf failed".into());
            }
            let pkg = if component == "rust-std" {
                "rust-std"
            } else {
                component
            };
            write!(
                channel,
                "\n[pkg.{pkg}.target.{HOST}]\navailable = true\nurl = \"{DIST}/{stem}.tar.gz\"\nhash = \"{}\"\nxz_url = \"{DIST}/{stem}.tar.xz\"\nxz_hash = \"{}\"\n",
                "0".repeat(64),
                sha256(&tarball)?
            )?;
            writeln!(listed, "{component}-{HOST}")?;
            write(
                &toolchain.join(format!("lib/rustlib/manifest-{component}-{HOST}")),
                &manifest_in,
            )?;
        }
        // A foreign target section the toolchain does not list.
        write!(
            channel,
            "\n[pkg.rustc.target.aarch64-apple-darwin]\navailable = true\nxz_url = \"{DIST}/absent.tar.xz\"\nxz_hash = \"{}\"\n",
            "1".repeat(64)
        )?;
        write(&toolchain.join("lib/rustlib/components"), &listed)?;
        write(&toolchain.join("lib/rustlib/rust-installer-version"), "3\n")?;
        write(
            &toolchain.join("lib/rustlib/multirust-channel-manifest.toml"),
            &channel,
        )?;
        write(&toolchain.join("lib/rustlib/multirust-config.toml"), "")?;
        let channel_path = root.join(format!("dist/channel-rust-{VERSION}.toml"));
        fs::write(&channel_path, &channel)?;
        let digest = sha256(&channel_path)?;
        write(
            &root.join("cargo/bin/rustup"),
            "#!/bin/sh\necho cached rustup\n",
        )?;
        Ok(Self { root, digest })
    }

    fn toolchain(&self) -> PathBuf {
        self.root
            .join(format!("rustup/toolchains/{VERSION}-{HOST}"))
    }

    fn run_with(&self, phase: ToolchainPhase, digest: &str, version: &str) -> TestResult<Output> {
        let script = toolchain_script(version, phase, &[(VERSION, digest, "2026-01-01")]);
        let inherited = std::env::var_os("PATH").ok_or("PATH is unavailable")?;
        let mut paths = vec![self.root.join("bin")];
        paths.extend(std::env::split_paths(&inherited));
        Ok(Command::new("sh")
            .arg("-c")
            .arg(&script)
            .env_clear()
            .env("PATH", std::env::join_paths(paths)?)
            .env("HOME", &self.root)
            .env("RUNNER_TEMP", self.root.join("runner-temp"))
            .env("RUSTUP_HOME", self.root.join("rustup"))
            .env("CARGO_HOME", self.root.join("cargo"))
            .env("FIXTURE_DIST", self.root.join("dist"))
            .env("FIXTURE_LOG", self.root.join("curl.log"))
            .output()?)
    }

    fn run(&self, phase: ToolchainPhase) -> TestResult<Output> {
        self.run_with(phase, &self.digest.clone(), VERSION)
    }

    fn downloads(&self) -> usize {
        fs::read_to_string(self.root.join("curl.log"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.ends_with(".tar.xz"))
            .count()
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn assert_fails(output: &Output, code: &str) {
    let log = text(output);
    assert!(!output.status.success(), "{code} expected:\n{log}");
    assert!(
        log.contains(&format!("::error::{code}: ")),
        "{code}:\n{log}"
    );
}

#[test]
fn a_genuine_toolchain_verifies_file_by_file() -> TestResult {
    let fixture = Fixture::new("genuine")?;
    let output = fixture.run(ToolchainPhase::Installed)?;
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("verified file by file"));
    assert_eq!(fixture.downloads(), 3, "one tarball per listed component");
    Ok(())
}

#[test]
fn a_replaced_rustc_fails() -> TestResult {
    // The review's reproduction: `bin/rustc` swapped for a script while
    // rustup's metadata stays intact passed the metadata-only check.
    let fixture = Fixture::new("rustc")?;
    fs::write(
        fixture.toolchain().join("bin/rustc"),
        "#!/bin/sh\necho pwned\n",
    )?;
    assert_fails(
        &fixture.run(ToolchainPhase::Installed)?,
        "rust_toolchain_bytes_mismatch",
    );
    Ok(())
}

#[test]
fn an_extra_file_or_a_component_dropped_from_the_list_fails() -> TestResult {
    let extra = Fixture::new("extra")?;
    write(&extra.toolchain().join("bin/cargo-evil"), "#!/bin/sh\n")?;
    assert_fails(
        &extra.run(ToolchainPhase::Installed)?,
        "rust_toolchain_bytes_mismatch",
    );
    let unlisted = Fixture::new("unlisted")?;
    fs::write(
        unlisted.toolchain().join("lib/rustlib/components"),
        format!("rustc-{HOST}\nrust-std-{HOST}\n"),
    )?;
    assert_fails(
        &unlisted.run(ToolchainPhase::Installed)?,
        "rust_toolchain_bytes_mismatch",
    );
    Ok(())
}

#[test]
fn a_missing_file_fails() -> TestResult {
    let fixture = Fixture::new("missing")?;
    fs::remove_file(fixture.toolchain().join("lib/librustc_driver.so"))?;
    assert_fails(
        &fixture.run(ToolchainPhase::Installed)?,
        "rust_toolchain_bytes_mismatch",
    );
    Ok(())
}

#[test]
fn a_symlink_in_the_toolchain_fails() -> TestResult {
    let fixture = Fixture::new("symlink")?;
    fs::remove_file(fixture.toolchain().join("bin/cargo"))?;
    symlink("/bin/sh", fixture.toolchain().join("bin/cargo"))?;
    assert_fails(
        &fixture.run(ToolchainPhase::Installed)?,
        "rust_toolchain_file_unexpected",
    );
    Ok(())
}

#[test]
fn a_tarball_that_differs_from_the_manifest_digest_fails() -> TestResult {
    let fixture = Fixture::new("tarball")?;
    let tarball = fixture
        .root
        .join(format!("dist/cargo-{VERSION}-{HOST}.tar.xz"));
    fs::write(&tarball, "not the pinned tarball")?;
    assert_fails(
        &fixture.run(ToolchainPhase::Installed)?,
        "rust_component_tarball_digest_mismatch",
    );
    Ok(())
}

#[test]
fn a_channel_manifest_that_differs_from_the_catalog_digest_fails() -> TestResult {
    let fixture = Fixture::new("channel")?;
    assert_fails(
        &fixture.run_with(ToolchainPhase::Installed, &"2".repeat(64), VERSION)?,
        "rust_channel_manifest_digest_mismatch",
    );
    let unknown = Fixture::new("uncatalogued")?;
    assert_fails(
        &unknown.run_with(ToolchainPhase::Installed, &unknown.digest.clone(), "1.0.1")?,
        NOT_CATALOGUED_CODE,
    );
    Ok(())
}

#[test]
fn a_component_the_manifest_does_not_ship_fails() -> TestResult {
    let fixture = Fixture::new("unshipped")?;
    fs::write(
        fixture.toolchain().join("lib/rustlib/components"),
        format!("rustc-{HOST}\ncargo-{HOST}\nrust-std-{HOST}\nmiri-{HOST}\n"),
    )?;
    assert_fails(
        &fixture.run(ToolchainPhase::Installed)?,
        "rust_component_not_in_manifest",
    );
    Ok(())
}

#[test]
fn the_installed_phase_requires_a_toolchain() -> TestResult {
    let fixture = Fixture::new("absent-installed")?;
    fs::remove_dir_all(fixture.toolchain())?;
    assert_fails(
        &fixture.run(ToolchainPhase::Installed)?,
        "rust_toolchain_missing",
    );
    Ok(())
}

#[test]
fn the_restored_phase_discards_the_cached_rustup_and_skips_an_empty_cache() -> TestResult {
    let fixture = Fixture::new("absent-restored")?;
    fs::remove_dir_all(fixture.toolchain())?;
    let output = fixture.run(ToolchainPhase::Restored)?;
    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("no restored Rust"));
    assert!(
        !fixture.root.join("cargo/bin").exists(),
        "the cached rustup and its proxies must never run"
    );
    Ok(())
}

#[test]
fn the_restored_phase_fails_on_a_poisoned_cache_before_anything_runs_it() -> TestResult {
    let fixture = Fixture::new("poisoned")?;
    fs::write(
        fixture.toolchain().join("bin/rustc"),
        "#!/bin/sh\necho pwned\n",
    )?;
    assert_fails(
        &fixture.run(ToolchainPhase::Restored)?,
        "rust_toolchain_bytes_mismatch",
    );
    Ok(())
}

#[test]
fn both_phases_share_one_download_per_component_in_a_job() -> TestResult {
    let fixture = Fixture::new("reuse")?;
    let restored = fixture.run(ToolchainPhase::Restored)?;
    assert!(restored.status.success(), "{}", text(&restored));
    let installed = fixture.run(ToolchainPhase::Installed)?;
    assert!(installed.status.success(), "{}", text(&installed));
    assert_eq!(fixture.downloads(), 3, "tarball sums are reused");
    // Reused sums still catch a file changed between the two phases.
    fs::write(fixture.toolchain().join("bin/cargo"), "changed\n")?;
    assert_fails(
        &fixture.run(ToolchainPhase::Installed)?,
        "rust_toolchain_bytes_mismatch",
    );
    Ok(())
}
