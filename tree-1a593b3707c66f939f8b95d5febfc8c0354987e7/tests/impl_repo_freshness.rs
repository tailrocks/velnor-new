//! Freshness-procedure pins: boot equality, renovate, update rules.
//!
//! Covers BOOT-3.4, GAP-E.2, RQ-2.11, RQ-9.8, VER-0.1, VER-1.5, VER-1.7,
//! VER-2.27, VER-3.2, VER-3.3, VER-3.4, VER-3.7, VER-4.4. Mechanical halves
//! only; human residuals live in docs/content/docs/implemented/release-gates.mdx.

use std::error::Error;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

#[path = "fixtures/p12_property.rs"]
mod p12_property;

/// Repo root: two levels above this crate's manifest directory.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Read a repo-relative file to a string.
fn read(relative: &str) -> Result<String, Box<dyn Error>> {
    Ok(std::fs::read_to_string(repo_root().join(relative))?)
}

/// First double-quoted value on the first line containing `key`.
fn quoted_value(text: &str, key: &str) -> Result<String, Box<dyn Error>> {
    text.lines()
        .filter(|line| line.contains(key))
        .filter_map(|line| line.split('"').nth(1))
        .map(str::to_owned)
        .next()
        .ok_or_else(|| format!("{key} not found").into())
}

const FETCH_CAP: usize = 512 * 1024;

#[derive(Clone)]
struct TestResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct LocalServer {
    address: std::net::SocketAddr,
    responses: Arc<Mutex<std::collections::HashMap<String, TestResponse>>>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LocalServer {
    fn new(
        responses: std::collections::HashMap<String, TestResponse>,
    ) -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let responses = Arc::new(Mutex::new(responses));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_responses = Arc::clone(&responses);
        let worker_requests = Arc::clone(&requests);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Some((path, accept_encoding)) = read_request(&mut stream) {
                            if let Ok(mut seen) = worker_requests.lock() {
                                seen.push(accept_encoding);
                            }
                            let response = worker_responses
                                .lock()
                                .ok()
                                .and_then(|map| map.get(&path).cloned())
                                .unwrap_or_else(|| TestResponse {
                                    status: 404,
                                    headers: Vec::new(),
                                    body: b"not found".to_vec(),
                                });
                            let _response_write = write_response(&mut stream, &response);
                        }
                        let _shutdown = stream.shutdown(Shutdown::Both);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            address,
            responses,
            requests,
            stop,
            worker: Some(worker),
        })
    }

    fn url(&self, route: &str) -> String {
        format!("http://{}{}", self.address, route)
    }

    fn accept_encoding_headers(&self) -> Vec<String> {
        self.requests
            .lock()
            .map(|requests| requests.clone())
            .unwrap_or_default()
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _wake_result = TcpStream::connect(self.address);
        if let Some(worker) = self.worker.take() {
            let _join_result = worker.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..count]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > 16 * 1024 {
            return None;
        }
    }
    let text = String::from_utf8_lossy(&request);
    let mut lines = text.lines();
    let path = lines.next()?.split_whitespace().nth(1)?.to_owned();
    let accept_encoding = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.eq_ignore_ascii_case("accept-encoding"))
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default();
    Some((path, accept_encoding))
}

