// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::run::scratch;

struct Fixture {
    temp: velnor_test_support::ScratchDir,
    work: PathBuf,
    bin: PathBuf,
}

impl Fixture {
    fn new(name: &str, roots: &[&str]) -> Self {
        let temp = scratch(name);
        let work = temp.join("work");
        let bin = temp.join("bin");
        fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
        for root in roots {
            fs::create_dir_all(work.join(root)).unwrap_or_else(|e| unreachable!("{e}"));
        }
        let cargo = bin.join("cargo");
        fs::write(
            &cargo,
            "#!/bin/sh\nprintf '%s|%s\\n' \"$(basename \"$PWD\")\" \"$*\" >> \"$FAKE_CARGO_CALLS\"\n\
             echo \"fetching $(basename \"$PWD\")\"\n\
             if [ -f fail ]; then echo 'error: cannot fetch' >&2; exit 42; fi\n\
             if [ -f database ]; then d=\"$HOME/.cargo/git/db/dep-1\"; \
             mkdir -p \"$d/hooks\" \"$d/objects/info\" \"$d/objects/pack\"; \
             echo ref: refs/heads/main > \"$d/HEAD\"; cp database \"$d/config\"; \
             echo hook > \"$d/hooks/post-fetch\"; echo head > \"$d/FETCH_HEAD\"; fi\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        Self { temp, work, bin }
    }

    fn fail_in(&self, root: &str) {
        fs::write(self.work.join(root).join("fail"), "").unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn fetched_database(&self, root: &str, config: &str) {
        fs::write(self.work.join(root).join("database"), config)
            .unwrap_or_else(|e| unreachable!("{e}"));
    }

    fn database(&self) -> PathBuf {
        self.temp.join("home/.cargo/git/db/dep-1")
    }

    fn fetch(&self, roots: &[&str]) -> Output {
        let path = format!(
            "{}:{}",
            self.bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["sources", "fetch", "--roots"])
            .args(roots)
            .current_dir(&self.work)
            .env("PATH", path)
            .env("RUNNER_TEMP", self.temp.path())
            .env("HOME", self.temp.join("home"))
            .env("FAKE_CARGO_CALLS", self.calls_path())
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    }

    fn calls_path(&self) -> PathBuf {
        self.temp.join("calls")
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.calls_path())
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.temp.join("velnor-run.log")).unwrap_or_default()
    }

