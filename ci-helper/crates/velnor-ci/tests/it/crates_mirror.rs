// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::io::Repo;

type Requests = Arc<Mutex<Vec<String>>>;

type TestResult = Result<(), Box<dyn Error>>;

struct Registry {
    url: String,
    requests: Requests,
    address: SocketAddr,
    stopping: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Registry {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        drop(TcpStream::connect(self.address));
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

fn setup(root: &Path, registry: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["crates-mirror", "--registry", registry, "--cainfo"])
        .arg(root.join("ca.pem"))
        .env("CARGO_HOME", root.join("home"))
        .env("RUNNER_TEMP", root)
        .env("GITHUB_ENV", root.join("env"))
        .env("GITHUB_PATH", root.join("path"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn cargo_fetches_uncached_crates_with_changed_homes_and_explicit_toolchain() -> TestResult {
    mirror_fixture(direct_fetches)
}

#[test]
fn build_scripts_fetch_uncached_crates_from_tmp_through_path_and_cargo_env() -> TestResult {
    mirror_fixture(child_fetches)
}

fn mirror_fixture(exercise: fn(&Repo, &Path, &Path) -> TestResult) -> TestResult {
    let repo = Repo::empty("crates-mirror");
    let root = repo.root();
    repo.write_unstaged("ca.pem", "not used by the local HTTP fixture");
    repo.write_unstaged("shadow/cargo", "not executable");
    fs::set_permissions(root.join("shadow/cargo"), fs::Permissions::from_mode(0o644))?;
    repo.write_unstaged(
        "package/probe-1.0.0/Cargo.toml",
        "[package]\nname='probe'\nversion='1.0.0'\nedition='2024'\n",
    );
    repo.write_unstaged("package/probe-1.0.0/src/lib.rs", "");
    let packed = Command::new("tar")
        .env("COPYFILE_DISABLE", "1")
        .args(["-czf"])
        .arg(root.join("probe.crate"))
        .args(["-C"])
        .arg(root.join("package"))
        .arg("probe-1.0.0")
        .status()?;
    assert!(packed.success());
    let archive = fs::read(root.join("probe.crate"))?;
    let checksum = velnor_rust::sha256_hex(&archive);
    let registry = serve(archive, &checksum)?;
    for _ in 0..2 {
        let output = setup(root, &registry.url);
        assert!(output.status.success(), "{output:?}");
    }
    let config = root.join("home/config.toml");
    let text = fs::read_to_string(&config)?;
    assert!(text.contains(&root.join("ca.pem").display().to_string()));
    let env = fs::read_to_string(root.join("env"))?;
    let wrapper = fs::canonicalize(root.join("velnor-cargo/cargo"))?;
    assert!(env.contains(&format!("CARGO={}", wrapper.display())));
    exercise(&repo, &wrapper, &config)?;
    let requests = registry
        .requests
        .lock()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        requests
            .iter()
            .filter(|path| path.as_str() == "/crates/probe/1.0.0/download")
            .count(),
        2,
        "{requests:?}"
    );
    Ok(())
}

fn mise_shim(repo: &Repo) -> TestResult {
    let output = Command::new("rustup").args(["which", "cargo"]).output()?;
    assert!(output.status.success(), "{output:?}");
    let cargo = String::from_utf8(output.stdout)?;
    fs::create_dir(repo.root().join("native"))?;
    let native = repo.root().join("native/cargo");
    std::os::unix::fs::symlink(cargo.trim(), &native)?;
    assert!(fs::symlink_metadata(&native)?.file_type().is_symlink());
    let cargo = serde_json::to_string(&native)?;
    repo.write_unstaged("mise-tools/mise.rs", &format!(r#"fn main() {{
        let args: Vec<_> = std::env::args().collect();
        match std::path::Path::new(&args[0]).file_stem().and_then(|name| name.to_str()) {{
            Some("mise") if args[1..] == ["which", "cargo"] => println!("{{}}", {cargo}),
            Some("cargo") => std::process::exit(std::process::Command::new({cargo}).args(&args[1..]).status().unwrap().code().unwrap_or(1)),
            _ => {{ eprintln!("invalid Mise shim name"); std::process::exit(1); }}
        }}
    }}"#));
    let status = Command::new("rustc")
        .arg(repo.root().join("mise-tools/mise.rs"))
        .arg("-o")
        .arg(repo.root().join("mise-tools/mise"))
        .status()?;
    assert!(status.success());
    fs::create_dir(repo.root().join("mise-shims"))?;
    std::os::unix::fs::symlink(
        repo.root().join("mise-tools/mise"),
        repo.root().join("mise-shims/cargo"),
    )?;
    Ok(())
}

fn direct_fetches(repo: &Repo, wrapper: &Path, config: &Path) -> TestResult {
    let root = repo.root();
    for (index, program, toolchain) in [
        (0, "cargo", false),
        (1, wrapper.to_str().ok_or("wrapper path")?, true),
    ] {
        let project = format!("tmp-{index}");
        repo.write_unstaged(&format!("{project}/Cargo.toml"), "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n");
        repo.write_unstaged(&format!("{project}/src/lib.rs"), "");
        let paths = [
            fs::canonicalize(root.join("velnor-cargo"))?,
            root.join("shadow"),
            std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME")?).join(".cargo/bin"),
        ]
        .into_iter()
        .chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ))
        .collect::<Vec<_>>();
        let mut command = Command::new(program);
        if toolchain {
            let active = Command::new("rustup")
                .args(["show", "active-toolchain"])
                .output()?;
            assert!(active.status.success());
            let toolchain = String::from_utf8(active.stdout)?;
            command.arg(format!(
                "+{}",
                toolchain
                    .split_whitespace()
                    .next()
                    .ok_or("active toolchain")?
            ));
        }
        let output = command
            .args(["fetch", "--manifest-path"])
            .arg(root.join(&project).join("Cargo.toml"))
            .current_dir(root.join(&project))
            .env("PATH", std::env::join_paths(paths)?)
            .env("CARGO_HOME", root.join(format!("different-home-{index}")))
            .env("VELNOR_CARGO_CONFIG", config)
            .env("VELNOR_CARGO_WRAPPER", wrapper)
            .env("CARGO_NET_OFFLINE", "false")
            .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
            .env("NO_PROXY", "127.0.0.1")
            .env("CARGO_NET_RETRY", "0")
            .env(
                "CARGO_REGISTRIES_CRATES_IO_INDEX",
                "sparse+http://127.0.0.1:1/index/",
            )
            .output()?;
        assert!(output.status.success(), "{output:?}");
        assert!(
            root.join(format!("different-home-{index}/registry/src"))
                .is_dir()
        );
    }
    Ok(())
}

fn child_fetches(repo: &Repo, wrapper: &Path, config: &Path) -> TestResult {
    let root = repo.root();
    mise_shim(repo)?;
    repo.write_unstaged("tmp-0/Cargo.toml", "[package]\nname='consumer'\nversion='1.0.0'\nedition='2024'\n[dependencies]\nprobe='=1.0.0'\n");
    repo.write_unstaged("tmp-0/src/lib.rs", "");
    repo.write_unstaged(
        "outer/Cargo.toml",
        "[package]\nname='outer'\nversion='1.0.0'\nedition='2024'\n",
    );
    repo.write_unstaged("outer/src/lib.rs", "");
    repo.write_unstaged("outer/build.rs", r#"fn main() {
        let root = std::env::var("MIRROR_FIXTURE_ROOT").unwrap();
        for (name, program) in [("path", "cargo".to_owned()), ("env", std::env::var("CARGO").unwrap())] {
            let status = std::process::Command::new(program).args(["fetch", "--manifest-path"])
                .arg(format!("{root}/tmp-0/Cargo.toml"))
                .env("CARGO_HOME", format!("{root}/child-home-{name}"))
                .current_dir(std::env::temp_dir()).status().unwrap();
            assert!(status.success());
        }
    }"#);
    let paths = [
        wrapper.parent().ok_or("wrapper parent")?.to_path_buf(),
        root.join("shadow"),
        root.join("mise-shims"),
        std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME")?).join(".cargo/bin"),
    ]
    .into_iter()
    .chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ))
    .collect::<Vec<_>>();
    let output = Command::new(wrapper)
        .args(["build", "--manifest-path"])
        .arg(root.join("outer/Cargo.toml"))
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env("MIRROR_FIXTURE_ROOT", root)
        .env("CARGO_HOME", root.join("outer-home"))
        .env("CARGO_TARGET_DIR", root.join("outer-target"))
        .env("VELNOR_CARGO_CONFIG", config)
        .env("VELNOR_CARGO_WRAPPER", wrapper)
        .env("PATH", std::env::join_paths(paths)?)
        .env("CARGO_NET_OFFLINE", "false")
        .env("CARGO_HTTP_PROXY", "http://127.0.0.1:1")
        .env("NO_PROXY", "127.0.0.1")
        .env("CARGO_NET_RETRY", "0")
        .env(
            "CARGO_REGISTRIES_CRATES_IO_INDEX",
            "sparse+http://127.0.0.1:1/index/",
        )
        .output()?;
    assert!(output.status.success(), "{output:?}");
    Ok(())
}

