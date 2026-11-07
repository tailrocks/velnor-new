// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

const DUMMY: &str = "fixture-not-an-actual-credential";

fn git_fixture(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let out = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .output()?;
    if !out.status.success() {
        return Err("fixture git failed".into());
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

fn response(stream: &mut TcpStream, code: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn read_request(
    stream: &mut TcpStream,
) -> Result<(Vec<u8>, usize, usize), Box<dyn std::error::Error>> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut data = Vec::new();
    let mut buffer = [0; 8192];
    let (header_end, length);
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err("incomplete request".into());
        }
        data.extend_from_slice(&buffer[..count]);
        if let Some(end) = data.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..end]);
            let len = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + len {
                header_end = end;
                length = len;
                break;
            }
        }
    }
    Ok((data, header_end, length))
}

fn serve_git(
    mut stream: TcpStream,
    project_root: &Path,
    authorized: &AtomicUsize,
) -> Result<(), Box<dyn std::error::Error>> {
    let (data, header_end, length) = read_request(&mut stream)?;
    let headers = String::from_utf8_lossy(&data[..header_end]);
    let expected = format!(
        "authorization: basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{DUMMY}"))
    )
    .to_ascii_lowercase();
    if !headers
        .lines()
        .any(|line| line.to_ascii_lowercase() == expected)
    {
        response(&mut stream, "401 Unauthorized", b"")?;
        return Ok(());
    }
    let argv = Command::new("/bin/ps").args(["-eo", "args"]).output()?;
    let arguments = String::from_utf8(argv.stdout)?;
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{DUMMY}"));
    assert!(!arguments.contains(DUMMY));
    assert!(!arguments.contains(&encoded));
    authorized.fetch_add(1, Ordering::Relaxed);
    let request = headers.lines().next().ok_or("missing request")?;
    let mut fields = request.split_whitespace();
    let method = fields.next().ok_or("method")?;
    let url = fields.next().ok_or("url")?;
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let content_type = headers
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.eq_ignore_ascii_case("content-type"))
                .map(|(_, value)| value.trim())
        })
        .unwrap_or("");
    let mut child = Command::new("/usr/bin/git")
        .arg("http-backend")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("REQUEST_METHOD", method)
        .env("CONTENT_TYPE", content_type)
        .env("CONTENT_LENGTH", length.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("stdin")?
        .write_all(&data[header_end + 4..header_end + 4 + length])?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err("backend failed".into());
    }
    write_backend_response(&mut stream, &out.stdout)
}

fn write_backend_response(
    stream: &mut TcpStream,
    output: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let end = output
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or("backend headers")?;
    let mut status = "200 OK";
    let head = String::from_utf8_lossy(&output[..end]);
    for line in head.lines() {
        if let Some(value) = line.strip_prefix("Status: ") {
            status = value.trim();
        }
    }
    write!(stream, "HTTP/1.1 {status}\r\n")?;
    for line in head.lines() {
        if !line.starts_with("Status:") && !line.to_ascii_lowercase().starts_with("content-length:")
        {
            write!(stream, "{line}\r\n")?;
        }
    }
    let body = &output[end + 4..];
    write!(
        stream,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

type TestServer = (
    String,
    Arc<AtomicBool>,
    Arc<AtomicUsize>,
    thread::JoinHandle<()>,
);

#[path = "crate_release_checkout_deadline_tests.rs"]
mod deadline_tests;

fn server(root: PathBuf) -> Result<TestServer, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&stop);
    let count = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&count);
    let worker = thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => serve_git(stream, &root, &observed).expect("serve git"),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    Ok((origin, stop, count, worker))
}

fn assert_no_credential(dir: &Path, encoded: &str) -> Result<(), Box<dyn std::error::Error>> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.file_type()?;
        if metadata.is_dir() {
            assert_no_credential(&entry.path(), encoded)?;
        } else if metadata.is_file() {
            let bytes = fs::read(entry.path())?;
            assert!(
                !bytes
                    .windows(DUMMY.len())
                    .any(|value| value == DUMMY.as_bytes())
            );
            assert!(
                !bytes
                    .windows(encoded.len())
                    .any(|value| value == encoded.as_bytes())
            );
        }
    }
    Ok(())
}

fn hostile_parent() -> Result<(), Box<dyn std::error::Error>> {
    let hostile = tempfile::tempdir()?;
    let marker = hostile.path().join("source-executed");
    let trace = hostile.path().join("git-trace");
    let config = hostile.path().join("global-config");
    fs::write(
        &config,
        format!(
            "[filter \"untrusted\"]\n smudge = sh -c 'touch {}; cat'\n required = true\n",
            marker.display()
        ),
    )?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            crate::libtest_case::name(concat!(
                module_path!(),
                "::checkout_fetches_exact_private_source_without_credential_files"
            )),
        ])
        .env("VELNOR_CHECKOUT_TEST_CHILD", "1")
        .env("GIT_CONFIG_GLOBAL", &config)
        .env("GIT_TRACE", &trace)
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .output()?;
    assert!(output.status.success(), "hostile-parent child failed");
    crate::libtest_case::assert_one_passed(&output);
    assert!(!marker.exists());
    assert!(!trace.exists());
    Ok(())
}