    fn invocations(&self) -> Vec<serde_json::Value> {
        fs::read_to_string(self.temp.join("velnor-invocations.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap_or_else(|e| unreachable!("{e}")))
            .collect()
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn every_root_is_fetched_locked_in_order_and_recorded_like_run() {
    let fixture = Fixture::new("sources-fetch-all", &["zeta", "alpha", "nested/mid"]);
    let output = fixture.fetch(&["zeta", "alpha", "nested/mid"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fixture.calls(),
        [
            "zeta|fetch --locked",
            "alpha|fetch --locked",
            "mid|fetch --locked"
        ]
    );
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("fetching zeta\nfetching alpha\nfetching mid\n"),
        "{stdout}"
    );
    let log = fixture.log();
    assert_eq!(
        log.matches("velnor-ci run: cargo (2 arguments)").count(),
        3,
        "{log}"
    );
    assert_eq!(log.matches("velnor-ci run: exit 0\n").count(), 3, "{log}");
    let invocations = fixture.invocations();
    let argv: Vec<_> = invocations
        .iter()
        .map(|record| record["argv"].to_string())
        .collect();
    assert_eq!(argv, [r#"["cargo","fetch","--locked"]"#; 3]);
    let roots: Vec<_> = invocations
        .iter()
        .map(|record| {
            record["root"]
                .as_str()
                .map(|root| root.rsplit('/').next().unwrap_or("").to_owned())
        })
        .collect();
    assert_eq!(
        roots,
        [
            Some("zeta".to_owned()),
            Some("alpha".to_owned()),
            Some("mid".to_owned())
        ]
    );
}

#[test]
fn the_first_failure_stops_the_run_names_the_root_and_exits_non_zero() {
    let fixture = Fixture::new("sources-fetch-fail", &["one", "two", "three"]);
    fixture.fail_in("two");
    let output = fixture.fetch(&["one", "two", "three"]);
    assert_eq!(output.status.code(), Some(42), "{output:?}");
    assert_eq!(
        fixture.calls(),
        ["one|fetch --locked", "two|fetch --locked"]
    );
    let stderr = text(&output.stderr);
    assert!(stderr.contains("error: cannot fetch"), "{stderr}");
    assert!(stderr.contains("sources fetch failed in two"), "{stderr}");
    assert!(!stderr.contains("three"), "{stderr}");
    assert!(fixture.log().ends_with("velnor-ci run: exit 42\n"));
}

#[test]
fn an_empty_root_list_is_refused_before_any_fetch() {
    let fixture = Fixture::new("sources-fetch-empty", &["one"]);
    let output = fixture.fetch(&[]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(text(&output.stderr).contains("--roots"), "{output:?}");
    assert_eq!(fixture.calls(), Vec::<String>::new());
}

#[test]
fn duplicate_roots_are_refused_before_any_fetch_even_when_spelled_differently() {
    let fixture = Fixture::new("sources-fetch-duplicate", &["one", "two"]);
    for roots in [
        &["one", "two", "one"][..],
        &["one", "./one/"],
        &["one", "two/../one"],
    ] {
        let output = fixture.fetch(roots);
        assert_eq!(output.status.code(), Some(1), "{roots:?} {output:?}");
        let stderr = text(&output.stderr);
        assert!(stderr.contains("duplicate root"), "{roots:?} {stderr}");
        assert_eq!(fixture.calls(), Vec::<String>::new(), "{roots:?}");
    }
}

#[test]
fn a_missing_root_is_refused_before_any_fetch() {
    let fixture = Fixture::new("sources-fetch-missing", &["one"]);
    let output = fixture.fetch(&["one", "absent"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(stderr.contains("absent"), "{stderr}");
    assert_eq!(fixture.calls(), Vec::<String>::new());
}

#[test]
fn the_fetch_runs_in_the_root_directory_and_the_working_directory_is_unchanged() {
    let fixture = Fixture::new("sources-fetch-cwd", &["one"]);
    let absolute = fixture.work.join("one");
    let output = fixture.fetch(&[absolute.to_str().unwrap_or_default()]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(fixture.calls(), ["one|fetch --locked"]);
    assert!(Path::new(&fixture.work).is_dir());
}

fn shim(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn only_cargo_is_started_and_nothing_else_reaches_the_network() {
    let temp = scratch("sources-fetch-offline");
    let work = temp.join("work");
    let bin = temp.join("bin");
    for root in ["one", "two"] {
        fs::create_dir_all(work.join(root)).unwrap_or_else(|e| unreachable!("{e}"));
    }
    fs::create_dir_all(&bin).unwrap_or_else(|e| unreachable!("{e}"));
    let programs = temp.join("programs");
    let marker = temp.join("FAIL");
    shim(
        &bin,
        "cargo",
        "echo \"cargo $*\" >> \"$FAKE_PROGRAMS\"\necho \"fetched $PWD\"\n",
    );
    for name in ["curl", "wget", "gh", "git"] {
        shim(
            &bin,
            name,
            &format!(
                "echo \"{name} $*\" >> \"$FAKE_PROGRAMS\"\necho \"{name} $*\" >> \"$FAKE_FAIL\"\nexit 97\n"
            ),
        );
    }
    let proxy = "http://127.0.0.1:9";
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["sources", "fetch", "--roots", "one", "two"])
        .current_dir(&work)
        .env_clear()
        .env("PATH", &bin)
        .env("RUNNER_TEMP", temp.path())
        .env("HOME", temp.join("home"))
        .env("FAKE_PROGRAMS", &programs)
        .env("FAKE_FAIL", &marker);
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env(key, proxy);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        !marker.exists(),
        "a network tool was started: {}",
        fs::read_to_string(&marker).unwrap_or_default()
    );
    let started: Vec<String> = fs::read_to_string(&programs)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(started, ["cargo fetch --locked", "cargo fetch --locked"]);
}

const PLAIN_CONFIG: &str = "[core]\n\tbare = true\n";
const TOKEN_CONFIG: &str =
    "[remote \"origin\"]\n\turl = https://x-access-token:ghs_secret@github.com/acme/shared-lib\n";

#[test]
fn a_database_fetched_by_the_run_has_its_hooks_and_fetch_head_left_out() {
    let fixture = Fixture::new("sources-fetch-scrub", &["one", "two"]);
    fixture.fetched_database("two", PLAIN_CONFIG);
    let output = fixture.fetch(&["one", "two"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let database = fixture.database();
    assert!(database.join("config").is_file());
    assert!(!database.join("hooks").exists());
    assert!(!database.join("FETCH_HEAD").exists());
}

#[test]
fn a_database_fetched_by_the_run_that_holds_a_credential_fails_the_fetch() {
    let fixture = Fixture::new("sources-fetch-credential", &["one", "two"]);
    fixture.fetched_database("two", TOKEN_CONFIG);
    let output = fixture.fetch(&["one", "two"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = text(&output.stderr);
    assert!(stderr.contains("credential"), "{stderr}");
    assert!(stderr.contains("config"), "{stderr}");
    assert!(!stderr.contains("ghs_secret"), "{stderr}");
}

#[test]
fn a_run_without_a_home_directory_fails_the_fetch() {
    let fixture = Fixture::new("sources-fetch-no-home", &["one"]);
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["sources", "fetch", "--roots", "one"])
        .current_dir(&fixture.work)
        .env("PATH", format!("{}:/usr/bin:/bin", fixture.bin.display()))
        .env("RUNNER_TEMP", fixture.temp.path())
        .env("FAKE_CARGO_CALLS", fixture.calls_path())
        .env_remove("HOME")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        text(&output.stderr).contains("HOME is not set"),
        "{output:?}"
    );
}