fn write_response(stream: &mut TcpStream, response: &TestResponse) -> std::io::Result<()> {
    let reason = if response.status == 200 {
        "OK"
    } else {
        "Bad Request"
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        reason,
        response.body.len()
    )?;
    for (key, value) in &response.headers {
        write!(stream, "{key}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(&response.body)
}

fn gzip_bytes(input: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut child = Command::new("python3")
        .args(["-c", "import gzip,sys; sys.stdout.buffer.write(gzip.compress(sys.stdin.buffer.read(), mtime=0))"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("python stdin unavailable")?
        .write_all(input)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "gzip fixture generation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}

fn copy_repo_file(source_root: &Path, fixture_root: &Path, relative: &str) -> std::io::Result<()> {
    let source = source_root.join(relative);
    let destination = fixture_root.join(relative);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(source, destination)?;
    Ok(())
}

fn latest_body(source: &str, latest: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if source.contains("crates.io/api/v1/crates/") {
        Ok(serde_json::to_vec(
            &json!({"crate": {"max_version": latest}}),
        )?)
    } else if source.contains("static.rust-lang.org") {
        Ok(format!("[pkg.rust]\nversion = \"{latest} (fixture)\"\n").into_bytes())
    } else {
        Ok(serde_json::to_vec(&json!({"tag_name": latest}))?)
    }
}

fn rewrite_tool_sources(
    inventory: &mut Value,
    server: &LocalServer,
    stamp: &str,
    responses: &mut std::collections::HashMap<String, TestResponse>,
) -> Result<(), Box<dyn Error>> {
    let tools = inventory["tools"]
        .as_array_mut()
        .ok_or("tools must be an array")?;
    for (index, tool) in tools.iter_mut().enumerate() {
        let route = format!("/tool/{index}");
        let pin = tool["pinned"]
            .as_str()
            .ok_or("tool pin missing")?
            .to_owned();
        let source = tool["source"]
            .as_str()
            .ok_or("tool source missing")?
            .to_owned();
        tool["source"] = Value::String(server.url(&route));
        tool["latest"] = Value::String(pin.clone());
        tool["qualified"] = Value::String(pin.clone());
        tool["status"] = Value::String("current".to_owned());
        tool["checked_at"] = Value::String(stamp.to_owned());
        responses.insert(
            route,
            TestResponse {
                status: 200,
                headers: Vec::new(),
                body: latest_body(&source, &pin)?,
            },
        );
    }
    Ok(())
}

fn rewrite_action_sources(
    inventory: &mut Value,
    server: &LocalServer,
    stamp: &str,
    responses: &mut std::collections::HashMap<String, TestResponse>,
) -> Result<(), Box<dyn Error>> {
    let actions = inventory["actions"]
        .as_array_mut()
        .ok_or("actions must be an array")?;
    for (index, action) in actions.iter_mut().enumerate() {
        let route = format!("/action/{index}");
        let pin = action["pinned_version"]
            .as_str()
            .ok_or("action pin missing")?
            .to_owned();
        let source = action["source"]
            .as_str()
            .ok_or("action source missing")?
            .to_owned();
        action["source"] = Value::String(server.url(&route));
        action["latest"] = Value::String(pin.clone());
        action["qualified_version"] = Value::String(pin.clone());
        action["qualified_sha"] = action["pinned_sha"].clone();
        action["status"] = Value::String("current".to_owned());
        action["checked_at"] = Value::String(stamp.to_owned());
        responses.insert(
            route,
            TestResponse {
                status: 200,
                headers: Vec::new(),
                body: latest_body(&source, &pin)?,
            },
        );
    }
    Ok(())
}

fn rewrite_runner_source(
    inventory: &mut Value,
    server: &LocalServer,
    stamp: &str,
    responses: &mut std::collections::HashMap<String, TestResponse>,
) -> Result<(), Box<dyn Error>> {
    let route = "/runner".to_owned();
    let runner = inventory["runner"]
        .as_object_mut()
        .ok_or("runner missing")?;
    let pin = runner
        .get("default")
        .and_then(Value::as_str)
        .ok_or("runner default missing")?
        .to_owned();
    runner.insert("source".to_owned(), Value::String(server.url(&route)));
    runner.insert("checked_at".to_owned(), Value::String(stamp.to_owned()));
    runner.insert("status".to_owned(), Value::String("current".to_owned()));
    responses.insert(
        route,
        TestResponse {
            status: 200,
            headers: Vec::new(),
            body: latest_body("github", &pin)?,
        },
    );
    inventory["checked_at"] = Value::String(stamp.to_owned());
    Ok(())
}

fn rewrite_inventory_sources(
    inventory: &mut Value,
    server: &LocalServer,
    stamp: &str,
) -> Result<std::collections::HashMap<String, TestResponse>, Box<dyn Error>> {
    let mut responses = std::collections::HashMap::new();
    rewrite_tool_sources(inventory, server, stamp, &mut responses)?;
    rewrite_action_sources(inventory, server, stamp, &mut responses)?;
    rewrite_runner_source(inventory, server, stamp, &mut responses)?;
    Ok(responses)
}

fn write_fixture_files(fixture_root: &Path, inventory: &Value) -> Result<(), Box<dyn Error>> {
    let source_root = repo_root();
    let files = [
        ".velnor/version-policy.toml",
        "crates/adapters/velnor-actions-mise-catalog/src/catalog.rs",
        "crates/adapters/velnor-actions-actionlint/src/actions.rs",
        "crates/adapters/velnor-actions-actionlint/src/capabilities.rs",
        "crates/adapters/velnor-actions-actionlint/src/tools.rs",
        "crates/adapters/velnor-actions-actionlint/src/config.rs",
        "crates/services/velnor-actions-workflow-steps/src/action_ref.rs",
    ];
    for file in files {
        copy_repo_file(&source_root, fixture_root, file)?;
    }
    std::fs::create_dir_all(fixture_root.join(".velnor"))?;
    std::fs::write(
        fixture_root.join(".velnor/freshness-inventory.json"),
        serde_json::to_vec_pretty(inventory)?,
    )?;
    std::fs::write(
        fixture_root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/fixture\"]\nresolver = \"3\"\n",
    )?;
    std::fs::write(
        fixture_root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"freshness-fixture\"\nversion = \"0.1.0\"\n",
    )?;
    std::fs::write(
        fixture_root.join("deny.toml"),
        "[advisories]\nignore = []\n",
    )?;
    std::fs::create_dir_all(fixture_root.join(".cargo"))?;
    std::fs::write(
        fixture_root.join(".cargo/mutants.toml"),
        "# pinned: cargo-mutants = \"27.1.0\"\n\nexamine_globs = [\n    \"crates/fixture/src/lib.rs\",\n]\n",
    )?;
    std::fs::create_dir_all(fixture_root.join("crates/fixture/src"))?;
    std::fs::write(
        fixture_root.join("crates/fixture/Cargo.toml"),
        "[package]\nname = \"freshness-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    std::fs::write(fixture_root.join("crates/fixture/src/lib.rs"), "")?;
    std::fs::create_dir_all(
        fixture_root.join("crates/services/velnor-actions-orchestrator-selection/src"),
    )?;
    std::fs::write(
        fixture_root.join("crates/services/velnor-actions-orchestrator-selection/src/select.rs"),
        "pub fn fixture() {}\n",
    )?;
    std::fs::create_dir_all(fixture_root.join("scripts"))?;
    let script = source_root.join("scripts/check-freshness.sh");
    std::fs::copy(&script, fixture_root.join("scripts/check-freshness.sh"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            fixture_root.join("scripts/check-freshness.sh"),
            std::fs::Permissions::from_mode(0o755),
        )?;
    }
    Ok(())
}

fn fixture_inventory(
    fixture_root: &Path,
    server: &LocalServer,
) -> Result<(Value, std::collections::HashMap<String, TestResponse>), Box<dyn Error>> {
    let source_root = repo_root();
    let mut inventory: Value = serde_json::from_slice(&std::fs::read(
        source_root.join(".velnor/freshness-inventory.json"),
    )?)?;
    let now = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()?;
    if !now.status.success() {
        return Err("date failed while creating current fixture evidence".into());
    }
    let stamp = String::from_utf8(now.stdout)?.trim().to_owned();
    let responses = rewrite_inventory_sources(&mut inventory, server, &stamp)?;
    write_fixture_files(fixture_root, &inventory)?;
    Ok((inventory, responses))
}

fn run_upstream_probe(root: &Path) -> Result<Output, Box<dyn Error>> {
    let script = root.join("scripts/check-freshness.sh");
    Ok(Command::new("bash")
        .arg(script)
        .args([
            "--root",
            root.to_str().ok_or("non-UTF8 fixture path")?,
            "--check-upstream",
        ])
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env("NO_PROXY", "127.0.0.1,localhost")
        .env("no_proxy", "127.0.0.1,localhost")
        .output()?)
}

fn run_fixture(
    special_route: Option<(&str, TestResponse)>,
) -> Result<(Output, Vec<String>), Box<dyn Error>> {
    let root = crate::impl_cli_tmp::fresh_tempdir("freshness-http")?;
    let server = LocalServer::new(std::collections::HashMap::new())?;
    let (_inventory, mut responses) = fixture_inventory(&root, &server)?;
    if let Some((route, response)) = special_route {
        responses.insert(route.to_owned(), response);
    }
    if let Ok(mut server_responses) = server.responses.lock() {
        *server_responses = responses;
    }
    let output = run_upstream_probe(&root)?;
    let request_headers = server.accept_encoding_headers();
    drop(server);
    crate::impl_cli_tmp::cleanup(&root);
    Ok((output, request_headers))
}

fn first_tool_body() -> Result<Vec<u8>, Box<dyn Error>> {
    let inventory: Value = serde_json::from_slice(&std::fs::read(
        repo_root().join(".velnor/freshness-inventory.json"),
    )?)?;
    let tool = inventory["tools"][0]
        .as_object()
        .ok_or("first tool must be an object")?;
    latest_body(
        tool.get("source")
            .and_then(Value::as_str)
            .ok_or("first tool source missing")?,
        tool.get("pinned")
            .and_then(Value::as_str)
            .ok_or("first tool pin missing")?,
    )
}

fn response(headers: Vec<(String, String)>, body: Vec<u8>) -> TestResponse {
    TestResponse {
        status: 200,
        headers,
        body,
    }
}

fn assert_probe_rejected(output: &Output, diagnostic: &str) {
    assert!(!output.status.success(), "probe unexpectedly passed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("lookup_failed"),
        "expected a failed upstream-probe row, got:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains(diagnostic),
        "missing diagnostic {diagnostic:?} in:\n{stdout}"
    );
}

#[test]
fn boot34_mise_version_matches_catalog() -> Result<(), Box<dyn Error>> {
    let pinned = read(".mise-version")?;
    let catalog = read("crates/adapters/velnor-actions-mise-catalog/src/catalog.rs")?;
    assert_eq!(
        pinned.trim(),
        quoted_value(&catalog, "MISE_VERSION")?.as_str()
    );
    let gates = read("docs/content/docs/implemented/release-gates.mdx")?;
    assert!(
        gates.contains("BOOT-3.4"),
        "seed equality half must be recorded"
    );
    assert!(
        gates.contains(".velnor/generator.lock"),
        "lock side must name the seed-created lock"
    );
    Ok(())
}

#[test]
fn gape2_seed_rules_documented() -> Result<(), Box<dyn Error>> {
    let gates = read("docs/content/docs/implemented/release-gates.mdx")?;
    assert!(gates.contains("BOOT-4.2"), "seed rule must be recorded");
    assert!(
        gates.contains("2 distinct admin approvals"),
        "seed must require two approvals"
    );
    assert!(
        gates.contains("sha256"),
        "seed must require a rebuild hash match"
    );
    assert!(
        gates.contains("trust-on-review"),
        "pre-seed trust rule must be marked"
    );
    assert!(
        gates.contains("SEPARATE reviewed change"),
        "post-seed lock updates need their own review"
    );
    Ok(())
}

#[test]
fn rq211_lock_staleness_probe() -> Result<(), Box<dyn Error>> {
    let script = read("scripts/check-freshness.sh")?;
    assert!(
        script.contains("lock-staleness"),
        "script must probe staleness"
    );
    assert!(
        script.contains("exact `=x.y.z` (VER-2.26)"),
        "direct deps must declare exact versions"
    );
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(
        procedure.contains("MUST NOT remain stale"),
        "lock must not stay stale when the build passes"
    );
    assert!(
        procedure.contains("lock-staleness"),
        "procedure must cite the mechanical probe"
    );
    Ok(())
}

#[test]
fn rq98_risk_triggers_documented() -> Result<(), Box<dyn Error>> {
    let mutants = read(".cargo/mutants.toml")?;
    assert!(
        mutants.contains("examine_globs"),
        "mutant scope must be set"
    );
    assert!(
        mutants.contains("crates/services/velnor-actions-orchestrator-selection/src/select.rs"),
        "selection must be in mutant scope"
    );
    assert!(
        mutants.contains("NOT wired into CI"),
        "manual-only status must be explicit"
    );
    let triggers = read("docs/content/docs/implemented/verification-triggers.mdx")?;
    for technique in [
        "Mutation testing",
        "Property testing",
        "Fuzzing",
        "Miri / Loom",
        "cargo-semver-checks",
    ] {
        assert!(triggers.contains(technique), "triggers miss {technique}");
    }
    assert!(
        triggers.contains("NOT behavioral evidence"),
        "coverage must not count as behavioral evidence"
    );
    assert!(
        triggers.contains("MUST be pinned"),
        "CI use must require pinned tool versions"
    );
    Ok(())
}

#[test]
fn ver01_policy_header() -> Result<(), Box<dyn Error>> {
    let policy = read(".velnor/version-policy.toml")?;
    for setting in [
        "schema = 1",
        "channel = \"stable\"",
        "check_interval_hours = 24",
        "max_exception_days = 14",
    ] {
        assert!(policy.contains(setting), "policy header misses {setting}");
    }
    Ok(())
}

#[test]
fn ver15_incompatible_is_migration() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(procedure.contains("VER-1.5"), "procedure must cite VER-1.5");
    assert!(
        procedure.contains("required migration"),
        "incompatible updates need a migration"
    );
    assert!(
        procedure.contains("never silently omitted"),
        "holds must never be silent"
    );
    assert!(
        read("docs/content/docs/implemented/release-gates.mdx")?.contains("VER-1.5"),
        "human residual must be recorded"
    );
    Ok(())
}

#[test]
fn ver17_expedited_security_path() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(procedure.contains("VER-1.7"), "procedure must cite VER-1.7");
    assert!(
        procedure.contains("same-day"),
        "security path must be same-day"
    );
    assert!(
        procedure.contains("minimal scope"),
        "security path must be minimal-scope"
    );
    assert!(
        procedure.contains("full gate MUST"),
        "full gate must still pass before merge"
    );
    Ok(())
}

#[test]
fn ver227_renovate_proposes_all() -> Result<(), Box<dyn Error>> {
    let renovate = read("renovate.json")?;
    for token in [
        "\"cargo\"",
        "\"github-actions\"",
        "\"major\"",
        "needs-migration-review",
        "\"minor\"",
        "dependencyDashboard",
    ] {
        assert!(renovate.contains(token), "renovate.json misses {token}");
    }
    assert!(
        read("docs/content/docs/implemented/update-procedure.mdx")?.contains("never merges"),
        "renovate must propose only, never merge"
    );
    Ok(())
}

#[test]
fn ver32_one_coherent_set() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(procedure.contains("VER-3.2"), "procedure must cite VER-3.2");
    assert!(
        procedure.contains("ONE change covering ALL"),
        "updates must ship as one coherent set"
    );
    Ok(())
}

#[test]
fn ver33_records_and_qualifies() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(procedure.contains("VER-3.3"), "procedure must cite VER-3.3");
    assert!(
        procedure.contains("timestamp + version delta"),
        "update set must record timestamp and delta"
    );
    assert!(
        procedure.contains("ONLY Velnor-owned pins and locks"),
        "refresh must stay Velnor-owned"
    );
    for gate in ["cargo fmt", "Clippy", "doctests", "cargo deny"] {
        assert!(procedure.contains(gate), "qual list misses {gate}");
    }
    Ok(())
}

#[test]
fn ver34_tool_files_untouched() -> Result<(), Box<dyn Error>> {
    let renovate = read("renovate.json")?;
    for file in ["mise.toml", "mise.lock", "rust-toolchain.toml"] {
        assert!(renovate.contains(file), "renovate must name {file}");
    }
    assert!(
        renovate.contains("\"enabled\": false"),
        "tool inputs must be disabled in renovate"
    );
    assert!(
        renovate.contains("\"matchManagers\": [\"mise\"]"),
        "tool-input guard must match the mise manager that owns mise.toml/mise.lock"
    );
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(procedure.contains("VER-3.4"), "procedure must cite VER-3.4");
    assert!(
        procedure.contains("MUST NOT edit `mise.toml`"),
        "procedure must forbid tool-file edits"
    );
    Ok(())
}

#[test]
fn ver37_merge_after_qual() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(
        procedure.contains("only after qualification passes"),
        "pins must merge only after qual"
    );
    assert!(
        procedure.contains("--locked"),
        "normal builds must use --locked"
    );
    assert!(
        procedure.contains("never resolve versions"),
        "normal builds must never resolve versions"
    );
    let gates = read("docs/content/docs/implemented/release-gates.mdx")?;
    assert!(
        gates.contains("VER-3.7"),
        "protection residual must be recorded"
    );
    assert!(
        gates.contains("branch protection"),
        "residual must name branch protection"
    );
    Ok(())
}

#[test]
fn ver44_velnor_owned_refresh_only() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/content/docs/implemented/update-procedure.mdx")?;
    assert!(procedure.contains("VER-4.4"), "procedure must cite VER-4.4");
    assert!(
        procedure.contains("ONLY Velnor-owned pins and locks"),
        "refresh must stay Velnor-owned"
    );
    assert!(
        procedure.contains("recommendations only"),
        "tool files get recommendations only"
    );
    assert!(
        read("docs/content/docs/implemented/release-gates.mdx")?.contains("VER-4.4"),
        "human residual must be recorded"
    );
    Ok(())
}

