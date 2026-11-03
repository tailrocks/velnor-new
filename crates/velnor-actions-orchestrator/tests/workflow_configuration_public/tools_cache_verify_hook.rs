// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::render_staged_tree;

use super::ledger_cli_verify_script::{
    BYTES, BYTES_SHA256, Planted, RenderedVerify, TAMPERED, rendered_step, resolve_on,
};
use super::tools_cache_install_run::{Install, run_install};
use super::tools_cache_pin_fixture::{
    HttpSpec, MACHETE_BINARY, MACHETE_DIR, MACHETE_VERSION, assert_urls_carry_pinned_version,
    machete_archive, pins_with_fixture_sha, rendered_http_specs,
};
use super::tools_producers_donors::prepared_for;

const RESTORE_ACTION: &str = ".github/actions/velnor-tools-cache-restore/action.yml";
const SCRUB_ACTION: &str = ".github/actions/velnor-tools-cache-scrub/action.yml";
const ADMISSION_STEP: &str = "Keep only an exact cache hit or validated seed on a miss";
const REVERIFY_STEP: &str = "Re-verify restored Mise installs";
const SYSTEM_PATH: &str = "/usr/bin:/bin";
pub(super) const RUNNER_BASH: &str = "bash --noprofile --norc -e -o pipefail {0}";

pub(super) type Env = Vec<(String, String)>;

pub(super) fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

pub(super) struct Sandbox {
    pub(super) home: TempDir,
    pub(super) runner_temp: TempDir,
    pub(super) workspace: TempDir,
}

impl Sandbox {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            home: tempfile::tempdir()?,
            runner_temp: tempfile::tempdir()?,
            workspace: tempfile::tempdir()?,
        })
    }

    pub(super) fn mise_root(&self) -> PathBuf {
        self.home.path().join(".local/share/mise")
    }

    fn base_env(&self) -> Env {
        vec![
            pair("HOME", &self.home.path().display().to_string()),
            pair(
                "RUNNER_TEMP",
                &self.runner_temp.path().display().to_string(),
            ),
            pair(
                "GITHUB_WORKSPACE",
                &self.workspace.path().display().to_string(),
            ),
        ]
    }

    fn run(
        &self,
        step: &RenderedVerify,
        extra: &[(String, String)],
    ) -> Result<Option<i32>, Box<dyn Error>> {
        Ok(self
            .spawn(&expand_shell(&step.shell), &step.run, extra)?
            .code)
    }

    pub(super) fn run_with_output(
        &self,
        shell: &str,
        run: &str,
        extra: &[(String, String)],
    ) -> Result<Captured, Box<dyn Error>> {
        let output = self.workspace.path().join("github_output");
        fs::write(&output, "")?;
        let mut env = vec![pair("GITHUB_OUTPUT", &output.display().to_string())];
        env.extend_from_slice(extra);
        let mut captured = self.spawn(shell, run, &env)?;
        captured.github_output = fs::read_to_string(&output)?;
        Ok(captured)
    }

    pub(super) fn spawn(
        &self,
        shell: &str,
        run: &str,
        extra: &[(String, String)],
    ) -> Result<Captured, Box<dyn Error>> {
        let script = self.workspace.path().join("step.sh");
        fs::write(&script, run)?;
        let mut words = shell.split_whitespace();
        let program = resolve_on(words.next().ok_or("empty shell")?, SYSTEM_PATH)?;
        let output = Command::new(program)
            .args(words.map(|word| word.replace("{0}", &script.display().to_string())))
            .env_clear()
            .env("PATH", SYSTEM_PATH)
            .envs(self.base_env())
            .envs(extra.iter().map(|(name, value)| (name, value)))
            .current_dir(self.workspace.path())
            .output()?;
        Ok(Captured {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout)?,
            github_output: String::new(),
        })
    }
}

pub(super) struct Captured {
    pub(super) code: Option<i32>,
    pub(super) stdout: String,
    pub(super) github_output: String,
}

pub(super) fn expand_shell(shell: &str) -> String {
    if shell == "bash" {
        RUNNER_BASH.to_owned()
    } else {
        shell.to_owned()
    }
}

pub(super) struct Attack {
    planted: Planted,
    hook: PathBuf,
}

