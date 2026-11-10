// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ProductReleasePins, features};
use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{JobTimeout, ReleaseTarget};

const GATE: &str = "inputs.mode == 'stable-sources' && github.event_name == 'workflow_dispatch' && github.repository == 'tailrocks/velnor-new' && startsWith(github.ref, 'refs/heads/') && github.ref != format('refs/heads/{0}', github.event.repository.default_branch)";
const PREPARE: &str = "bash scripts/generator-release/stable-source-consumer.sh";
const GUARD: &str = "set -eu\nCARGO_NET_OFFLINE=true bash scripts/build-owned-archive-guard.sh --source-cache \"$CARGO_HOME\"";

#[derive(Clone, Copy)]
struct Platform {
    id: &'static str,
    producer_runner: &'static str,
    consumer_runner: &'static str,
    producer_target: ReleaseTarget,
    consumer_target: ReleaseTarget,
}

const PLATFORMS: [Platform; 3] = [
    Platform {
        id: "linux",
        producer_runner: "ubuntu-26.04",
        consumer_runner: "ubuntu-26.04",
        producer_target: ReleaseTarget::LinuxX86_64,
        consumer_target: ReleaseTarget::LinuxX86_64,
    },
    Platform {
        id: "macos",
        producer_runner: "macos-15",
        consumer_runner: "macos-15",
        producer_target: ReleaseTarget::MacosArm64,
        consumer_target: ReleaseTarget::MacosArm64,
    },
    Platform {
        id: "intel",
        producer_runner: "macos-15",
        consumer_runner: "macos-15-intel",
        producer_target: ReleaseTarget::MacosArm64,
        consumer_target: ReleaseTarget::MacosX86_64,
    },
];

pub(super) fn jobs(pins: &ProductReleasePins) -> Result<Vec<(String, Yaml)>, RenderError> {
    let mut result = Vec::new();
    for platform in PLATFORMS {
        result.push(producer(platform, pins)?);
        result.push(consumer(platform, pins, true)?);
        result.push(consumer(platform, pins, false)?);
    }
    Ok(result)
}

fn producer(platform: Platform, pins: &ProductReleasePins) -> Result<(String, Yaml), RenderError> {
    let id = format!("stable-sources-producer-{}", platform.id);
    let mut fields = fields(
        &format!("Stable sources / {} / producer", platform.id),
        platform.producer_runner,
    );
    fields.push((
        "outputs".into(),
        map(&[
            ("artifact_id", "${{ steps.upload.outputs.artifact-id }}"),
            (
                "artifact_digest",
                "${{ steps.upload.outputs.artifact-digest }}",
            ),
            ("tree", "${{ steps.identity.outputs.tree }}"),
            ("root_lock", "${{ steps.identity.outputs.root_lock }}"),
            ("runner_lock", "${{ steps.identity.outputs.runner_lock }}"),
            ("attempt", "${{ steps.identity.outputs.attempt }}"),
        ]),
    ));
    let mut steps = setup(platform.producer_target, pins)?;
    steps.push(shell(
        "Prepare cold producer sources",
        "bash scripts/generator-release/stable-source-producer.sh",
        &[("VELNOR_RUST_VERSION", &pins.rust_version)],
    ));
    let identity = r#"set -eu
test "$(/usr/bin/python3 -I -S scripts/generator-release/cargo-source-tree.py "$RUNNER_TEMP/stable-cargo-sources")" = "$STABLE_SOURCES_TREE"
test "$(shasum -a 256 Cargo.lock | cut -d ' ' -f 1)" = "$STABLE_SOURCES_ROOT_LOCK"
test "$(shasum -a 256 crates/velnor-runner/Cargo.lock | cut -d ' ' -f 1)" = "$STABLE_SOURCES_RUNNER_LOCK"
printf 'tree=%s\nroot_lock=%s\nrunner_lock=%s\nattempt=%s\n' "$STABLE_SOURCES_TREE" "$STABLE_SOURCES_ROOT_LOCK" "$STABLE_SOURCES_RUNNER_LOCK" "$STABLE_SOURCES_ATTEMPT" >> "$GITHUB_OUTPUT""#;
    steps.push(with_id(
        shell("Record producer identity", identity, &[]),
        "identity",
    ));
    steps.push(Yaml::Map(vec![
        ("name".into(), Yaml::str("Upload exact source artifact")),
        ("id".into(), Yaml::str("upload")),
        ("uses".into(), Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES)),
        (
            "with".into(),
            map(&[
                (
                    "name",
                    &format!(
                        "proof-{}-sources-${{{{ github.run_attempt }}}}",
                        platform.id
                    ),
                ),
                ("path", "${{ runner.temp }}/stable-cargo-sources/"),
                ("include-hidden-files", "true"),
                ("if-no-files-found", "error"),
                ("retention-days", "1"),
            ]),
        ),
    ]));
    Ok(features::finish(&id, fields, steps))
}