#[test]
fn upstream_http_accepts_identity_and_single_gzip_and_requests_compression()
-> Result<(), Box<dyn Error>> {
    let plain = first_tool_body()?;
    let compressed = gzip_bytes(&plain)?;
    let gzip_response = response(
        vec![("Content-Encoding".to_owned(), "gzip".to_owned())],
        compressed,
    );
    let identity_response = response(
        vec![("Content-Encoding".to_owned(), "identity".to_owned())],
        plain,
    );
    let (output, request_headers) = run_fixture(Some(("/tool/0", gzip_response)))?;
    assert!(
        output.status.success(),
        "gzip probe failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("check-freshness: PASS"));
    assert_eq!(
        request_headers.len(),
        19,
        "all tool/action URLs should be probed"
    );
    assert!(
        request_headers
            .iter()
            .all(|header| header.eq_ignore_ascii_case("gzip, identity")),
        "probe did not consistently request supported compression: {request_headers:?}"
    );

    let (identity_output, identity_headers) = run_fixture(Some(("/tool/0", identity_response)))?;
    assert!(
        identity_output.status.success(),
        "identity response failed:\n{}\n{}",
        String::from_utf8_lossy(&identity_output.stdout),
        String::from_utf8_lossy(&identity_output.stderr)
    );
    assert_eq!(identity_headers.len(), 19);
    Ok(())
}

#[test]
fn upstream_http_rejects_unsupported_stacked_and_duplicate_codings() -> Result<(), Box<dyn Error>> {
    let body = first_tool_body()?;
    for (headers, expected) in [
        (
            vec![("Content-Encoding".to_owned(), "br".to_owned())],
            "unsupported Content-Encoding",
        ),
        (
            vec![("Content-Encoding".to_owned(), "gzip, identity".to_owned())],
            "unsupported Content-Encoding",
        ),
        (
            vec![
                ("Content-Encoding".to_owned(), "gzip".to_owned()),
                ("Content-Encoding".to_owned(), "identity".to_owned()),
            ],
            "unsupported Content-Encoding",
        ),
    ] {
        let (output, _) = run_fixture(Some(("/tool/0", response(headers, body.clone()))))?;
        assert_probe_rejected(&output, expected);
    }
    Ok(())
}

#[test]
fn upstream_http_rejects_malformed_and_truncated_gzip() -> Result<(), Box<dyn Error>> {
    let valid = gzip_bytes(&first_tool_body()?)?;
    let malformed = response(
        vec![("Content-Encoding".to_owned(), "gzip".to_owned())],
        b"not a gzip stream".to_vec(),
    );
    let (output, _) = run_fixture(Some(("/tool/0", malformed)))?;
    assert_probe_rejected(&output, "lookup_failed");

    let mut truncated_bytes = valid;
    truncated_bytes.truncate(truncated_bytes.len().saturating_sub(8));
    let truncated = response(
        vec![("Content-Encoding".to_owned(), "gzip".to_owned())],
        truncated_bytes,
    );
    let (output, _) = run_fixture(Some(("/tool/0", truncated)))?;
    assert_probe_rejected(&output, "lookup_failed");
    Ok(())
}

#[test]
fn upstream_http_enforces_encoded_and_decompressed_caps() -> Result<(), Box<dyn Error>> {
    let valid = first_tool_body()?;
    let mut exact_cap = valid.clone();
    assert!(exact_cap.len() < FETCH_CAP);
    exact_cap.resize(FETCH_CAP, b' ');
    let (output, _) = run_fixture(Some(("/tool/0", response(Vec::new(), exact_cap))))?;
    assert!(
        output.status.success(),
        "exact encoded limit should be accepted:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let mut encoded_overflow = valid.clone();
    encoded_overflow.resize(FETCH_CAP + 1, b' ');
    let (output, _) = run_fixture(Some(("/tool/0", response(Vec::new(), encoded_overflow))))?;
    assert_probe_rejected(&output, "encoded response exceeds 524288 bytes");

    let mut decoded_overflow = valid;
    decoded_overflow.resize(FETCH_CAP + 1, b' ');
    let (output, _) = run_fixture(Some((
        "/tool/0",
        response(
            vec![("Content-Encoding".to_owned(), "gzip".to_owned())],
            gzip_bytes(&decoded_overflow)?,
        ),
    )))?;
    assert_probe_rejected(&output, "decompressed response exceeds 524288 bytes");

    let mut high_entropy = Vec::with_capacity(FETCH_CAP + 1);
    let mut state = 0x9e37_79b9_u32;
    while high_entropy.len() < FETCH_CAP + 1 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        high_entropy.push(u8::try_from(state & 0xff)?);
    }
    let compressed_overflow = gzip_bytes(&high_entropy)?;
    assert!(compressed_overflow.len() > FETCH_CAP);
    let (output, _) = run_fixture(Some((
        "/tool/0",
        response(
            vec![("Content-Encoding".to_owned(), "gzip".to_owned())],
            compressed_overflow,
        ),
    )))?;
    assert_probe_rejected(&output, "encoded response exceeds 524288 bytes");
    Ok(())
}
