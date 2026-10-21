use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use super::*;

const SECRET: &str = "ghs_SecretToken123";

fn token() -> Token {
    Token::new(SECRET).expect("valid token")
}

/// Run the real `git credential fill` against a generated configuration.
fn credential_fill(config: &Path, input: &str) -> (bool, String) {
    let mut child = crate::git_fixture::sterile_command()
        .args(["credential", "fill"])
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("git runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write");
    let output = child.wait_with_output().expect("wait");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

fn write_config(dir: &Path, repositories: &[&str]) -> std::path::PathBuf {
    let token_file = dir.join("token");
    fs::write(&token_file, SECRET).expect("token");
    let names: Vec<String> = repositories.iter().map(|r| (*r).to_owned()).collect();
    let config = dir.join("gitconfig");
    fs::write(
        &config,
        git_config("ChainArgos", &names, &token_file).expect("config"),
    )
    .expect("config file");
    config
}

#[test]
fn the_credential_helper_answers_only_for_the_exact_allowlisted_urls() {
    let dir = tempfile::tempdir().expect("dir");
    let config = write_config(dir.path(), &["processor-shared", "eth-domain"]);
    let ask = |host: &str, path: &str, protocol: &str| {
        credential_fill(
            &config,
            &format!("protocol={protocol}\nhost={host}\npath={path}\n\n"),
        )
    };
    for path in [
        "ChainArgos/processor-shared",
        "ChainArgos/processor-shared.git",
        "ChainArgos/eth-domain",
    ] {
        let (ok, out) = ask("github.com", path, "https");
        assert!(
            ok && out.contains(&format!("password={SECRET}")),
            "{path}: {out}"
        );
        assert!(out.contains("username=x-access-token"), "{out}");
    }
    for (host, path, protocol) in [
        ("github.com", "ChainArgos/other-repo", "https"),
        ("github.com", "ChainArgos/processor-shared-evil", "https"),
        ("github.com", "Other/processor-shared", "https"),
        ("github.com", "ChainArgos/processor-shared/../x", "https"),
        ("evil.example", "ChainArgos/processor-shared", "https"),
        (
            "github.com.evil.example",
            "ChainArgos/processor-shared",
            "https",
        ),
        ("gist.github.com", "ChainArgos/processor-shared", "https"),
        ("github.com", "ChainArgos/processor-shared", "http"),
    ] {
        let (ok, out) = ask(host, path, protocol);
        assert!(
            !ok && !out.contains(SECRET),
            "{protocol}://{host}/{path} must get no credential: {out}"
        );
    }
}

#[test]
fn the_generated_config_carries_no_token_value_and_no_other_helper() {
    let dir = tempfile::tempdir().expect("dir");
    let config = write_config(dir.path(), &["processor-shared"]);
    let text = fs::read_to_string(config).expect("read");
    assert!(
        !text.contains(SECRET),
        "the token stays in its file: {text}"
    );
    assert!(text.contains("useHttpPath = true"));
    assert_eq!(
        text.matches("helper =").count(),
        2,
        "plain and .git URL: {text}"
    );
    assert!(!text.contains("askpass") && !text.contains("[http"));
    assert!(git_config("ChainArgos", &["x".to_owned()], Path::new("/tmp/a b/token")).is_err());
    assert!(git_config("ChainArgos", &["x".to_owned()], Path::new("relative")).is_err());
    assert!(git_config("ChainArgos", &["x".to_owned()], Path::new("/tmp/t'oken")).is_err());
}

#[test]
fn curl_config_pins_https_and_the_api_origin_and_rejects_hostile_input() {
    let out = Path::new("/tmp/velnor/out.json");
    let text = curl_config(
        &token(),
        "GET",
        "repos/ChainArgos/x/releases/assets/7",
        "application/octet-stream",
        out,
        false,
    )
    .expect("config");
    for line in [
        "proto = \"=https\"",
        "proto-redir = \"=https\"",
        "url = \"https://api.github.com/repos/ChainArgos/x/releases/assets/7\"",
        "header = \"Accept: application/octet-stream\"",
        "output = \"/tmp/velnor/out.json\"",
        "location",
        "fail-with-body",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    assert!(!text.contains("location-trusted"), "{text}");
    for (method, route, accept, output) in [
        ("POST", "repos/a/b", "application/json", out),
        ("GET", "/repos/a/b", "application/json", out),
        ("GET", "repos/a/../b", "application/json", out),
        ("GET", "repos/a b", "application/json", out),
        (
            "GET",
            "repos/a\"\nurl=\"https://evil",
            "application/json",
            out,
        ),
        ("GET", "repos/a/b", "text/x\"", out),
        (
            "GET",
            "repos/a/b",
            "application/json",
            Path::new("/tmp/a b"),
        ),
        ("GET", "repos/a/b", "application/json", Path::new("rel")),
    ] {
        assert!(
            curl_config(&token(), method, route, accept, output, false).is_err(),
            "{method} {route} {accept} {output:?}"
        );
    }
    let status = curl_config(
        &token(),
        "DELETE",
        "installation/token",
        "application/json",
        Path::new("/dev/null"),
        true,
    )
    .expect("status config");
    assert!(
        status.contains("write-out = \"%{http_code}\"") && status.contains("request = \"DELETE\"")
    );
}

#[test]
fn tokens_validate_and_never_print() {
    assert!(Token::new("").is_err());
    for bad in ["a b", "a\nb", "a\"b", "a$b", "a;b"] {
        assert!(Token::new(bad).is_err(), "{bad:?}");
    }
    assert!(!format!("{:?}", token()).contains(SECRET));
}

#[cfg(unix)]
#[test]
fn the_session_directory_is_private_and_vanishes_on_drop() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("temp");
    let session = Session::create(
        temp.path(),
        token(),
        "ChainArgos",
        &["processor-shared".to_owned()],
        Vec::new(),
    )
    .expect("session");
    let dir = temp.path().join("velnor/private-fetch");
    let mode = |p: &Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&session.token_file()), 0o600);
    assert_eq!(mode(&session.git_config_file()), 0o600);
    assert_eq!(
        fs::read_to_string(session.token_file()).expect("token"),
        SECRET
    );
    drop(session);
    assert!(!dir.exists(), "credential directory removed on drop");
}