fn consumer(
    platform: Platform,
    pins: &ProductReleasePins,
    warm: bool,
) -> Result<(String, Yaml), RenderError> {
    let state = if warm { "warm" } else { "cold" };
    let producer = format!("stable-sources-producer-{}", platform.id);
    let mut fields = fields(
        &format!("Stable sources / {} / {state}", platform.id),
        platform.consumer_runner,
    );
    if warm {
        fields.push(("needs".into(), Yaml::Seq(vec![Yaml::str(&producer)])));
    }
    let mut steps = setup(platform.consumer_target, pins)?;
    let tree = format!("${{{{ needs.{producer}.outputs.tree }}}}");
    if warm {
        steps.extend(warm_steps(platform, &producer));
    }
    steps.push(shell(
        "Measure source preparation",
        PREPARE,
        &[
            ("VELNOR_RUST_VERSION", &pins.rust_version),
            (
                "SOURCES_DOWNLOADED",
                if warm {
                    "${{ steps.download.outcome }}"
                } else {
                    "skipped"
                },
            ),
            ("SOURCES_TREE", if warm { &tree } else { "" }),
        ],
    ));
    let expected = if warm { "1" } else { "0" };
    steps.push(shell("Require source provenance and fetch mode", &format!("set -eu\nevidence=\"$RUNNER_TEMP/source-qualification-artifacts/cargo-fetch-lock-identity.txt\"\ngrep -Fx 'source_artifact_admitted={expected}' \"$evidence\"\ngrep -Fx 'fetch_mode=Cargo.toml:{}' \"$evidence\"\ngrep -Fx 'fetch_mode=crates/velnor-runner/Cargo.toml:{}' \"$evidence\"", if warm { "offline" } else { "network" }, if warm { "offline" } else { "network" }), &[]));
    steps.push(shell("Measure offline archive guard build", GUARD, &[]));
    Ok(features::finish(
        &format!("stable-sources-{}-{state}", platform.id),
        fields,
        steps,
    ))
}

fn warm_steps(platform: Platform, producer: &str) -> Vec<Yaml> {
    let mut steps = Vec::new();
    let values = [
        "artifact_id",
        "artifact_digest",
        "tree",
        "root_lock",
        "runner_lock",
        "attempt",
    ]
    .map(|name| format!("${{{{ needs.{producer}.outputs.{name} }}}}"));
    let admission = shell(
        "Admit same-run source artifact",
        "/usr/bin/python3 -I -S scripts/generator-release/stable-source-admission.py",
        &[
            ("GH_TOKEN", "${{ github.token }}"),
            ("SOURCES_ID", &values[0]),
            ("SOURCES_DIGEST", &values[1]),
            ("SOURCES_TREE", &values[2]),
            ("SOURCES_ROOT_LOCK", &values[3]),
            ("SOURCES_RUNNER_LOCK", &values[4]),
            ("SOURCES_ATTEMPT", &values[5]),
            ("SOURCES_NAME", &format!("proof-{}", platform.id)),
        ],
    );
    steps.push(with_id(admission, "admission"));
    steps.push(shell(
        "Require source artifact admission",
        "test \"$SOURCES_ADMITTED\" = true",
        &[(
            "SOURCES_ADMITTED",
            "${{ steps.admission.outputs.admitted }}",
        )],
    ));
    steps.push(Yaml::Map(vec![
        ("name".into(), Yaml::str("Download exact admitted sources")),
        ("id".into(), Yaml::str("download")),
        (
            "uses".into(),
            Yaml::str(crate::steps::DOWNLOAD_ARTIFACT_USES),
        ),
        (
            "with".into(),
            map(&[
                ("artifact-ids", &values[0]),
                ("path", "${{ runner.temp }}/stable-cargo-download"),
            ]),
        ),
    ]));

    steps
}

fn fields(name: &str, runner: &str) -> Vec<(String, Yaml)> {
    let mut result = features::base(name, Yaml::str(runner), JobTimeout::PRODUCT_MACOS_COMPILER);
    result.push(("if".into(), Yaml::str(GATE)));
    result.push((
        "permissions".into(),
        map(&[("actions", "read"), ("contents", "read")]),
    ));
    result
}

fn setup(target: ReleaseTarget, pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let setup = pins.setup_for(target);
    setup.validate()?;
    Ok(vec![
        features::checkout_step(),
        shell(
            "Verify branch source",
            "set -eu\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"",
            &[],
        ),
        Yaml::Map(vec![
            ("name".into(), Yaml::str("Set up Mise")),
            ("uses".into(), Yaml::str(&setup.uses)),
            (
                "with".into(),
                map(&[
                    ("version", &setup.version),
                    ("sha256", &setup.sha256),
                    ("install", "false"),
                    ("env", "false"),
                    ("cache", "false"),
                    ("cache_save", "false"),
                ]),
            ),
        ]),
        shell(
            "Install pinned qualification tools",
            &crate::commands::join_argv_for_run(&pins.install_qualify_tools_argv)?,
            &[("GITHUB_TOKEN", "${{ github.token }}")],
        ),
        crate::verifier_shell::render_command(
            crate::VERIFY_RUST_TOOLCHAIN_STEP,
            &pins.verify_rust_argv,
        )?,
    ])
}

fn map(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), Yaml::str(*value)))
            .collect(),
    )
}

fn shell(name: &str, run: &str, env: &[(&str, &str)]) -> Yaml {
    let mut fields = vec![
        ("name".into(), Yaml::str(name)),
        ("shell".into(), Yaml::str("bash")),
        ("run".into(), Yaml::str(run)),
    ];
    if !env.is_empty() {
        fields.push(("env".into(), map(env)));
    }
    Yaml::Map(fields)
}

fn with_id(step: Yaml, id: &str) -> Yaml {
    let Yaml::Map(mut fields) = step else {
        return step;
    };
    fields.push(("id".into(), Yaml::str(id)));
    Yaml::Map(fields)
}
