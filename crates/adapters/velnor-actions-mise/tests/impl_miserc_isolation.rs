//! Regression coverage for the official Mise release that fixes
//! `--no-config` / `MISE_NO_CONFIG` handling of malformed `.miserc.toml`.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use velnor_actions_mise::command::IsolatedCommand;

const BINARY_ENV: &str = "VELNOR_MISE_FIXED_BINARY";
const FIXED_VERSION: &str = "2026.10.5";
const FIXED_BINARY_SHA256: &str =
    "8a223b5f8ca71100220a3e5bef259614c348e7b1d80e6b15c2a9c9aa3affe5e4";
const NO_CONFIG_ERROR: &str = "Invalid TOML in config file";
const EXEC_SENTINEL: &str = "velnor-miserc-fixed-release-ok";

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug)]
enum ConfigLayer {
    Project,
    Global,
    System,
}

#[derive(Clone, Copy, Debug)]
enum Selector {
    Flag,
    Environment,
}

#[derive(Clone, Copy, Debug)]
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
    cache: PathBuf,
    data: PathBuf,
    state: PathBuf,
    malformed: Option<PathBuf>,
}

impl Fixture {
    fn new(layer: Option<ConfigLayer>) -> Result<Self, String> {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "velnor-miserc-fixed-{}-{stamp}-{id}",
            std::process::id()
        ));
        std::fs::create_dir(&base).map_err(|error| error.to_string())?;
        let mut fixture = Self {
            root: base.join("repo"),
            nested: base.join("repo/nested"),
            home: base.join("home"),
            config: base.join("config"),
            system: base.join("system"),
            cache: base.join("cache"),
            data: base.join("data"),
            state: base.join("state"),
            base,
            malformed: None,
        };
        for directory in [
            &fixture.nested,
            &fixture.home,
            &fixture.config,
            &fixture.system,
            &fixture.cache,
            &fixture.data,
            &fixture.state,
        ] {
            std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        }
        fixture.malformed = match layer {
            Some(ConfigLayer::Project) => Some(fixture.root.join(".miserc.toml")),
            Some(ConfigLayer::Global) => Some(fixture.config.join("miserc.toml")),
            Some(ConfigLayer::System) => Some(fixture.system.join("miserc.toml")),
            None => None,
        };
        if let Some(path) = &fixture.malformed {
            std::fs::write(path, "[invalid\n").map_err(|error| error.to_string())?;
        }
        Ok(fixture)
    }

    fn invoke(
        &self,
        binary: &Path,
        cwd: &Path,
        selector: Option<Selector>,
        operation: Operation,
    ) -> Result<Output, String> {
        let mut args = Vec::new();
        if matches!(selector, Some(Selector::Flag)) {
            args.push("--no-config".into());
        }
        match operation {
            Operation::Version => args.push("version".into()),
            Operation::Exec => args.extend(
                ["exec", "--", "/bin/echo", EXEC_SENTINEL]
                    .into_iter()
                    .map(Into::into),
            ),
        }
        let overlay = matches!(selector, Some(Selector::Environment))
            .then(|| vec![("MISE_NO_CONFIG".into(), "1".into())])
            .unwrap_or_default();
        self.invoke_raw(binary, cwd, &args, &overlay)
    }

    fn invoke_raw(
        &self,
        binary: &Path,
        cwd: &Path,
        args: &[std::ffi::OsString],
        overlay: &[(std::ffi::OsString, std::ffi::OsString)],
    ) -> Result<Output, String> {
        let mut command = Command::new(binary);
        command
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("MISE_CONFIG_DIR", &self.config)
            .env("MISE_SYSTEM_CONFIG_DIR", &self.system)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("XDG_DATA_HOME", &self.data)
            .env("XDG_STATE_HOME", &self.state);
        command.envs(overlay.iter().cloned());
        command.output().map_err(|error| error.to_string())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let temp = std::env::temp_dir();
        if !self.base.starts_with(&temp) {
            eprintln!("refuse fixture cleanup outside temp");
            return;
        }
        if let Err(error) = std::fs::remove_dir_all(&self.base) {
            eprintln!("fixture cleanup failed: {error}");
        }
    }
}

fn fixed_official_binary() -> Result<PathBuf, String> {
    let supplied = std::env::var_os(BINARY_ENV).ok_or_else(|| {
        format!("NOT_RUN: set {BINARY_ENV} to the authenticated Mise v{FIXED_VERSION} binary")
    })?;
    let binary = PathBuf::from(supplied)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !std::fs::metadata(&binary)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err(format!(
            "Mise binary is not a regular file: {}",
            binary.display()
        ));
    }
    let digest = sha256_file(&binary)?;
    if digest != FIXED_BINARY_SHA256 {
        return Err(format!(
            "Mise v{FIXED_VERSION} binary digest mismatch: {digest}"
        ));
    }
    Ok(binary)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let hasher = ["/usr/bin/sha256sum", "/bin/sha256sum"]
        .into_iter()
        .find(|candidate| Path::new(candidate).is_file())
        .ok_or_else(|| "NOT_RUN: sha256sum is unavailable".to_owned())?;
    let output = Command::new(hasher)
        .arg("--")
        .arg(path)
        .env_clear()
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("sha256sum exited with {}", output.status));
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "sha256sum returned an invalid digest".to_owned())
}

