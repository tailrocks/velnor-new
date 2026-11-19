// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, refusal, rendered, workflow};

fn job_running(job_keys: &str, command: &str, step_keys: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n{job_keys}\n[[workflow.job.step]]\nname = \"Run\"\nrun = '''{command}'''\n{step_keys}\n"
        ),
    )
}

const WRITE: &str = "permissions = { contents = \"write\" }";
const TOKEN: &str = "env = { GH_TOKEN = \"${{ github.token }}\" }";

/// Each wrapper that runs the rest of its command line, with `{}` where the payload goes.
const WRAPPERS: [&str; 40] = [
    "nohup {}",
    "exec {}",
    "command {}",
    "command -p {}",
    "builtin {}",
    "nice {}",
    "nice -n 5 {}",
    "nice -n5 {}",
    "nice -5 {}",
    "nice --adjustment=5 {}",
    "ionice -c 3 {}",
    "ionice -c3 -n 2 {}",
    "ionice -t {}",
    "timeout 5 {}",
    "timeout -s KILL 5 {}",
    "timeout -sKILL 5 {}",
    "timeout --kill-after=1 5 {}",
    "timeout -k 1s --signal TERM -f 10m {}",
    "stdbuf -oL {}",
    "stdbuf -o L -e0 {}",
    "stdbuf --output=L {}",
    "time {}",
    "time -p {}",
    "xargs {}",
    "xargs -0 -r {}",
    "xargs -n 1 -P 4 {}",
    "xargs --max-args=1 -- {}",
    "sudo {}",
    "sudo -u root {}",
    "sh -c \"{}\"",
    "bash -c \"{}\"",
    "zsh -c \"{}\"",
    "sh -ec \"{}\"",
    "bash --norc -c \"{}\"",
    "env -C stage nohup timeout 5 {}",
    "nohup nice -n 5 timeout 5 stdbuf -oL time {}",
    "FOO=1 nohup env A=1 nice {}",
    "sh -c \"nohup {}\"",
    "/usr/bin/nohup {}",
    "eval \"{}\"",
];

#[test]
fn wrappers_cannot_hide_a_helper_call() -> TestResult {
    for wrapper in WRAPPERS {
        let command = wrapper.replace("{}", "velnor-ci check");
        let message = refusal(&job_running("", &command, ""))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(message.contains("helper = true"), "{command:?}: {message}");
        let command = wrapper.replace("{}", "velnor-ci plan");
        let message = refusal(&job_running("helper = true", &command, ""))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(
            message.contains("must start with velnor-ci"),
            "{command:?}: {message}"
        );
    }
    Ok(())
}

#[test]
fn a_command_sequence_is_refused_when_it_calls_the_helper() -> TestResult {
    for command in [
        "true && velnor-ci check",
        "true; velnor-ci check",
        "false || velnor-ci check",
        "echo x | velnor-ci check",
        "(velnor-ci check)",
        "sh -c \"true && velnor-ci check\"",
    ] {
        refusal(&job_running("", command, "")).map_err(|error| format!("{command:?}: {error}"))?;
    }
    Ok(())
}

#[test]
fn wrappers_cannot_hide_a_repository_script_from_the_write_token_rule() -> TestResult {
    for wrapper in WRAPPERS {
        for payload in ["./publish.sh", "bash publish.sh"] {
            let command = wrapper.replace("{}", payload);
            let message = refusal(&job_running(WRITE, &command, TOKEN))
                .map_err(|error| format!("{command:?}: {error}"))?;
            assert!(message.contains("github.token"), "{command:?}: {message}");
        }
    }
    Ok(())
}

#[test]
fn a_command_sequence_is_refused_when_it_runs_a_repository_script() -> TestResult {
    for command in [
        "true && ./publish.sh",
        "echo hi; bash publish.sh",
        "echo hi | ./publish.sh",
        "false || python3 publish.py",
        "echo hi & ./publish.sh",
        "(./publish.sh)",
        "sh -c \"true && ./publish.sh\"",
        "sh -c \"gh release create v1; ./publish.sh\"",
    ] {
        refusal(&job_running(WRITE, command, TOKEN))
            .map_err(|error| format!("{command:?}: {error}"))?;
    }
    Ok(())
}