#[test]
fn a_stale_directory_is_scrubbed_before_a_new_session_starts() {
    let temp = tempfile::tempdir().expect("temp");
    let stale = temp.path().join("velnor/private-fetch");
    fs::create_dir_all(&stale).expect("stale");
    fs::write(stale.join("token"), "old-secret").expect("old");
    let session = Session::create(
        temp.path(),
        token(),
        "ChainArgos",
        &["a".to_owned()],
        Vec::new(),
    )
    .expect("session");
    assert_eq!(
        fs::read_to_string(session.token_file()).expect("token"),
        SECRET
    );
    assert_eq!(fs::read_dir(&stale).expect("dir").count(), 2);
}

#[test]
fn cargo_fetch_is_locked_isolated_and_carries_no_token() {
    let temp = tempfile::tempdir().expect("temp");
    let session = Session::create(
        temp.path(),
        token(),
        "ChainArgos",
        &["processor-shared".to_owned()],
        vec![("MISE_CARGO_HOME".to_owned(), "/runner/cargo".to_owned())],
    )
    .expect("session");
    let command = session
        .cargo_fetch_command(Path::new("/work/Cargo.toml"), "rust@1.98.1")
        .expect("command");
    let argv: Vec<String> = command
        .argv()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(argv[0], "mise");
    for needed in [
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "--",
        "cargo",
        "fetch",
        "--locked",
        "--manifest-path",
        "/work/Cargo.toml",
    ] {
        assert!(argv.iter().any(|a| a == needed), "{needed}: {argv:?}");
    }
    assert!(!argv.iter().any(|a| a == "--offline"), "{argv:?}");
    assert!(!argv.iter().any(|a| a.contains(SECRET)));
    assert_eq!(
        command.cwd().map(|p| p.ends_with("velnor/cargo-clean")),
        Some(true),
        "runs outside the checkout"
    );
    let ambient = vec![
        (OsString::from("PATH"), OsString::from("/bin")),
        (OsString::from("GIT_ASKPASS"), OsString::from("/evil")),
        (OsString::from("GITHUB_TOKEN"), OsString::from("ambient")),
        (
            OsString::from("VELNOR_PRIVATE_FETCH_TOKEN"),
            OsString::from("ambient"),
        ),
    ];
    let env = command.spawn_env(&ambient);
    let get = |key: &str| {
        env.iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        get("GIT_CONFIG_GLOBAL"),
        Some(session.git_config_file().into_os_string())
    );
    assert_eq!(
        get("CARGO_NET_GIT_FETCH_WITH_CLI"),
        Some(OsString::from("true"))
    );
    assert_eq!(get("GIT_ALLOW_PROTOCOL"), Some(OsString::from("https")));
    assert_eq!(
        get("MISE_CARGO_HOME"),
        Some(OsString::from("/runner/cargo"))
    );
    for key in ["GIT_ASKPASS", "GITHUB_TOKEN", "VELNOR_PRIVATE_FETCH_TOKEN"] {
        assert!(get(key).is_none(), "{key} must not reach cargo or git");
    }
    for (_, value) in &env {
        assert!(!value.to_string_lossy().contains(SECRET));
    }
}