fn output_text(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

fn production_invocation() -> Result<
    (
        Vec<std::ffi::OsString>,
        Vec<(std::ffi::OsString, std::ffi::OsString)>,
    ),
    String,
> {
    let payload = [
        std::ffi::OsString::from("/bin/echo"),
        std::ffi::OsString::from(EXEC_SENTINEL),
    ];
    let command = IsolatedCommand::mise_exec(&[], &payload).map_err(|error| error.to_string())?;
    let argv = command.argv();
    let actual = argv
        .iter()
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let expected =
        format!("mise --no-config --no-env --no-hooks exec -- /bin/echo {EXEC_SENTINEL}");
    if actual.join(" ") != expected {
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

fn project_cwds(layer: ConfigLayer, fixture: &Fixture) -> Vec<&Path> {
    match layer {
        ConfigLayer::Project => vec![&fixture.root, &fixture.nested],
        ConfigLayer::Global | ConfigLayer::System => vec![&fixture.root],
    }
}

fn assert_malformed_control(output: &Output, fixture: &Fixture) -> Result<(), String> {
    let expected = fixture
        .malformed
        .as_ref()
        .ok_or_else(|| "control fixture lacks malformed miserc".to_owned())?;
    let text = output_text(output);
    let expected_path = expected.display().to_string();
    if output.status.success() || !text.contains(NO_CONFIG_ERROR) || !text.contains(&expected_path)
    {
        return Err(format!(
            "unselected malformed config must fail at {expected_path}; got {text}"
        ));
    }
    Ok(())
}

fn assert_fixed_result(output: &Output, operation: Operation) -> Result<(), String> {
    if !output.status.success() {
        return Err(format!(
            "Mise v{FIXED_VERSION} no-config invocation failed: {}",
            output_text(output)
        ));
    }
    let expected = match operation {
        Operation::Version => FIXED_VERSION,
        Operation::Exec => EXEC_SENTINEL,
    };
    if !output_text(output).contains(expected) {
        return Err(format!(
            "expected {expected:?}, got {}",
            output_text(output)
        ));
    }
    Ok(())
}

#[test]
#[ignore = "requires VELNOR_MISE_FIXED_BINARY pointing to authenticated v2026.10.5 Linux x64"]
fn fixed_official_mise_honors_no_config_with_malformed_miserc() -> Result<(), String> {
    let binary = fixed_official_binary()?;
    let (production_args, production_env) = production_invocation()?;
    let identity = Fixture::new(None)?;
    let version = identity.invoke(&binary, &identity.root, None, Operation::Version)?;
    if !version.status.success() || !output_text(&version).contains(FIXED_VERSION) {
        return Err(format!(
            "unexpected official binary version: {}",
            output_text(&version)
        ));
    }
    eprintln!(
        "case=identity release=v{FIXED_VERSION} binary_sha256={FIXED_BINARY_SHA256} exit={:?} output={:?}",
        version.status.code(),
        output_text(&version)
    );
    for layer in [
        ConfigLayer::Project,
        ConfigLayer::Global,
        ConfigLayer::System,
    ] {
        let fixture = Fixture::new(Some(layer))?;
        for cwd in project_cwds(layer, &fixture) {
            let control = fixture.invoke(&binary, cwd, None, Operation::Version)?;
            assert_malformed_control(&control, &fixture)?;
            eprintln!(
                "case=malformed_control layer={layer:?} cwd={:?} exit={:?} output={:?}",
                cwd.strip_prefix(&fixture.base).unwrap_or(cwd),
                control.status.code(),
                output_text(&control)
            );
            for selector in [Selector::Flag, Selector::Environment] {
                for operation in [Operation::Version, Operation::Exec] {
                    let output = fixture.invoke(&binary, cwd, Some(selector), operation)?;
                    assert_fixed_result(&output, operation)?;
                    eprintln!(
                        "case=no_config_positive layer={layer:?} cwd={:?} selector={selector:?} operation={operation:?} exit={:?} output={:?}",
                        cwd.strip_prefix(&fixture.base).unwrap_or(cwd),
                        output.status.code(),
                        output_text(&output)
                    );
                }
            }
            let production = fixture.invoke_raw(&binary, cwd, &production_args, &production_env)?;
            assert_fixed_result(&production, Operation::Exec)?;
            eprintln!(
                "case=product_isolated_command layer={layer:?} cwd={:?} exit={:?} output={:?}",
                cwd.strip_prefix(&fixture.base).unwrap_or(cwd),
                production.status.code(),
                output_text(&production)
            );
        }
    }
    Ok(())
}
