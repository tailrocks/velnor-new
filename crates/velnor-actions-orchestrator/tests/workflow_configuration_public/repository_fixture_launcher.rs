// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fmt::Write as _,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    nonrust_tools_fixture::Fixture,
    tools_cache_verify_hook::{Sandbox, expand_shell, pair},
};

const SENTINEL: &str = "fixture-origin-credential-sentinel";

#[test]
fn repository_fixture_launcher_restricts_credentials_to_direct_provisioner()
-> Result<(), Box<dyn Error>> {
    let (fixture, _replay) = Fixture::load()?;
    let step = super::repository_fixture::launcher()?;
    for repository in ["tailrocks/velnor-new", "example/foreign"] {
        let runner = Sandbox::new()?;
        let (bin, log) = probe(&fixture, &runner)?;
        let mut env = step
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        env.extend([
            pair("VELNOR_FIXTURE_ORIGIN_TOKEN", SENTINEL),
            pair("GITHUB_REPOSITORY", repository),
            pair(
                "GITHUB_WORKSPACE",
                &runner.workspace.path().display().to_string(),
            ),
            pair(
                "PATH",
                &format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", bin.display()),
            ),
            pair("VELNOR_PROBE_LOG", &log.display().to_string()),
            pair(
                "VELNOR_CHILD_PROBE",
                &runner.home.path().join("child.ts").display().to_string(),
            ),
            pair("GH_TOKEN", SENTINEL),
            pair("GITHUB_TOKEN", SENTINEL),
            pair(
                "BUN_OPTIONS",
                &preload_options(&runner.home.path().join("startup.ts")),
            ),
        ]);
        let output = runner.run_with_output(&expand_shell(&step.shell), &step.run, &env)?;
        assert_ne!(output.code, Some(0));
        assert!(!output.stdout.contains(SENTINEL) && !output.stderr.contains(SENTINEL));
        if repository == "example/foreign" {
            assert!(!log.exists());
            assert!(output.stderr.contains("fixture repository is not admitted"));
        } else {
            assert!(
                output
                    .stderr
                    .contains("probe reached credential-free installer"),
                "{}",
                output.stderr
            );
            inspect(&fs::read_to_string(&log)?)?;
            assert_no_recorded_credential(
                &runner
                    .runner_temp
                    .path()
                    .join("velnor-nonrust-tools-fixture"),
            )?;
        }
    }
    Ok(())
}

fn probe(fixture: &Fixture, runner: &Sandbox) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let bin = runner.home.path().join("probe-bin");
    fs::create_dir(&bin)?;
    let log = runner.home.path().join("probe.jsonl");
    let preload = runner.home.path().join("probe.ts");
    fs::write(&preload, PRELOAD)?;
    fs::write(runner.home.path().join("child.ts"), CHILD)?;
    fs::write(runner.home.path().join("startup.ts"), STARTUP)?;
    startup_control(fixture, runner)?;
    workspace_control(fixture, runner)?;
    let bun = bin.join("bun");
    executable(
        &bun,
        &format!(
            "#!/bin/sh\nPATH={mise}:/usr/bin:/bin:/usr/sbin:/sbin\nexport PATH\nexec {bun} --preload {preload} \"$@\"\n",
            mise = quote(fixture.mise.parent().ok_or("no mise directory")?),
            bun = quote(&fixture.bun),
            preload = quote(&preload)
        ),
    )?;
    executable(
        &bin.join("mise"),
        &format!(
            "#!/bin/sh\nif [ -n \"${{VELNOR_FIXTURE_ORIGIN_TOKEN-}}${{GH_TOKEN-}}${{GITHUB_TOKEN-}}\" ]; then printf '%s\\n' 'resolver received credentials' >&2; exit 12; fi\nprintf '%s\\n' '{{\"stage\":\"resolver\",\"hadCredential\":false}}' >> \"$VELNOR_PROBE_LOG\"\nprintf '%s\\n' {bun}\n",
            bun = quote(&bun)
        ),
    )?;
    Ok((bin, log))
}

