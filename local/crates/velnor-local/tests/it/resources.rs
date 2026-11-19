// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command as Process;
use std::time::Duration;

use clap::Parser;
use velnor_local::cli::{Cli, Command};
use velnor_local::io::authority::Backend;
use velnor_local::io::pair::{MintError, Minted};
use velnor_local::io::supervisor::{self, Pairs};
use velnor_local::pairs::{Limits, PlanInput, Sizes, parse_cpus, parse_memory, plan};
use velnor_local::side::Side;

use crate::support::temp_dir;

const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;

#[test]
fn memory_sizes_are_binary_integers_with_a_unit() {
    assert_eq!(parse_memory("6g"), Ok(6 * GIB));
    assert_eq!(parse_memory("6GiB"), Ok(6 * GIB));
    assert_eq!(parse_memory("512m"), Ok(512 * MIB));
    assert_eq!(parse_memory("1536MiB"), Ok(1536 * MIB));
}

#[test]
fn bad_memory_sizes_are_errors_that_name_the_flag() {
    for bad in [
        "0",
        "0g",
        "-1g",
        "abc",
        "6x",
        "",
        "g",
        "1.5g",
        "256m",
        "99999999999999999999g",
    ] {
        let error = parse_memory(bad).expect_err(bad);
        assert!(error.contains("--runner-memory"), "{bad}: {error}");
    }
}

#[test]
fn cpus_are_decimals_that_become_millicores() {
    assert_eq!(parse_cpus("4"), Ok(4000));
    assert_eq!(parse_cpus("1.5"), Ok(1500));
    assert_eq!(parse_cpus("0.25"), Ok(250));
    assert_eq!(parse_cpus("0.001"), Ok(1));
}

#[test]
fn bad_cpu_counts_are_errors_that_name_the_flag() {
    for bad in [
        "0",
        "0.0",
        "-1",
        "abc",
        "",
        ".",
        "1.5555",
        "99999999999999999999",
        "1e3",
    ] {
        let error = parse_cpus(bad).expect_err(bad);
        assert!(error.contains("--runner-cpus"), "{bad}: {error}");
    }
}

fn input(sides: &[(Side, String, u16)], limits: Limits) -> PlanInput<'_> {
    PlanInput {
        environment: "velnor-local-ab",
        authority: "127.0.0.1:5311",
        token: "t0k3n",
        docker: "unix:///var/run/docker.sock",
        runner_image: "sha256:aa",
        dind_image: "sha256:bb",
        sides,
        policy: None,
        limits,
        storage: Sizes::default(),
    }
}

#[test]
fn the_default_limits_are_the_old_constants_and_the_plan_carries_given_ones() {
    let sides = [(Side::Trusted, "n".to_owned(), 1)];
    let planned = plan(&input(&sides, Limits::default()));
    assert_eq!(planned["limits"]["runner_cpu_millicores"], 4000);
    assert_eq!(planned["limits"]["runner_memory_bytes"], 6 * GIB);
    assert_eq!(planned["limits"]["dind_cpu_millicores"], 2000);
    assert_eq!(planned["limits"]["dind_memory_bytes"], 2 * GIB);
    let small = Limits {
        runner_cpu_millicores: 1500,
        runner_memory_bytes: GIB,
    };
    let planned = plan(&input(&sides, small));
    assert_eq!(planned["limits"]["runner_cpu_millicores"], 1500);
    assert_eq!(planned["limits"]["runner_memory_bytes"], GIB);
    assert_eq!(planned["limits"]["dind_memory_bytes"], 2 * GIB);
    assert_eq!(small.pair_memory_bytes(), 3 * GIB);
}

fn up(extra: &[&str]) -> Result<Limits, clap::Error> {
    let mut args = vec!["velnor-local", "up", "--dir", "/d", "--repo", "/r"];
    args.extend_from_slice(extra);
    Cli::try_parse_from(args).map(|cli| match cli.command {
        Command::Up(args) => args.environment.limits(),
        other => unreachable!("{other:?}"),
    })
}

#[test]
fn up_takes_runner_memory_and_cpus_and_defaults_to_the_old_limits() {
    assert_eq!(up(&[]).ok(), Some(Limits::default()));
    let given = up(&["--runner-memory", "3g", "--runner-cpus", "1.5"]);
    assert_eq!(
        given.ok(),
        Some(Limits {
            runner_cpu_millicores: 1500,
            runner_memory_bytes: 3 * GIB,
        })
    );
    let error = up(&["--runner-memory", "0"]).expect_err("zero memory");
    assert!(error.to_string().contains("--runner-memory"), "{error}");
    assert!(up(&["--runner-cpus", "0"]).is_err());
}

struct Online;

impl Backend for Online {
    fn get(&self, _: Side, path: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        let body = if path.contains("isagentonline") {
            r#"{"online":true}"#
        } else {
            r#"{"value":[{"name":"trusted-1"}],"count":1}"#
        };
        std::future::ready(Ok(Some(body.as_bytes().to_vec())))
    }

    fn delete(&self, _: Side, _: &str) -> impl Future<Output = io::Result<()>> + Send {
        std::future::ready(Ok(()))
    }

    fn mint(
        &self,
        _: Side,
        _: Option<&str>,
    ) -> impl Future<Output = Result<Option<Minted>, MintError>> + Send {
        std::future::ready(Ok(None))
    }
}

#[tokio::test]
async fn the_limits_reach_the_authority_process_that_plans_the_pairs() {
    let dir = temp_dir("resources-supervise");
    let script = dir.join("fake-authority");
    std::fs::write(
        &script,
        "#!/bin/bash\nmkdir -p \"$3/runner\"\necho \"$@\" > \"$3/args\"\necho 127.0.0.1:9 > \"$3/authority/address\"\necho 'pair trusted-1 listening' > \"$3/runner/velnor-host.log\"\nexec /bin/sleep 600\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let pairs = Pairs {
        host: script.clone(),
        trusted: 1,
        untrusted: 0,
        policy: None,
        limits: Limits {
            runner_cpu_millicores: 1500,
            runner_memory_bytes: GIB,
        },
        storage: Sizes::default(),
        ..Pairs::default()
    };
    let started = supervisor::start_pairs(
        &dir,
        &script,
        &pairs,
        async |_: &Path| Ok(Online),
        Duration::from_secs(10),
    )
    .await;
    let args = std::fs::read_to_string(dir.join("args")).unwrap_or_default();
    let pid = std::fs::read_to_string(dir.join("authority/pid")).unwrap_or_default();
    let _ = Process::new("kill").args(["-KILL", pid.trim()]).status();
    assert!(started.is_ok(), "{started:?}");
    assert!(args.contains("--runner-memory-bytes 1073741824"), "{args}");
    assert!(args.contains("--runner-cpu-millicores 1500"), "{args}");
    let parsed = Cli::try_parse_from(["velnor-local"].into_iter().chain(args.split_whitespace()));
    assert!(matches!(
        parsed.map(|cli| cli.command),
        Ok(Command::Supervise(_))
    ));
}