fn seed_remote(
    root: &Path,
    source: &Path,
    owner: &Path,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    git_fixture(source, &["init", "-q", "-b", "main"])?;
    fs::write(source.join("README.md"), "fixture\n")?;
    fs::write(source.join(".gitattributes"), "*.txt filter=untrusted\n")?;
    fs::write(source.join("data.txt"), "data\n")?;
    fs::write(
        source.join("build.rs"),
        "fn main() { panic!(\"source execution forbidden\") }\n",
    )?;
    git_fixture(source, &["add", "."])?;
    git_fixture(source, &["commit", "-qm", "fixture"])?;
    git_fixture(source, &["tag", "v0.1.0"])?;
    git_fixture(
        source,
        &[
            "tag",
            "-a",
            "velnor-receipt/12/1/operation-1",
            "-m",
            "private journal fixture",
        ],
    )?;
    let sha = git_fixture(source, &["rev-parse", "HEAD"])?;
    fs::write(source.join("README.md"), "later commit\n")?;
    git_fixture(source, &["add", "README.md"])?;
    git_fixture(source, &["commit", "-qm", "later"])?;
    let head = git_fixture(source, &["rev-parse", "HEAD"])?;
    let bare = owner.join("demo.git");
    git_fixture(
        root,
        &["init", "-q", "--bare", bare.to_str().ok_or("bare path")?],
    )?;
    git_fixture(
        source,
        &["remote", "add", "origin", bare.to_str().ok_or("bare path")?],
    )?;
    git_fixture(source, &["push", "-q", "origin", "main", "--tags"])?;
    Ok((sha, head))
}

#[test]
fn checkout_fetches_exact_private_source_without_credential_files()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("VELNOR_CHECKOUT_TEST_CHILD").is_none() {
        return hostile_parent();
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let source = root.join("source");
    let owner = root.join("owner");
    let destination = root.join("checkout");
    fs::create_dir(&source)?;
    fs::create_dir(&owner)?;
    fs::create_dir(&destination)?;
    let (sha, head) = seed_remote(&root, &source, &owner)?;
    let (origin, stop, count, worker) = server(root.clone())?;
    let context = CheckoutContext {
        origin: &origin,
        test_http: true,
        deadline: velnor_actions_mise::CheckDeadline::after(Duration::from_secs(30))?,
    };
    checkout(&SystemGit, &destination, "owner/demo", &sha, DUMMY, context)?;
    assert!(count.load(Ordering::Relaxed) > 0);
    assert_eq!(git_fixture(&destination, &["rev-parse", "HEAD"])?, sha);
    assert_eq!(
        git_fixture(&destination, &["rev-parse", "refs/tags/v0.1.0"])?,
        sha
    );
    let journal_ref = "refs/tags/velnor-receipt/12/1/operation-1";
    let journal_object = git_fixture(&source, &["rev-parse", journal_ref])?;
    assert!(git_fixture(&destination, &["rev-parse", "--verify", journal_ref]).is_err());
    assert!(git_fixture(&destination, &["cat-file", "-e", &journal_object]).is_err());
    assert_eq!(git_fixture(&destination, &["status", "--porcelain"])?, "");
    assert_eq!(
        git_fixture(&destination, &["rev-parse", "refs/remotes/origin/main"])?,
        head
    );
    assert_eq!(
        fs::read_to_string(destination.join("README.md"))?,
        "fixture\n"
    );
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{DUMMY}"));
    assert_no_credential(&destination, &encoded)?;
    let wrong_auth = root.join("wrong-auth");
    fs::create_dir(&wrong_auth)?;
    assert!(
        checkout(
            &SystemGit,
            &wrong_auth,
            "owner/demo",
            &sha,
            "wrong-dummy",
            context
        )
        .is_err()
    );
    assert_no_credential(
        &wrong_auth,
        &base64::engine::general_purpose::STANDARD.encode("x-access-token:wrong-dummy"),
    )?;
    let missing = root.join("missing");
    fs::create_dir(&missing)?;
    assert!(
        checkout(
            &SystemGit,
            &missing,
            "owner/demo",
            &"a".repeat(40),
            DUMMY,
            context
        )
        .is_err()
    );
    assert_no_credential(&missing, &encoded)?;
    stop.store(true, Ordering::Relaxed);
    worker.join().map_err(|_| "server panicked")?;
    Ok(())
}

#[path = "crate_release_checkout_redirect_tests.rs"]
mod redirect_tests;