fn inspect(log: &str) -> Result<(), Box<dyn Error>> {
    assert!(!log.contains(SENTINEL));
    let entries = log
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    assert!(
        entries
            .iter()
            .any(|entry| entry["stage"] == "resolver" && entry["hadCredential"] == false)
    );
    let mut digest = String::new();
    for byte in Sha256::digest(SENTINEL.as_bytes()) {
        write!(digest, "{byte:02x}")?;
    }
    assert!(
        !entries.iter().any(|entry| entry["stage"] == "startup"),
        "{log}"
    );
    assert!(entries.iter().any(
        |entry| entry["stage"] == "direct" && entry["digest"].as_str() == Some(digest.as_str())
    ));
    assert!(
        entries
            .iter()
            .filter(|entry| entry["stage"] == "direct")
            .all(|entry| entry["dotenv"].is_null()),
        "{log}"
    );
    let children = entries
        .iter()
        .filter(|entry| entry["stage"] == "child")
        .collect::<Vec<_>>();
    assert_eq!(children.len(), 2, "{log}");
    for child in children {
        assert_eq!(child["hadCredential"], false, "{log}");
    }
    let observed = entries
        .iter()
        .filter(|entry| entry["stage"] == "codec-observed")
        .collect::<Vec<_>>();
    assert_eq!(observed.len(), 1, "{log}");
    assert_eq!(observed[0]["hadCredential"], false, "{log}");
    assert!(observed[0]["dotenv"].is_null(), "{log}");
    Ok(())
}

fn preload_options(path: &Path) -> String {
    let path = path.display().to_string();
    assert!(
        !path.chars().any(char::is_whitespace),
        "Bun startup probe requires a path without whitespace"
    );
    format!("--preload {path}")
}

fn startup_control(fixture: &Fixture, runner: &Sandbox) -> Result<(), Box<dyn Error>> {
    let log = runner.home.path().join("startup-control.jsonl");
    let output = super::nonrust_process::output(
        Command::new(&fixture.bun)
            .args(["-e", "\"use strict\";"])
            .env_clear()
            .env(
                "BUN_OPTIONS",
                preload_options(&runner.home.path().join("startup.ts")),
            )
            .env("VELNOR_PROBE_LOG", &log)
            .env("VELNOR_FIXTURE_ORIGIN_TOKEN", SENTINEL),
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed: Value = serde_json::from_str(fs::read_to_string(log)?.trim())?;
    assert_eq!(observed["stage"], "startup");
    assert_eq!(observed["hadCredential"], true);
    Ok(())
}

fn executable(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, text)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn workspace_control(fixture: &Fixture, runner: &Sandbox) -> Result<(), Box<dyn Error>> {
    let root = super::repository_fixture::root()?;
    std::os::unix::fs::symlink(root.join("crates"), runner.workspace.path().join("crates"))?;
    fs::create_dir(runner.workspace.path().join("scripts"))?;
    for script in [
        "provision-nonrust-tools-fixture.ts",
        "nonrust-tools-transport.ts",
        "nonrust-bundle-snappy.ts",
        "nonrust-bundle-snappy.test.ts",
    ] {
        fs::copy(
            root.join("scripts").join(script),
            runner.workspace.path().join("scripts").join(script),
        )?;
    }
    fs::write(
        runner.workspace.path().join("bunfig.toml"),
        format!(
            "preload = [{0}]\n[test]\npreload = [{0}]\n",
            serde_json::to_string(&runner.home.path().join("startup.ts"))?
        ),
    )?;
    let global_preload = serde_json::to_string(&runner.home.path().join("startup.ts"))?;
    fs::write(
        runner.home.path().join(".bunfig.toml"),
        format!("preload = [{global_preload}]\n[test]\npreload = [{global_preload}]\n"),
    )?;
    fs::write(
        runner.workspace.path().join(".env"),
        "VELNOR_PROBE_DOTENV_VALUE=loaded\n",
    )?;
    fs::write(
        runner.workspace.path().join("control.ts"),
        "\"use strict\"; import {appendFileSync} from \"node:fs\"; const log=process.env.VELNOR_PROBE_LOG;if(!log)throw new Error(\"missing log\");appendFileSync(log,JSON.stringify({stage:\"file-control\",dotenv:process.env.VELNOR_PROBE_DOTENV_VALUE??null})+\"\\n\");\n",
    )?;
    let log = runner.home.path().join("file-control.jsonl");
    let output = super::nonrust_process::output(
        Command::new(&fixture.bun)
            .arg(runner.workspace.path().join("control.ts"))
            .env_clear()
            .env("HOME", runner.home.path())
            .env("VELNOR_PROBE_LOG", &log)
            .env("VELNOR_FIXTURE_ORIGIN_TOKEN", SENTINEL)
            .current_dir(runner.workspace.path()),
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries = fs::read_to_string(log)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<Value>, _>>()?;
    assert!(
        entries
            .iter()
            .any(|entry| entry["stage"] == "startup" && entry["hadCredential"] == true)
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["stage"] == "file-control" && entry["dotenv"] == "loaded")
    );
    Ok(())
}

fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn assert_no_recorded_credential(path: &Path) -> Result<(), Box<dyn Error>> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            assert_no_recorded_credential(&entry.path())?;
        } else {
            assert!(
                !fs::read(entry.path())?
                    .windows(SENTINEL.len())
                    .any(|bytes| bytes == SENTINEL.as_bytes())
            );
        }
    }
    Ok(())
}