impl Attack {
    pub(super) fn new() -> Result<Self, Box<dyn Error>> {
        let planted = Planted::new(&[])?;
        let hook = planted.hook()?;
        Ok(Self { planted, hook })
    }

    pub(super) fn forging(payload: &str) -> Result<Self, Box<dyn Error>> {
        let planted = Planted::new(&[])?;
        let hook = planted.forging_hook(payload)?;
        Ok(Self { planted, hook })
    }

    pub(super) fn shadowing(tools: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let planted = Planted::new(tools)?;
        let hook = planted.hook()?;
        Ok(Self { planted, hook })
    }

    pub(super) fn path_env(&self, step: &RenderedVerify) -> Env {
        let tail = step.path.as_deref().unwrap_or(SYSTEM_PATH);
        let path = format!("{}:{tail}", self.planted.bin_dir().display());
        vec![pair("PATH", &path)]
    }

    pub(super) fn startup_env(&self) -> Env {
        let hook = self.hook.display().to_string();
        vec![pair("BASH_ENV", &hook), pair("ENV", &hook)]
    }

    pub(super) fn function_env(&self, digest: &str) -> Env {
        let function = format!(
            "() {{ echo hook >> \"{}\"; echo {digest}  forged; }}",
            self.planted.log().display()
        );
        ["sha256sum", "shasum"]
            .into_iter()
            .flat_map(|tool| {
                [
                    pair(&format!("BASH_FUNC_{tool}%%"), &function),
                    pair(tool, &function),
                ]
            })
            .collect()
    }

    pub(super) fn calls(&self) -> String {
        self.planted.calls()
    }
}

pub(super) fn step_in(action: &str, name: &str) -> Result<RenderedVerify, Box<dyn Error>> {
    let (_repo, prepared) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    let tree = render_staged_tree(&prepared)?;
    rendered_step(tree.get(action).ok_or("action is missing")?, name)
}

fn admission_exit(
    step: &RenderedVerify,
    binary: &[u8],
    extra: &[(String, String)],
) -> Result<Option<i32>, Box<dyn Error>> {
    let runner = Sandbox::new()?;
    let bin = runner.mise_root().join("bin");
    fs::create_dir_all(&bin)?;
    fs::write(bin.join("mise"), binary)?;
    let key = format!("mise-tools-v2-{}", "a".repeat(64));
    let mut env = vec![
        pair("TOOLS_CACHE_HIT", "true"),
        pair("TOOLS_EXPECTED_KEY", &key),
        pair("TOOLS_MATCHED_KEY", &key),
        pair("TOOLS_SEED_ADMITTED", "false"),
        pair("TOOLS_MISE_SHA256", BYTES_SHA256),
    ];
    env.extend_from_slice(extra);
    runner.run(step, &env)
}

#[test]
fn admission_ignores_env_hooks_and_exported_functions() -> Result<(), Box<dyn Error>> {
    for action in [RESTORE_ACTION, SCRUB_ACTION] {
        let step = step_in(action, ADMISSION_STEP)?;
        assert_eq!(admission_exit(&step, BYTES, &[])?, Some(0), "{action}");
        assert_eq!(admission_exit(&step, TAMPERED, &[])?, Some(1), "{action}");
        let attack = Attack::new()?;
        let mut env = attack.startup_env();
        env.extend(attack.function_env(BYTES_SHA256));
        let exit = admission_exit(&step, TAMPERED, &env)?;
        assert_eq!((exit, attack.calls().as_str()), (Some(1), ""), "{action}");
        let digest = format!("echo {BYTES_SHA256}  -");
        let shadow = Attack::shadowing(&[("sha256sum", &digest), ("shasum", &digest)])?;
        let exit = admission_exit(&step, TAMPERED, &shadow.path_env(&step))?;
        assert_eq!((exit, shadow.calls().as_str()), (Some(1), ""), "{action}");
    }
    Ok(())
}

fn install_outcome(
    step: &RenderedVerify,
    install: &[u8],
    extra: &[(String, String)],
) -> Result<(Option<i32>, bool), Box<dyn Error>> {
    let pins = format!("fake 1.0.0 linux-x64 https://example.invalid/fake.bin {BYTES_SHA256}");
    let outcome = run_install(
        step,
        &Install {
            dir: "fake",
            version: "1.0.0",
            pins: &pins,
            lock: None,
            archive_sha256: BYTES_SHA256,
            archive: BYTES,
            extra_archives: &[],
            file: install,
            links: &[],
        },
        extra,
    )?;
    Ok((outcome.exit, outcome.kept))
}

