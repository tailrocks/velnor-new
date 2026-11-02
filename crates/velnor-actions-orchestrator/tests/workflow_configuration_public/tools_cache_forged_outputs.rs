// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::render_staged_tree;

use super::ledger_cli_verify_script::{RUNNER_DEFAULT_SHELL, RenderedVerify, rendered_step};
use super::tools_cache_verify_hook::{
    Attack, Captured, Env, RUNNER_BASH, Sandbox, expand_shell, pair, step_in,
};
use super::tools_producers_donors::prepared_for;

const SEED_ACTION: &str = ".github/actions/velnor-tool-seed/action.yml";
const SEED_STEP: &str = "Copy matching tool seed";
const IDENTITY_STEP: &str = "V2 identity";
const IDENTITY_SCRIPT: &str = ".github/scripts/velnor-tools-cache-identity.sh";
const ZERO_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const IDENTITY_LANES: [(&str, &str, &str); 3] = [
    ("u22", "ubuntu-22.04", "ubuntu22"),
    ("u24", "ubuntu-24.04", "ubuntu24"),
    ("u26", "ubuntu-26.04", "ubuntu26"),
];

fn step_was_found(step: &RenderedVerify) -> bool {
    step.shell != RUNNER_DEFAULT_SHELL && !step.run.is_empty()
}

fn forged_echo() -> Env {
    vec![pair(
        "BASH_FUNC_echo%%",
        "() { builtin echo \"$@\"; printf 'seed_admitted=true\\n' >> \"$GITHUB_OUTPUT\"; }",
    )]
}

fn seed_run(
    runner: &Sandbox,
    shell: &str,
    step: &RenderedVerify,
    extra: &[(String, String)],
) -> Result<Captured, Box<dyn Error>> {
    let mut env = vec![pair("SEED_KEY", "")];
    env.extend_from_slice(extra);
    runner.run_with_output(shell, &step.run, &env)
}

#[test]
fn seed_copy_ignores_env_hooks_and_exported_functions() -> Result<(), Box<dyn Error>> {
    let step = step_in(SEED_ACTION, SEED_STEP)?;
    assert!(step_was_found(&step), "{}", step.shell);
    let runner = Sandbox::new()?;
    let product = expand_shell(&step.shell);
    let control = seed_run(&runner, &product, &step, &[])?;
    assert_eq!(control.github_output, "");
    assert_eq!(
        (control.code, control.stdout.as_str()),
        (Some(0), "tool seed disabled\n")
    );

    let forged = "seed_admitted=true\n";
    let armed = Attack::forging(forged)?;
    let unhardened = seed_run(&runner, RUNNER_BASH, &step, &armed.startup_env())?;
    assert_eq!(
        (unhardened.github_output.as_str(), armed.calls().as_str()),
        (forged, "hook\n")
    );

    let attack = Attack::forging(forged)?;
    let hooked = seed_run(&runner, &product, &step, &attack.startup_env())?;
    assert_eq!(hooked.github_output, "");
    assert_eq!(
        (hooked.code, hooked.stdout.as_str()),
        (Some(0), "tool seed disabled\n")
    );
    assert_eq!(attack.calls(), "");

    let function = seed_run(&runner, &product, &step, &forged_echo())?;
    assert_eq!(function.github_output, "");
    assert_eq!(function.code, Some(0));
    Ok(())
}

struct IdentityLane {
    runner: Sandbox,
    layout: TempDir,
    name: &'static str,
    image_os: &'static str,
    lane: &'static str,
}

impl IdentityLane {
    fn new(
        (name, lane, image_os): (&'static str, &'static str, &'static str),
        script: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let layout = tempfile::tempdir()?;
        fs::create_dir_all(layout.path().join(".github/actions").join(name))?;
        fs::create_dir_all(layout.path().join(".github/scripts"))?;
        fs::write(layout.path().join(IDENTITY_SCRIPT), script)?;
        Ok(Self {
            runner: Sandbox::new()?,
            layout,
            name,
            image_os,
            lane,
        })
    }

    fn env(&self, qualified: bool) -> Result<Env, Box<dyn Error>> {
        let temp = fs::canonicalize(self.runner.runner_temp.path())?;
        let home = fs::canonicalize(self.runner.home.path())?;
        let path = |dir: &Path| dir.display().to_string();
        let cargo = path(&temp.join("velnor/cargo"));
        let rustup = path(&temp.join("velnor/rustup"));
        let action = self.layout.path().join(".github/actions").join(self.name);
        let mut env = vec![
            pair("HOME", &path(&home)),
            pair("RUNNER_TEMP", &path(&temp)),
            pair("RUNNER_OS", "Linux"),
            pair("RUNNER_ARCH", "X64"),
            pair("VELNOR_CACHE_LANE", self.lane),
            pair("VELNOR_CACHE_STATIC_DIGEST", "x"),
            pair("GITHUB_ACTION_PATH", &path(&action)),
            pair("CARGO_HOME", &cargo),
            pair("MISE_CARGO_HOME", &cargo),
            pair("RUSTUP_HOME", &rustup),
            pair("MISE_RUSTUP_HOME", &rustup),
        ];
        if qualified {
            env.push(pair("ImageOS", self.image_os));
        }
        Ok(env)
    }

    fn run(
        &self,
        shell: &str,
        step: &RenderedVerify,
        qualified: bool,
        extra: &[(String, String)],
    ) -> Result<Captured, Box<dyn Error>> {
        let mut env = self.env(qualified)?;
        env.extend_from_slice(extra);
        self.runner.run_with_output(shell, &step.run, &env)
    }
}

fn identity_of(output: &str) -> Option<&str> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("identity="))
}