const PRELOAD: &str = r#""use strict";
import { appendFileSync } from "node:fs";
const log = process.env.VELNOR_PROBE_LOG;
if (!log) throw new Error("probe log missing");
const credential = process.env.VELNOR_FIXTURE_ORIGIN_TOKEN;
appendFileSync(log, JSON.stringify({stage:"direct",digest:credential ? new Bun.CryptoHasher("sha256").update(credential).digest("hex") : null,dotenv:process.env.VELNOR_PROBE_DOTENV_VALUE??null})+"\n");
const spawn = Bun.spawn;
Object.defineProperty(Bun,"spawn",{value:(...args:unknown[]):unknown=>{
  const argv = Array.isArray(args[0]) ? args[0] : [];
  const options = args[1];
  const env:unknown = typeof options === "object" && options !== null && "env" in options ? options.env : process.env;
  const hadCredential = typeof env === "object" && env !== null && "VELNOR_FIXTURE_ORIGIN_TOKEN" in env && env.VELNOR_FIXTURE_ORIGIN_TOKEN !== undefined;
  appendFileSync(log,JSON.stringify({stage:"child",argv,hadCredential})+"\n");
  if (argv.includes("install")) throw new Error("probe reached credential-free installer");
  if (argv.includes("test")) {
    const child = process.env.VELNOR_CHILD_PROBE;
    if (!child || typeof argv[0] !== "string") throw new Error("missing codec probe");
    return Reflect.apply(spawn,Bun,[[argv[0],"test","--preload",child,...argv.slice(2)],...args.slice(1)]);
  }
  return Reflect.apply(spawn,Bun,args);
}});
"#;

const STARTUP: &str = r#""use strict";
import { appendFileSync } from "node:fs";
const log = process.env.VELNOR_PROBE_LOG;
if (!log) throw new Error("probe log missing");
appendFileSync(log,JSON.stringify({stage:"startup",hadCredential:process.env.VELNOR_FIXTURE_ORIGIN_TOKEN!==undefined})+"\n");
"#;

const CHILD: &str = r#""use strict";
import { appendFileSync } from "node:fs";
const log = process.env.VELNOR_PROBE_LOG;
if (!log) throw new Error("probe log missing");
appendFileSync(log,JSON.stringify({stage:"codec-observed",hadCredential:process.env.VELNOR_FIXTURE_ORIGIN_TOKEN!==undefined,dotenv:process.env.VELNOR_PROBE_DOTENV_VALUE??null})+"\n");
"#;
