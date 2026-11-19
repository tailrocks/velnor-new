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
use velnor_local::pairs::{
    Limits, PlanInput, Sizes, parse_home_size, parse_tmp_size, parse_work_size, plan,
};
use velnor_local::side::Side;

use crate::support::temp_dir;

const GIB: u64 = 1 << 30;

type SizeParser = fn(&str) -> Result<u64, String>;

fn up(extra: &[&str]) -> Result<Sizes, clap::Error> {
    let mut args = vec!["velnor-local", "up", "--dir", "/d", "--repo", "/r"];
    args.extend_from_slice(extra);
    Cli::try_parse_from(args).map(|cli| match cli.command {
        Command::Up(args) => args.environment.storage.storage(),
        other => unreachable!("{other:?}"),
    })
}

fn planned(storage: Sizes) -> serde_json::Value {
    let sides = [(Side::Trusted, "n".to_owned(), 1)];
    plan(&PlanInput {
        environment: "velnor-local-ab",
        authority: "127.0.0.1:5311",
        token: "t0k3n",
        docker: "unix:///var/run/docker.sock",
        runner_image: "sha256:aa",
        dind_image: "sha256:bb",
        sides: &sides,
        policy: None,
        limits: Limits::default(),
        storage,
        local_ci: None,
    })
}

#[test]
fn size_flags_parse_and_name_their_flag() {
    assert_eq!(parse_home_size("3g"), Ok(3 * GIB));
    assert_eq!(parse_work_size("64GiB"), Ok(64 * GIB));
    assert_eq!(parse_tmp_size("1024m"), Ok(GIB));
    let parsers: [(&str, SizeParser); 3] = [
        ("--home-size", parse_home_size),
        ("--work-size", parse_work_size),
        ("--tmp-size", parse_tmp_size),
    ];
    for (flag, parse) in parsers {
        for bad in ["0", "6x", "1.5g", "", "g", "-1g", "99999999999999999999g"] {
            let error = parse(bad).expect_err(bad);
            assert!(error.starts_with(flag), "{bad}: {error}");
        }
        let small = parse("512m").expect_err("below the least");
        assert!(small.contains("the least is 1g"), "{small}");
        let large = parse("100g").expect_err("above the most");
        assert!(large.contains("at most 64g"), "{large}");
    }
}

#[test]
fn absent_size_flags_keep_today_values_and_given_ones_reach_the_plan() {
    let sizes = up(&[]).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(sizes, Sizes::default());
    let storage = &planned(sizes)["storage"];
    assert_eq!(storage["home_bytes"], 2 * GIB);
    assert_eq!(storage["work_bytes"], 16 * GIB);
    assert_eq!(storage["tmp_bytes"], 2 * GIB);
    let given = up(&[
        "--home-size",
        "6g",
        "--work-size",
        "8GiB",
        "--tmp-size",
        "3g",
    ]);
    let storage = &planned(given.unwrap_or_else(|e| unreachable!("{e}")))["storage"];
    assert_eq!(storage["home_bytes"], 6 * GIB);
    assert_eq!(storage["work_bytes"], 8 * GIB);
    assert_eq!(storage["tmp_bytes"], 3 * GIB);
    let error = up(&["--tmp-size", "100g"]).expect_err("above the most");
    assert!(error.to_string().contains("--tmp-size"), "{error}");
}

#[test]
fn plain_mode_refuses_a_size_flag_and_names_it() {
    let dir = temp_dir("storage-plain");
    let output = Process::new(env!("CARGO_BIN_EXE_velnor-local"))
        .args(["up", "--runner", "plain", "--home-size", "3g", "--dir"])
        .arg(dir.join("state"))
        .arg("--repo")
        .arg(&dir)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(text.contains("--home-size needs --runner pair"), "{text}");
}

#[test]
fn supervise_bytes_round_trip() {
    let parsed = Cli::try_parse_from([
        "velnor-local",
        "supervise",
        "--dir",
        "/d",
        "--home-bytes",
        "6442450944",
    ]);
    let Ok(Command::Supervise(args)) = parsed.map(|cli| cli.command) else {
        unreachable!("supervise did not parse");
    };
    assert_eq!(args.sizes.storage().home_bytes, 6 * GIB);
    assert_eq!(args.sizes.storage().work_bytes, 16 * GIB);
    assert_eq!(args.sizes.storage().tmp_bytes, 2 * GIB);
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
async fn the_sizes_reach_the_authority_process_that_plans_the_pairs() {
    let dir = temp_dir("storage-supervise");
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
        storage: Sizes {
            home_bytes: 6 * GIB,
            work_bytes: 8 * GIB,
            tmp_bytes: 3 * GIB,
        },
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
    Process::new("kill")
        .args(["-KILL", pid.trim()])
        .status()
        .ok();
    assert!(started.is_ok(), "{started:?}");
    assert!(args.contains("--home-bytes 6442450944"), "{args}");
    assert!(args.contains("--work-bytes 8589934592"), "{args}");
    assert!(args.contains("--tmp-bytes 3221225472"), "{args}");
}
