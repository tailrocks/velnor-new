//! Real mise binaries must honor both no-config selectors with malformed miserc files.
#![cfg(unix)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use velnor_actions_mise::command::IsolatedCommand;

const PINNED_MISE: &str = "2026.10.6";
const FIX_SOURCE_REV: &str = "dfe74a90b41603625ee6aabecb42f14a1f5eb0f6";
const FIX_SOURCE_VERSION: &str = "2026.10.1";
const BINARY_ENV: &str = "VELNOR_MISE_REGRESSION_BINARY";
const BINARY_SHA_ENV: &str = "VELNOR_MISE_REGRESSION_SHA256";
const SOURCE_REV_ENV: &str = "VELNOR_MISE_REGRESSION_SOURCE_REV";
const BAD_TOML: &str = "[invalid\n";
const EXEC_OUTPUT: &str = "velnor-miserc-exec-ok";

static FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
enum ConfigLayer {
    Project,
    Global,
    System,
}

#[derive(Clone, Copy)]
enum Selection {
    FlagOnly,
    EnvironmentOnly,
}

#[derive(Clone, Copy)]
enum Operation {
    Version,
    Exec,
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    nested: PathBuf,
    home: PathBuf,
    config: PathBuf,
    system: PathBuf,
    malformed: Option<PathBuf>,
}

struct MiseBinary {
    path: PathBuf,
    version: String,
}

type RawInvocation = (Vec<OsString>, Vec<(OsString, OsString)>);

impl Fixture {
    fn new(layer: Option<ConfigLayer>) -> Result<Self, String> {
        let id = FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("velnor-miserc-{}-{id}", std::process::id()));
        std::fs::create_dir(&base).map_err(|err| err.to_string())?;
        let mut fixture = Self {
            root: base.join("root"),
            nested: base.join("root/nested"),
            home: base.join("home"),
            config: base.join("config"),
            system: base.join("system"),
            base,
            malformed: None,
        };
        for dir in ["root/nested", "home", "config", "system"] {
            std::fs::create_dir_all(fixture.base.join(dir)).map_err(|err| err.to_string())?;
        }
        let malformed = match layer {
            Some(ConfigLayer::Project) => Some(fixture.root.join(".miserc.toml")),
            Some(ConfigLayer::Global) => Some(fixture.config.join("miserc.toml")),
            Some(ConfigLayer::System) => Some(fixture.system.join("miserc.toml")),
            None => None,
        };
        if let Some(path) = &malformed {
            std::fs::write(path, BAD_TOML).map_err(|err| err.to_string())?;
        }
        fixture.malformed = malformed;
        Ok(fixture)
    }

    fn cwd(&self, nested: bool) -> &Path {
        if nested { &self.nested } else { &self.root }
    }

    fn clean_env(&self) -> Vec<(OsString, OsString)> {
        [
            ("PATH", "/usr/bin:/bin".to_owned()),
            ("HOME", self.home.display().to_string()),
            ("MISE_CONFIG_DIR", self.config.display().to_string()),
            ("MISE_SYSTEM_CONFIG_DIR", self.system.display().to_string()),
        ]
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.base.starts_with(std::env::temp_dir()) {
            eprintln!("refuse fixture cleanup outside temp");
            return;
        }
        if let Err(err) = std::fs::remove_dir_all(&self.base) {
            eprintln!("fixture cleanup failed: {err}");
        }
    }
}