#[test]
fn reverify_discards_a_tampered_install_despite_env_hooks() -> Result<(), Box<dyn Error>> {
    let step = step_in(RESTORE_ACTION, REVERIFY_STEP)?;
    assert_eq!(install_outcome(&step, BYTES, &[])?, (Some(0), true));
    assert_eq!(install_outcome(&step, TAMPERED, &[])?, (Some(0), false));
    let attack = Attack::new()?;
    let hooked = attack.startup_env();
    let outcome = install_outcome(&step, TAMPERED, &hooked)?;
    assert_eq!((outcome, attack.calls().as_str()), ((Some(0), false), ""));
    let outcome = install_outcome(&step, BYTES, &hooked)?;
    assert_eq!((outcome, attack.calls().as_str()), ((Some(0), true), ""));
    let digest = format!("echo {BYTES_SHA256}  -");
    let shadow = Attack::shadowing(&[("sha256sum", &digest), ("shasum", &digest)])?;
    let shadowed = shadow.path_env(&step);
    let outcome = install_outcome(&step, TAMPERED, &shadowed)?;
    assert_eq!((outcome, shadow.calls().as_str()), ((Some(0), false), ""));
    let outcome = install_outcome(&step, BYTES, &shadowed)?;
    assert_eq!((outcome, shadow.calls().as_str()), ((Some(0), true), ""));
    Ok(())
}

#[test]
fn catalog_pins_match_every_installed_http_validator() -> Result<(), Box<dyn Error>> {
    let specs = rendered_http_specs()?;
    let step = step_in(RESTORE_ACTION, REVERIFY_STEP)?;
    let pins = step
        .tool_pins
        .ok_or("restore step has no VELNOR_TOOL_PINS")?;
    let mut rows: Vec<&str> = pins
        .lines()
        .filter(|row| row.starts_with("http-"))
        .collect();
    rows.sort_unstable();
    let expected: Vec<String> = specs.iter().map(HttpSpec::row).collect();
    for name in ["cargo-machete", "lychee"] {
        assert!(
            specs.iter().any(|spec| spec.name == name),
            "no rendered job installs {name}: {expected:?}"
        );
    }
    assert_eq!(rows, expected);
    assert_urls_carry_pinned_version(&rows)
}

#[test]
fn restored_cargo_machete_install_is_kept_when_it_matches_its_pinned_archive()
-> Result<(), Box<dyn Error>> {
    let step = step_in(RESTORE_ACTION, REVERIFY_STEP)?;
    let rendered = step
        .tool_pins
        .as_deref()
        .ok_or("restore step has no pins")?;
    let scratch = tempfile::tempdir()?;
    let (archive, sha256) = machete_archive(scratch.path(), MACHETE_BINARY)?;
    let pins = pins_with_fixture_sha(rendered, &sha256);
    let run = |file: &[u8]| {
        run_install(
            &step,
            &Install {
                dir: MACHETE_DIR,
                version: MACHETE_VERSION,
                pins: &pins,
                lock: None,
                archive_sha256: &sha256,
                archive: &archive,
                extra_archives: &[],
                file,
                links: &[],
            },
            &[],
        )
    };
    let kept = run(MACHETE_BINARY)?;
    assert_eq!(
        (kept.exit, kept.kept),
        (Some(0), true),
        "stdout: {}",
        kept.stdout
    );
    assert!(
        kept.stdout
            .contains("restored http-cargo-machete@0.9.2 matches its mise.lock archive"),
        "stdout: {}",
        kept.stdout
    );
    assert!(
        !kept.stdout.contains("discarding"),
        "stdout: {}",
        kept.stdout
    );
    let tampered = run(b"fixture cargo-machete binary, changed\n")?;
    assert_eq!((tampered.exit, tampered.kept), (Some(0), false));
    assert!(
        tampered
            .stdout
            .contains("a file differs from the pinned archive"),
        "stdout: {}",
        tampered.stdout
    );
    Ok(())
}
