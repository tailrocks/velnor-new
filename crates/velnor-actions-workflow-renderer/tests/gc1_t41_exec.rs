// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use velnor_actions_contract::Step;

use crate::gc1_t41::render;

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn rendered_run(yaml: &str, step_name: &str) -> Result<String, Box<dyn Error>> {
    let step_header = format!("      - name: {step_name}\n");
    let step = yaml
        .split_once(&step_header)
        .map(|(_, step)| step)
        .ok_or_else(|| fail(&format!("rendered workflow lost step:{step_name}")))?;
    let run = step
        .lines()
        .find_map(|line| line.strip_prefix("        run: "))
        .ok_or_else(|| fail(&format!("rendered step lost run field:{step_name}")))?;
    if run.starts_with('"') {
        Ok(serde_json::from_str(run)?)
    } else {
        Ok(run.to_owned())
    }
}

fn execute_rendered_run(
    run: &str,
    script_env: &[(&str, &str)],
) -> Result<(u32, Vec<u8>), Box<dyn Error>> {
    let mut command = std::process::Command::new("sh");
    command
        .arg("-c")
        .arg(run)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for key in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        command.env(key, format!("sentinel-{key}"));
    }
    command.env("VELNOR_TEST_CONTROL", "control-value");
    for (key, value) in script_env {
        command.env(key, value);
    }
    let child = command.spawn()?;
    let pid = child.id();
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(fail(&format!(
            "rendered run {run:?} failed with status {:?} and stderr {:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok((pid, output.stdout))
}

fn shell_step(name: &str, argv: &[&str]) -> Result<Step, Box<dyn Error>> {
    Ok(velnor_actions_workflow_renderer::steps::shell_step(
        name,
        argv.iter().map(|arg| (*arg).to_owned()).collect(),
        BTreeMap::default(),
    )?)
}

#[test]
fn gc1_t41_exec_scrubs_credentials_and_keeps_controls() -> Result<(), Box<dyn Error>> {
    let credential_names =
        velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS.join(" ");
    let script = "for key in $VELNOR_TEST_CHECK_VARS; do if printenv \"$key\" >/dev/null; then exit 21; fi; done; [ \"$VELNOR_TEST_CONTROL\" = control-value ] && printf '%s' \"quoted value\"";
    for (name, argv) in [
        ("Run direct quoted script", vec!["sh", "-c", script]),
        ("Run quoted script", vec!["exec", "sh", "-c", script]),
    ] {
        let yaml = render(shell_step(name, &argv)?)?;
        let run = rendered_run(&yaml, name)?;
        let (_, output) =
            execute_rendered_run(&run, &[("VELNOR_TEST_CHECK_VARS", &credential_names)])?;
        if output != b"quoted value" {
            return Err(fail(&format!(
                "rendered run {run:?} produced stdout {:?}",
                String::from_utf8_lossy(&output)
            )));
        }
    }

    let pid_script = "printf '%s\\n' \"$$\"; /usr/bin/env";
    let yaml = render(shell_step(
        "Run plain exec",
        &["exec", "/usr/bin/env", "sh", "-c", pid_script],
    )?)?;
    let run = rendered_run(&yaml, "Run plain exec")?;
    let (pid, bytes) = execute_rendered_run(&run, &[])?;
    let output = String::from_utf8(bytes)?;
    let (reported_pid, output) = output
        .split_once('\n')
        .ok_or_else(|| fail("plain exec output lost process ID"))?;
    if reported_pid != pid.to_string() {
        return Err(fail(&format!(
            "plain exec changed shell PID:{pid}:{reported_pid}"
        )));
    }
    for key in velnor_actions_workflow_renderer::toolchain_env::CREDENTIAL_UNSET_VARS {
        if output
            .lines()
            .any(|line| line.starts_with(&format!("{key}=")))
        {
            return Err(fail(&format!("plain exec retained credential:{key}")));
        }
    }
    if !output.contains("VELNOR_TEST_CONTROL=control-value") {
        return Err(fail("plain exec lost control environment variable"));
    }

    let token_step = shell_step("Reject token argv", &["exec", "/usr/bin/env", "GH_TOKEN"])?;
    let error =
        render(token_step).expect_err("plain exec must reject a credential-bearing user argument");
    if !error.to_string().contains("token_in_run") {
        return Err(fail(&format!(
            "plain exec token argument returned unexpected error:{error}"
        )));
    }
    Ok(())
}

#[test]
fn gc1_t41_quotes_ordinary_argv_through_a_real_shell() -> Result<(), Box<dyn Error>> {
    let argv = vec![
        "printf".to_owned(),
        "%s\\0".to_owned(),
        "'leading".to_owned(),
        "trailing'".to_owned(),
        "two''quotes".to_owned(),
        "with spaces".to_owned(),
        "left'$VELNOR_TEST_VALUE'right".to_owned(),
        "literal$".to_owned(),
    ];
    let run = velnor_actions_workflow_renderer::join_argv_for_run(&argv)?;
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&run)
        .env("VELNOR_TEST_VALUE", "expanded value")
        .output()?;
    if !output.status.success() {
        return Err(fail(&format!(
            "quoted argv {run:?} failed with status {:?} and stderr {:?}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let expected = [
        "'leading",
        "trailing'",
        "two''quotes",
        "with spaces",
        "left'expanded value'right",
        "literal$",
    ]
    .join("\0");
    let mut expected = expected.into_bytes();
    expected.push(0);
    if output.stdout != expected {
        return Err(fail(&format!(
            "quoted argv {run:?} produced {:?}, expected {:?}",
            output.stdout, expected
        )));
    }
    Ok(())
}