#[test]
fn a_shell_script_that_is_not_a_plain_literal_is_a_repository_script() -> TestResult {
    for command in [
        "sh -c \"$SCRIPT\"",
        "bash -c 'echo $HOME'",
        "bash -c \"echo `date`\"",
        "sh -c 'echo \"unclosed'",
        "sh -c",
        "sh -c 'if true; then gh release create v1; fi'",
        "sh -c 'nice $CMD'",
        "sh -c \"sh -c 'echo $HOME'\"",
        "eval 'gh release create v1'",
        "eval \"$CMD\"",
        "builtin eval 'gh release create v1'",
        "source env.sh",
        ". ./env.sh",
        "\"$SCRIPT\" release",
        "nohup \"$SCRIPT\" release",
        "xargs -I{} {} release",
        "xargs -i {} release",
        "nice --unknown-option gh release create v1",
        "timeout --unknown 5 gh release create v1",
        "sudo gh release create v1",
        "find . -exec gh release create v1 {} +",
        "exec -x gh release create v1",
    ] {
        let message = refusal(&job_running(WRITE, command, TOKEN))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(message.contains("github.token"), "{command:?}: {message}");
    }
    Ok(())
}

#[test]
fn a_plain_shell_script_is_parsed_like_a_run_value() -> TestResult {
    for command in [
        "sh -c 'gh release create v1'",
        "bash -c \"gh release create v1\"",
        "zsh -c 'gh release create v1'",
        "bash -ec 'gh release create v1'",
        "sh -c 'nohup nice -n 5 gh release create v1'",
        "sh -c \"echo ok\"",
        "sh -c 'echo velnor-ci check'",
        "sh -c 'gh release create v1 >out.txt'",
        "env -C dist sh -c 'gh release create v1'",
        "sh -c 'gh release create v1' name",
    ] {
        rendered(&job_running(WRITE, command, TOKEN))
            .map_err(|error| format!("{command:?}: {error}"))?;
    }
    Ok(())
}

#[test]
fn benign_wrapper_uses_are_not_refused() -> TestResult {
    for command in [
        "timeout 5 echo ok",
        "timeout -s KILL 5 gh release create v1",
        "command -v jq",
        "command -V jq",
        "command -pv jq",
        "command -v velnor-ci",
        "nohup gh release create v1",
        "exec gh release create v1",
        "nice gh release create v1",
        "nice -n 5 gh release create v1",
        "ionice -c 3 gh release create v1",
        "stdbuf -oL gh release create v1",
        "time gh release create v1",
        "time -p echo ok",
        "xargs echo velnor-ci",
        "xargs -n 1 -I{} echo {}",
        "xargs -0 -r gh release create v1",
        "nohup nice -n 5 timeout 5 stdbuf -oL gh release create v1",
        "env -C dist timeout 5 gh release create v1",
        "echo velnor-ci check",
        "timeout 5 echo velnor-ci check",
    ] {
        rendered(&job_running(WRITE, command, TOKEN))
            .map_err(|error| format!("{command:?}: {error}"))?;
        rendered(&job_running("", command, "")).map_err(|error| format!("{command:?}: {error}"))?;
    }
    Ok(())
}

#[test]
fn a_repository_binary_named_velnor_ci_is_not_the_helper_behind_a_wrapper() -> TestResult {
    for command in [
        "nohup time ./velnor-ci --version",
        "timeout 5 stage/velnor-ci check",
    ] {
        rendered(&job_running("", command, "")).map_err(|error| format!("{command:?}: {error}"))?;
        let message = refusal(&job_running(WRITE, command, TOKEN))
            .map_err(|error| format!("{command:?}: {error}"))?;
        assert!(message.contains("github.token"), "{command:?}: {message}");
    }
    Ok(())
}