fn identity_shadow() -> Result<Attack, Box<dyn Error>> {
    let forged_sum = format!("echo {ZERO_DIGEST}  -");
    Attack::shadowing(&[
        ("sha256sum", &forged_sum),
        ("shasum", &forged_sum),
        ("mkdir", "exec /bin/mkdir \"$@\""),
        ("rm", "exec /bin/rm \"$@\""),
    ])
}

fn assert_prepended_path_is_ignored(
    host: &IdentityLane,
    step: &RenderedVerify,
    product: &str,
    genuine: &Captured,
    action: &str,
) -> Result<(), Box<dyn Error>> {
    let shadow = identity_shadow()?;
    let mut shadowed = host.env(true)?;
    shadowed.extend(shadow.path_env(step));
    let bare = format!(
        "/bin/bash {}",
        host.layout.path().join(IDENTITY_SCRIPT).display()
    );
    let planted_run = host.runner.run_with_output(RUNNER_BASH, &bare, &shadowed)?;
    assert_eq!(
        identity_of(&planted_run.github_output),
        Some(ZERO_DIGEST),
        "{action}: prepended PATH entry changed the cache identity: plant is not effective"
    );
    assert!(
        planted_run.github_output.contains("enabled=true\n"),
        "{action}: prepended PATH entry changed the cache identity: plant is not effective"
    );
    let planted_calls = shadow.calls();
    assert!(
        planted_calls.contains("sha256sum") && planted_calls.contains("mkdir"),
        "{action}: {planted_calls:?}"
    );

    let shadow = identity_shadow()?;
    let pathed = host.run(product, step, true, &shadow.path_env(step))?;
    assert_eq!(
        pathed.github_output, genuine.github_output,
        "{action}: prepended PATH entry changed the cache identity"
    );
    assert_eq!(
        (pathed.code, shadow.calls().as_str()),
        (Some(0), ""),
        "{action}: prepended PATH entry changed the cache identity"
    );
    Ok(())
}

#[test]
fn cache_identity_ignores_env_hooks_and_exported_functions() -> Result<(), Box<dyn Error>> {
    let (_repo, prepared) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    let tree = render_staged_tree(&prepared)?;
    let script = tree
        .get(IDENTITY_SCRIPT)
        .ok_or("identity script is missing")?;
    let forged = format!("identity={ZERO_DIGEST}\nenabled=true\n");
    let mut lanes = 0;
    for lane in IDENTITY_LANES {
        let action = format!(".github/actions/{}/action.yml", lane.0);
        let Some(text) = tree.get(&action) else {
            continue;
        };
        lanes += 1;
        let step = rendered_step(text, IDENTITY_STEP)?;
        assert!(step_was_found(&step), "{action}: {}", step.shell);
        let host = IdentityLane::new(lane, script)?;
        let product = expand_shell(&step.shell);

        let genuine = host.run(&product, &step, true, &[])?;
        let digest = identity_of(&genuine.github_output).ok_or("no identity")?;
        assert_eq!(genuine.code, Some(0), "{action}");
        assert!(
            digest.len() == 64 && digest != ZERO_DIGEST,
            "{action}: {digest}"
        );
        assert!(
            genuine
                .github_output
                .contains("enabled=true\nreason=qualified_hosted_image\n")
        );
        let unqualified = host.run(&product, &step, false, &[])?;
        let expected = "enabled=false\nreason=image_os_missing\n";
        assert_eq!(
            (unqualified.code, unqualified.github_output.as_str()),
            (Some(0), expected)
        );

        let armed = Attack::forging(&forged)?;
        let unhardened = host.run(RUNNER_BASH, &step, false, &armed.startup_env())?;
        assert_eq!(
            (unhardened.github_output.as_str(), armed.calls().as_str()),
            (forged.as_str(), "hook\n")
        );

        let attack = Attack::forging(&forged)?;
        let hooked = host.run(&product, &step, false, &attack.startup_env())?;
        assert_eq!(hooked.github_output, expected, "{action}");
        assert_eq!(
            (hooked.code, attack.calls().as_str()),
            (Some(0), ""),
            "{action}"
        );

        assert_prepended_path_is_ignored(&host, &step, &product, &genuine, &action)?;

        let attack = Attack::new()?;
        let functions = host.run(&product, &step, true, &attack.function_env(ZERO_DIGEST))?;
        assert_eq!(functions.github_output, genuine.github_output, "{action}");
        assert_eq!(
            (functions.code, attack.calls().as_str()),
            (Some(0), ""),
            "{action}"
        );
    }
    assert!(lanes > 0);
    Ok(())
}