impl MiseBinary {
    fn select() -> Result<Self, String> {
        let explicit_binary = std::env::var_os(BINARY_ENV).is_some();
        let path = match std::env::var_os(BINARY_ENV) {
            Some(path) => PathBuf::from(path),
            None => find_on_path("mise")?,
        };
        let path = path.canonicalize().map_err(|err| err.to_string())?;
        if !path.is_file() {
            return Err(format!("mise binary is not a file: {}", path.display()));
        }
        let sha256 = sha256_file(&path)?;
        let identity_fixture = Fixture::new(None)?;
        let version_result = run_raw(
            &path,
            &identity_fixture,
            &identity_fixture.root,
            &["--version"],
            &[],
        );
        let version_output = version_result?;
        if !version_output.status.success() {
            return Err(format!(
                "mise --version failed: {}",
                output_text(&version_output)
            ));
        }
        let version_text = String::from_utf8_lossy(&version_output.stdout);
        let version = version_text
            .split_whitespace()
            .next()
            .ok_or_else(|| "mise --version returned no release identifier".to_owned())?
            .to_owned();
        let source_rev = std::env::var(SOURCE_REV_ENV).ok();
        if let Some(rev) = source_rev {
            if !explicit_binary || rev != FIX_SOURCE_REV || version != FIX_SOURCE_VERSION {
                return Err(format!("unexpected fixed-source identity: {rev} {version}"));
            }
            let expected = std::env::var(BINARY_SHA_ENV)
                .map_err(|_| format!("{BINARY_SHA_ENV} required for source binary"))?;
            if sha256 != expected {
                return Err(format!(
                    "source binary digest mismatch: {sha256} != {expected}"
                ));
            }
            eprintln!("mise source={rev} version={version} sha256={sha256}");
        } else {
            if !explicit_binary && version != PINNED_MISE {
                return Err(format!(
                    "PATH mise {version} does not match pin {PINNED_MISE}"
                ));
            }
            let expected = official_release_digest(&version)?;
            if sha256 != expected {
                return Err(format!(
                    "official mise digest mismatch: {sha256} != {expected}"
                ));
            }
            eprintln!("mise official_release={version} sha256={sha256}");
        }
        Ok(Self { path, version })
    }
}

fn find_on_path(program: &str) -> Result<PathBuf, String> {
    std::env::split_paths(&std::env::var_os("PATH").ok_or_else(|| "PATH is unset".to_owned())?)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| format!("{program} not found on PATH"))
}