#[test]
fn mirror_setup_preserves_unowned_cargo_configuration() -> TestResult {
    for name in ["config", "config.toml"] {
        let repo = Repo::empty("crates-mirror-owned");
        repo.write_unstaged("ca.pem", "fixture");
        repo.write_unstaged(&format!("home/{name}"), "[net]\noffline=true\n");
        let output = setup(repo.root(), "sparse+https://mirror.example/index/");
        assert!(!output.status.success(), "{output:?}");
        assert_eq!(
            fs::read_to_string(repo.root().join(format!("home/{name}")))?,
            "[net]\noffline=true\n"
        );
    }
    Ok(())
}

fn serve(archive: Vec<u8>, checksum: &str) -> Result<Registry, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let registry = format!("sparse+http://{address}/index/");
    let responses = BTreeMap::from([
        ("/index/config.json".to_owned(), format!("{{\"dl\":\"http://{address}/crates\"}}").into_bytes()),
        ("/index/pr/ob/probe".to_owned(), format!("{{\"name\":\"probe\",\"vers\":\"1.0.0\",\"deps\":[],\"cksum\":\"{checksum}\",\"features\":{{}},\"yanked\":false}}\n").into_bytes()),
        ("/crates/probe/1.0.0/download".to_owned(), archive),
    ]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let requests = seen.clone();
    let stopping = Arc::new(AtomicBool::new(false));
    let stop = stopping.clone();
    let thread = std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(1)))
                .unwrap_or_else(|e| unreachable!("{e}"));
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                request.push(byte[0]);
            }
            let request = String::from_utf8_lossy(&request);
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            requests
                .lock()
                .unwrap_or_else(|e| unreachable!("{e}"))
                .push(path.to_owned());
            let (status, body) = responses
                .get(path)
                .map_or(("404 Not Found", &b""[..]), |body| {
                    ("200 OK", body.as_slice())
                });
            let header = format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(header.as_bytes())
                .and_then(|()| stream.write_all(body))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
    });
    Ok(Registry {
        url: registry,
        requests: seen,
        address,
        stopping,
        thread: Some(thread),
    })
}