fn rust_session(temp: &Path) -> Session {
    Session::create(
        temp,
        token(),
        "ChainArgos",
        &["processor-shared-rust".to_owned()],
        vec![("MISE_CARGO_HOME".to_owned(), "/runner/cargo".to_owned())],
    )
    .expect("The private fetch session must be created.")
}

fn argv_of(command: &IsolatedCommand) -> Vec<String> {
    command
        .argv()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn cargo_fetch_runs_exactly_the_rust_it_is_given() {
    let temp = tempfile::tempdir().expect("The test directory must exist.");
    let session = rust_session(temp.path());
    let argv = argv_of(
        &session
            .cargo_fetch_command(Path::new("/work/Cargo.toml"), "rust@1.98.1")
            .expect("The toolchain command must be created."),
    );
    let exec = argv
        .iter()
        .position(|a| a == "exec")
        .expect("The command must contain exec.");
    assert_eq!(argv[exec + 1], "rust@1.98.1", "{argv:?}");
}

#[test]
fn the_toolchain_check_asks_mise_where_for_the_same_rust_and_carries_no_token() {
    let temp = tempfile::tempdir().expect("The test directory must exist.");
    let session = rust_session(temp.path());
    let command = session
        .rust_check_command("rust@1.98.1")
        .expect("The toolchain command must be created.");
    let argv = argv_of(&command);
    assert_eq!(argv[0], "mise");
    let tail: Vec<&str> = argv[argv.len() - 2..].iter().map(String::as_str).collect();
    assert_eq!(tail, ["where", "rust@1.98.1"], "{argv:?}");
    for flag in ["--no-config", "--no-env", "--no-hooks"] {
        assert!(argv.iter().any(|a| a == flag), "{flag}: {argv:?}");
    }
    let env = command.spawn_env(&[(
        OsString::from("VELNOR_PRIVATE_FETCH_TOKEN"),
        OsString::from(SECRET),
    )]);
    assert!(
        env.iter().all(
            |(k, v)| k != "VELNOR_PRIVATE_FETCH_TOKEN" && !v.to_string_lossy().contains(SECRET)
        )
    );
    assert!(
        env.iter()
            .any(|(k, v)| k == "MISE_CARGO_HOME" && v == "/runner/cargo"),
        "The check and fetch must use the same managed Mise environment."
    );
}