fn official_release_digest(version: &str) -> Result<&'static str, String> {
    match (version, std::env::consts::OS, std::env::consts::ARCH) {
        ("2026.10.6", "linux", "x86_64") => {
            Ok("3f44343eebc7e0d6623bcea46e304864f02dff648edd75c82871b53cc697b366")
        }
        ("2026.10.6", "macos", "aarch64") => {
            Ok("bbcea7b0f844d026424a4c8335357a15a2f5c9e9132c9408de990d9be6f26101")
        }
        _ => Err(format!(
            "unqualified mise release/platform: {version} {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    for (program, args) in [("shasum", vec!["-a", "256"]), ("sha256sum", vec![])] {
        let output = match Command::new(program).args(args).arg(path).output() {
            Ok(output) if output.status.success() => output,
            _ => continue,
        };
        let line = String::from_utf8_lossy(&output.stdout);
        let digest = line
            .split_whitespace()
            .next()
            .filter(|text| text.len() == 64 && text.chars().all(|ch| ch.is_ascii_hexdigit()))
            .ok_or_else(|| format!("invalid SHA-256 output from {program}: {line}"))?;
        return Ok(digest.to_ascii_lowercase());
    }
    Err("neither shasum nor sha256sum could hash the mise executable".to_owned())
}

fn run_raw(
    binary: &Path,
    fixture: &Fixture,
    cwd: &Path,
    args: &[impl AsRef<std::ffi::OsStr>],
    overlay: &[(OsString, OsString)],
) -> Result<Output, String> {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(fixture.clean_env())
        .envs(overlay.iter().cloned());
    command.output().map_err(|err| err.to_string())
}

fn selected_args(selection: Selection, operation: Operation) -> Vec<OsString> {
    let mut args = match operation {
        // The fixed 2026.10.1 CLI rejects `--no-config --version`; retain both supported forms.
        Operation::Version if matches!(selection, Selection::FlagOnly) => {
            vec![OsString::from("version")]
        }
        Operation::Version => vec![OsString::from("--version")],
        Operation::Exec => vec![
            OsString::from("exec"),
            OsString::from("--"),
            OsString::from("/bin/echo"),
            OsString::from(EXEC_OUTPUT),
        ],
    };
    if matches!(selection, Selection::FlagOnly) {
        args.insert(0, OsString::from("--no-config"));
    }
    args
}

fn selected_env(selection: Selection) -> Vec<(OsString, OsString)> {
    matches!(selection, Selection::EnvironmentOnly)
        .then(|| vec![(OsString::from("MISE_NO_CONFIG"), OsString::from("1"))])
        .unwrap_or_default()
}

fn assert_miserc_error(output: &Output, fixture: &Fixture) -> Result<(), String> {
    let expected = fixture
        .malformed
        .as_ref()
        .ok_or_else(|| "control fixture has no malformed miserc".to_owned())?
        .canonicalize()
        .map_err(|err| err.to_string())?;
    let text = output_text(output);
    let reported = text.lines().find_map(|line| {
        let location = line.split_once("╭─[")?.1.split_once(']')?.0;
        let path = location.rsplit_once(':')?.0.rsplit_once(':')?.0;
        Path::new(path).canonicalize().ok()
    });
    let has_parse_error = text.contains("Invalid TOML in config file");
    if output.status.success() || !has_parse_error || reported.as_ref() != Some(&expected) {
        return Err(format!("{text}\nexpected malformed {}", expected.display()));
    }
    Ok(())
}

fn assert_no_config_behavior(
    binary: &MiseBinary,
    output: &Output,
    _fixture: &Fixture,
    operation: Operation,
) -> Result<(), String> {
    if !output.status.success() {
        return Err(format!(
            "Mise config isolation failed: {}",
            output_text(output)
        ));
    }
    let expected = match operation {
        Operation::Version => binary.version.as_str(),
        Operation::Exec => EXEC_OUTPUT,
    };
    let actual = output_text(output);
    if !actual.contains(expected) {
        return Err(format!("expected {expected:?}, got {actual}"));
    }
    Ok(())
}

fn output_text(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

fn production_invocation() -> Result<RawInvocation, String> {
    let payload = [OsString::from("/bin/echo"), OsString::from(EXEC_OUTPUT)];
    let command = IsolatedCommand::mise_exec(&[], &payload).map_err(|err| err.to_string())?;
    let argv = command.argv();
    let actual: Vec<String> = argv
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    if actual.join(" ")
        != "mise --no-config --no-env --no-hooks exec -- /bin/echo velnor-miserc-exec-ok"
    {
        return Err(format!("production argv changed: {actual:?}"));
    }
    let environment = command.full_env();
    for (key, value) in [
        ("MISE_NO_CONFIG", "1"),
        ("MISE_NO_ENV", "1"),
        ("MISE_NO_HOOKS", "1"),
        ("MISE_LOCKFILE", "0"),
        ("MISE_AUTO_INSTALL", "false"),
        ("MISE_EXEC_AUTO_INSTALL", "false"),
    ] {
        if !environment
            .iter()
            .any(|(seen_key, seen_value)| seen_key == key && seen_value == value)
        {
            return Err(format!("production env missing {key}={value}"));
        }
    }
    Ok((argv.into_iter().skip(1).collect(), environment))
}

#[test]
fn real_mise_miserc_isolation_handles_the_current_release() -> Result<(), String> {
    let binary = MiseBinary::select()?;
    let (production_args, production_env) = production_invocation()?;
    eprintln!("mise binary path={}", binary.path.display());
    for layer in [
        ConfigLayer::Project,
        ConfigLayer::Global,
        ConfigLayer::System,
    ] {
        let fixture = Fixture::new(Some(layer))?;
        for nested in [false, true] {
            let cwd = fixture.cwd(nested);
            let control = run_raw(
                &binary.path,
                &fixture,
                cwd,
                &[OsString::from("--version")],
                &[],
            )?;
            assert_miserc_error(&control, &fixture)?;
            for selection in [Selection::FlagOnly, Selection::EnvironmentOnly] {
                for operation in [Operation::Version, Operation::Exec] {
                    let overlay = selected_env(selection);
                    let args = selected_args(selection, operation);
                    let has_flag = args.iter().any(|arg| arg == "--no-config");
                    let has_env = overlay
                        .iter()
                        .any(|(key, value)| key == "MISE_NO_CONFIG" && value == "1");
                    if has_flag != matches!(selection, Selection::FlagOnly)
                        || has_env != matches!(selection, Selection::EnvironmentOnly)
                    {
                        return Err("no-config selectors were not isolated".to_owned());
                    }
                    let output = run_raw(&binary.path, &fixture, cwd, &args, &overlay)?;
                    assert_no_config_behavior(&binary, &output, &fixture, operation)?;
                }
            }
            let output = run_raw(
                &binary.path,
                &fixture,
                cwd,
                &production_args,
                &production_env,
            )?;
            assert_no_config_behavior(&binary, &output, &fixture, Operation::Exec)?;
        }
    }
    Ok(())
}
