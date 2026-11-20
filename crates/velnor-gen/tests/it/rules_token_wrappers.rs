// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, check_rules};

const TOKEN_ACTION: &str =
    "actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510";
const ISOLATION: &str = "--no-config --no-env --no-hooks";

fn token_job(command: &str) -> RenderedWorkflow {
    let step = |run: Option<&str>, uses: Option<&str>| RenderedStep {
        name: "step".to_owned(),
        run: run.map(str::to_owned),
        uses: uses.map(str::to_owned),
        ..RenderedStep::default()
    };
    RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "tag".to_owned(),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            steps: vec![step(Some(command), None), step(None, Some(TOKEN_ACTION))],
            ..RenderedJob::default()
        }],
    }
}

fn rules_of(command: &str) -> Vec<Rule> {
    check_rules(&token_job(command))
        .err()
        .into_iter()
        .flatten()
        .map(|violation| violation.rule)
        .collect()
}

/// Each wrapper that runs the rest of its command line, with `{}` where the payload goes.
const WRAPPERS: &[&str] = &[
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
    "timeout 5 {}",
    "timeout -s KILL 5 {}",
    "timeout --kill-after=1 5 {}",
    "timeout -k 1s --signal TERM -f 10m {}",
    "stdbuf -oL {}",
    "stdbuf -o L -e0 {}",
    "time {}",
    "time -p {}",
    "env {}",
    "env FOO=1 {}",
    "env -i FOO=1 {}",
    "env -C stage -u HOME {}",
    "/usr/bin/env {}",
    "FOO=1 {}",
    "xargs {}",
    "xargs -0 -r {}",
    "xargs -n 1 -P 4 {}",
    "sh -c \"{}\"",
    "bash -c '{}'",
    "bash --norc -ec \"{}\"",
    "nohup timeout 5 sh -c '{}'",
    "nohup nice -n 5 timeout 5 stdbuf -oL time {}",
    "FOO=1 nohup env A=1 nice {}",
    "sh -c \"nohup bash -c '{}'\"",
    "/usr/bin/nohup {}",
];

#[test]
fn a_wrapper_cannot_hide_mise_without_isolation() {
    for wrapper in WRAPPERS {
        for program in ["mise", "\"$RUNNER_TEMP/mise\"", "/usr/local/bin/mise"] {
            let command = wrapper.replace("{}", &format!("{program} exec rust -- cargo test"));
            assert_eq!(
                rules_of(&command),
                [Rule::MiseReadsRepositoryConfig],
                "{command}"
            );
        }
    }
}

#[test]
fn a_wrapper_around_isolated_mise_or_another_program_is_allowed() {
    for wrapper in WRAPPERS {
        for payload in [
            format!("mise {ISOLATION} exec gh@2.104.0 -- gh attestation verify dist/SHA256SUMS"),
            format!("\"$RUNNER_TEMP/mise\" {ISOLATION} install aqua:release-plz/release-plz@0.4.3"),
            "\"$RUNNER_TEMP/release-plz\" release --forge github".to_owned(),
            "gh release create v1".to_owned(),
            "echo mise".to_owned(),
        ] {
            if wrapper.contains("sh ") && payload.contains('$') {
                continue;
            }
            let command = wrapper.replace("{}", &payload);
            assert_eq!(rules_of(&command), Vec::<Rule>::new(), "{command}");
        }
    }
}

#[test]
fn an_expansion_inside_a_shell_script_is_refused_even_around_isolated_mise() {
    let command = format!("sh -c '\"$RUNNER_TEMP/mise\" {ISOLATION} install x'");
    assert_eq!(
        rules_of(&command),
        [Rule::MiseReadsRepositoryConfig],
        "{command}"
    );
}

#[test]
fn mise_in_a_command_sequence_is_found() {
    for command in [
        "true && mise install",
        "echo x; mise install",
        "false || mise install",
        "echo x | mise install",
        "(mise install)",
        "echo x & mise install",
        "sh -c \"true && mise install\"",
        "sh -c 'echo x; nohup mise install'",
    ] {
        assert!(
            rules_of(command).contains(&Rule::MiseReadsRepositoryConfig),
            "{command}"
        );
    }
}

#[test]
fn a_command_that_cannot_be_read_is_refused_in_a_token_job() {
    for command in [
        "eval \"$CMD\"",
        "eval 'echo hi'",
        "\"$CMD\" install",
        "nohup \"$CMD\" install",
        "\"$RUNNER_TEMP/$TOOL\" install",
        "sh -c \"$CMD\"",
        "bash -c 'echo $HOME'",
        "sh -c 'echo \"unclosed'",
        "sh -c",
        "sh -c 'nice $CMD'",
        "nice --unknown-option echo ok",
        "timeout --unknown 5 echo ok",
        "exec -x echo ok",
        "sudo echo ok",
        "xargs -I{} {} install",
        "find . -exec echo {} +",
        "source env.sh",
        ". ./env.sh",
        "sh -c 'if true; then echo ok; fi'",
    ] {
        let rules = rules_of(command);
        assert!(
            rules.contains(&Rule::UnreadableCommandInTokenJob)
                || rules.contains(&Rule::MiseReadsRepositoryConfig),
            "{command}: {rules:?}"
        );
    }
    for command in [
        "eval 'mise install'",
        "sudo mise install",
        "sh -c 'mise $X'",
    ] {
        assert!(
            rules_of(command).contains(&Rule::MiseReadsRepositoryConfig),
            "{command}"
        );
    }
}

#[test]
fn a_command_that_runs_nothing_is_allowed_in_a_token_job() {
    for command in [
        "command -v mise",
        "command -V mise",
        "echo mise install",
        "timeout 5 echo mise",
        "sh -c 'echo mise'",
        "exit 1",
        "mkdir -p dist",
    ] {
        assert_eq!(rules_of(command), Vec::<Rule>::new(), "{command}");
    }
}
