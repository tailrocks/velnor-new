"use strict";

const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const net = require("node:net");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");
const { TextDecoder } = require("node:util");

const fixtureBody = Buffer.from("Velnor verifier TLS retry fixture\n", "utf8");
const fixtureSha256 = "00433f81fee55442585d44ef1bf65ce3d3391ad5eeec05e887b0efbfd7db0e88";
const caseTimeoutMs = 45_000;
const wholeTimeoutMs = 240_000;

function requireMatch(text, pattern, label) {
  const matches = [...text.matchAll(pattern)];
  assert.equal(matches.length, 1, `${label}: expected one match, found ${matches.length}`);
  return matches[0];
}

function readGeneratedVerifier(actionPath) {
  assert.ok(path.isAbsolute(actionPath), "generated action path must be absolute");
  const actionInfo = fs.lstatSync(actionPath);
  assert.ok(actionInfo.isFile() && !actionInfo.isSymbolicLink(), "generated action must be a regular file");
  const actionBytes = fs.readFileSync(actionPath);
  const actionText = new TextDecoder("utf-8", { fatal: true }).decode(actionBytes);
  const lines = actionText.split("\n");
  const stepIndexes = lines.flatMap((line, index) =>
    line.trim() === "- name: Verify restored Rust toolchain" ? [index] : []);
  assert.equal(stepIndexes.length, 1, "generated restored Rust verifier step must be unique");
  const start = stepIndexes[0];
  const nextStep = lines.findIndex((line, index) => index > start && line.startsWith("    - name:"));
  const section = lines.slice(start, nextStep < 0 ? lines.length : nextStep);
  const runLines = section.filter((line) => /^      run: ".*"$/.test(line));
  assert.equal(runLines.length, 1, "generated restored Rust verifier must have one JSON-quoted run value");
  const quotedRun = runLines[0].trim().slice("run: ".length);
  const run = JSON.parse(quotedRun);

  const getMatch = requireMatch(run, /get\(\) \{ curl [^}]*; \}/g, "generated curl get function");
  const getFunction = getMatch[0];
  const betweenGetAndDigest = run.slice(getMatch.index + getFunction.length,
    requireMatch(run, /printf "%s  %s\\n" "\$want" "\$d\/channel\.toml" > "\$d\/channel\.sha256"; \$h -c "\$d\/channel\.sha256" >\/dev\/null \|\| fail "[^"]+"/g,
      "generated channel SHA-256 gate").index);
  assert.match(betweenGetAndDigest,
    /^; get "\$d\/channel\.toml" "https:\/\/static\.rust-lang\.org\/dist\/channel-rust-\$v\.toml";\s*$/,
    "generated channel SHA-256 gate must immediately follow the same channel download");
  const digestMatch = requireMatch(run,
    /printf "%s  %s\\n" "\$want" "\$d\/channel\.toml" > "\$d\/channel\.sha256"; \$h -c "\$d\/channel\.sha256" >\/dev\/null \|\| fail "[^"]+"/g,
    "generated channel SHA-256 gate");

  return {
    actionSha256: crypto.createHash("sha256").update(actionBytes).digest("hex"),
    actionPath,
    digestCheck: digestMatch[0],
    getFunction,
    getFunctionSha256: crypto.createHash("sha256").update(getFunction).digest("hex"),
  };
}

function executableVersion(executable, args) {
  const result = spawnSync(executable, args, {
    encoding: "utf8",
    env: { PATH: "/usr/bin:/bin" },
    timeout: 10_000,
  });
  assert.equal(result.error, undefined, `${executable} preflight: ${result.error?.message}`);
  assert.equal(result.status, 0, `${executable} preflight failed: ${result.stderr}`);
  return `${result.stdout}${result.stderr}`.trim().split("\n")[0];
}

function closeServer(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => error ? reject(error) : resolve());
  });
}

async function waitForProcessClose(promise) {
  await promise;
}

function signalProcessGroup(child, signal) {
  if (!child || !Number.isInteger(child.pid)) return;
  try {
    process.kill(-child.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

async function processGroupHasExited(processGroupId, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      process.kill(-processGroupId, 0);
    } catch (error) {
      if (error.code === "ESRCH") return true;
      throw error;
    }
    if (Date.now() >= deadline) return false;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

async function runCase({
  area, caPath, deadlineAt, digestCheck, expectTimeout = false, getFunction,
  mode, name, timeoutMs = null, trustCertificate,
}) {
  const liveSockets = new Set();
  const upstreamSockets = new Set();
  const serverTimers = new Set();
  let connectionCount = 0;
  const tlsServer = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    tls: {
      key: Bun.file(path.join(area, "key.pem")),
      cert: Bun.file(path.join(area, "cert.pem")),
    },
    fetch() {
      const body = mode === "wrong-body" ? Buffer.from("wrong channel bytes\n") : fixtureBody;
      const headers = {
        "content-length": String(body.length),
        "content-type": "text/plain",
        connection: "close",
      };
      if (mode === "slow-body") {
        let responseTimer;
        const slowBody = new ReadableStream({
          start(controller) {
            responseTimer = setTimeout(() => {
              serverTimers.delete(responseTimer);
              controller.enqueue(body);
              controller.close();
            }, 10_000);
            serverTimers.add(responseTimer);
          },
          cancel() {
            if (responseTimer) clearTimeout(responseTimer);
            serverTimers.delete(responseTimer);
          },
        });
        return new Response(slowBody, { status: 200, headers });
      }
      return new Response(body, { status: 200, headers });
    },
  });
  const tcpProxy = net.createServer((socket) => {
    connectionCount += 1;
    liveSockets.add(socket);
    socket.on("close", () => liveSockets.delete(socket));
    if (mode === "persistent-eof" || (mode === "transient-eof" && connectionCount === 1)) {
      socket.destroy();
      return;
    }
    const upstream = net.connect(tlsServer.port, "127.0.0.1");
    upstreamSockets.add(upstream);
    upstream.on("close", () => upstreamSockets.delete(upstream));
    socket.once("error", () => upstream.destroy());
    upstream.once("error", () => socket.destroy());
    socket.pipe(upstream);
    upstream.pipe(socket);
  });
  const startedAt = Date.now();
  let child;
  let timer;
  let childClosePromise;
  let result;
  let timedOut = false;
  let processGroupExited = false;
  let outputPath;
  try {
    await new Promise((resolve, reject) => {
      tcpProxy.once("error", reject);
      tcpProxy.listen(0, "127.0.0.1", resolve);
    });

    const channelDirectory = path.join(area, name);
    fs.mkdirSync(channelDirectory);
    outputPath = path.join(channelDirectory, "channel.toml");
    const requestUrl = `https://localhost:${tcpProxy.address().port}/channel.toml`;
    const shellScript = [
      "set -euo pipefail",
      getFunction,
      'out="$1"; url="$2"; want="$3"; d="$4"; v=fixture; fail() { printf "%s\\n" "$1" >&2; return 1; }',
      'if command -v sha256sum >/dev/null 2>&1; then h=sha256sum; else h="shasum -a 256"; fi',
      'get "$out" "$url"',
      digestCheck,
    ].join("\n");
    const env = {
      CURL_HOME: area,
      HOME: area,
      NO_PROXY: "localhost,127.0.0.1",
      PATH: "/usr/bin:/bin",
      no_proxy: "localhost,127.0.0.1",
    };
    if (trustCertificate) env.CURL_CA_BUNDLE = caPath;
    const remainingMs = timeoutMs ?? Math.min(caseTimeoutMs, deadlineAt - Date.now());
    assert.ok(remainingMs > 0, "whole TLS retry suite exceeded its deadline");
    result = await new Promise((resolve, reject) => {
      child = spawn("/bin/bash", [
        "--noprofile", "--norc", "-euo", "pipefail", "-c", shellScript,
        "velnor-rust-verifier-retry-fixture", outputPath, requestUrl, fixtureSha256, channelDirectory,
      ], { detached: true, env, stdio: ["ignore", "pipe", "pipe"] });
      let stdout = "";
      let stderr = "";
      child.stdout.on("data", (chunk) => { stdout = (stdout + chunk).slice(-32_768); });
      child.stderr.on("data", (chunk) => { stderr = (stderr + chunk).slice(-32_768); });
      timer = setTimeout(() => {
        timedOut = true;
        try {
          signalProcessGroup(child, "SIGKILL");
        } catch (error) {
          reject(error);
        }
      }, remainingMs);
      child.once("error", reject);
      childClosePromise = new Promise((resolveClose) => {
        child.once("close", (code, signal) =>
          resolveClose({ code, signal, stdout, stderr }));
      });
      childClosePromise.then(resolve);
    });
  } finally {
    if (timer) clearTimeout(timer);
    let processCleanupError;
    if (child) {
      try {
        signalProcessGroup(child, "SIGKILL");
        if (childClosePromise) {
          await waitForProcessClose(childClosePromise);
        }
        if (Number.isInteger(child.pid)) {
          processGroupExited = await processGroupHasExited(child.pid, 2_000);
        }
      } catch (error) {
        processCleanupError = error;
      }
    }
    for (const timerId of serverTimers) clearTimeout(timerId);
    for (const socket of liveSockets) socket.destroy();
    for (const socket of upstreamSockets) socket.destroy();
    if (tcpProxy.listening) await closeServer(tcpProxy);
    await tlsServer.stop(true);
    if (processCleanupError) throw processCleanupError;
  }

  const elapsedMs = Date.now() - startedAt;
  const outputDigest = outputPath && fs.existsSync(outputPath)
    ? crypto.createHash("sha256").update(fs.readFileSync(outputPath)).digest("hex")
    : null;
  const check = {
    code: result.code,
    connections: connectionCount,
    elapsedMs,
    name,
    outputSha256: outputDigest,
    processGroupExited,
    signal: result.signal,
  };
  try {
    if (expectTimeout) {
      assert.equal(timedOut, true, `${name}: timeout did not fire`);
      assert.equal(result.signal, "SIGKILL", `${name}: process group did not receive SIGKILL`);
      assert.equal(connectionCount, 1, `${name}: slow TLS response was not reached`);
      assert.equal(processGroupExited, true, `${name}: owned process group remained after cleanup`);
      assert.ok(elapsedMs < (timeoutMs ?? 0) + 5_000,
        `${name}: process cleanup exceeded bounded deadline`);
    } else if (name === "timeout-kills-owned-process-group") {
      assert.fail(`${name}: expected timeout lifecycle branch`);
    } else if (name === "transient-eof-recovers") {
      assert.equal(result.signal, null, `${name}: terminated by signal ${result.signal}`);
      assert.equal(processGroupExited, true, `${name}: owned process group remained after completion`);
      assert.equal(result.code, 0, `${name}: expected successful digest check, got ${result.code}: ${result.stderr}`);
      assert.equal(connectionCount, 2, `${name}: expected first EOF then successful TLS retry`);
      assert.equal(outputDigest, fixtureSha256, `${name}: downloaded bytes differ from fixture`);
    } else if (name === "persistent-eof-is-bounded") {
      assert.equal(result.signal, null, `${name}: terminated by signal ${result.signal}`);
      assert.equal(processGroupExited, true, `${name}: owned process group remained after completion`);
      assert.notEqual(result.code, 0, `${name}: persistent EOF unexpectedly succeeded`);
      assert.equal(connectionCount, 4, `${name}: expected initial attempt plus three retries`);
      assert.ok(elapsedMs < caseTimeoutMs, `${name}: exceeded bounded case deadline`);
    } else if (name === "normal-body-digest-passes") {
      assert.equal(result.signal, null, `${name}: terminated by signal ${result.signal}`);
      assert.equal(processGroupExited, true, `${name}: owned process group remained after completion`);
      assert.equal(result.code, 0, `${name}: expected successful digest check, got ${result.code}: ${result.stderr}`);
      assert.equal(connectionCount, 1, `${name}: expected one TLS connection`);
      assert.equal(outputDigest, fixtureSha256, `${name}: downloaded bytes differ from fixture`);
    } else if (name === "wrong-body-digest-rejects") {
      assert.equal(result.signal, null, `${name}: terminated by signal ${result.signal}`);
      assert.equal(processGroupExited, true, `${name}: owned process group remained after completion`);
      assert.notEqual(result.code, 0, `${name}: wrong channel bytes unexpectedly passed digest check`);
      assert.equal(connectionCount, 1, `${name}: expected one TLS connection`);
      assert.notEqual(outputDigest, fixtureSha256, `${name}: wrong-body fixture unexpectedly matches digest`);
    } else if (name === "bad-certificate-tls-rejects") {
      assert.equal(result.signal, null, `${name}: terminated by signal ${result.signal}`);
      assert.equal(processGroupExited, true, `${name}: owned process group remained after completion`);
      assert.notEqual(result.code, 0, `${name}: untrusted certificate unexpectedly succeeded`);
      assert.ok(connectionCount >= 1 && connectionCount <= 4,
        `${name}: retry count outside bounded curl attempts (${connectionCount})`);
    } else {
      assert.fail(`unexpected test case ${name}`);
    }
    check.passed = true;
  } catch (error) {
    check.passed = false;
    check.failure = error.message;
  }
  check.stderr = result.stderr.trim().slice(-2048);
  return check;
}

async function main() {
  assert.equal(process.versions.bun, Bun.version, "retry tests must run with Bun");
  assert.ok(Bun.semver.satisfies(Bun.version, ">=1.4.4"), `expected Bun 1.4.4 or newer, found ${Bun.version}`);
  assert.equal(process.argv.length, 4, "usage: test-rust-verifier-retries.cjs <absolute-generated-action.yml> <absolute-receipt.json>");
  const [, , actionPath, receiptPath] = process.argv;
  assert.ok(path.isAbsolute(receiptPath), "receipt path must be absolute");
  assert.notEqual(actionPath, receiptPath, "action and receipt paths must differ");
  const generated = readGeneratedVerifier(actionPath);
  const fixtureActualSha256 = crypto.createHash("sha256").update(fixtureBody).digest("hex");
  assert.equal(fixtureActualSha256, fixtureSha256, "controlled fixture SHA-256 changed");
  const curlPath = spawnSync("/bin/sh", ["-c", "command -v curl"], {
    encoding: "utf8",
    env: { PATH: "/usr/bin:/bin" },
    timeout: 10_000,
  });
  assert.equal(curlPath.status, 0, `curl preflight failed: ${curlPath.stderr}`);
  assert.equal(curlPath.stdout.trim(), "/usr/bin/curl", `expected /usr/bin/curl, found ${curlPath.stdout.trim()}`);
  const curlVersion = executableVersion("/usr/bin/curl", ["--version"]);
  const opensslVersion = executableVersion("/usr/bin/openssl", ["version"]);
  const shasumVersion = executableVersion("/usr/bin/shasum", ["--version"]);
  const area = fs.mkdtempSync(path.join(os.tmpdir(), "velnor-rust-verifier-retry-"));
  let receipt;
  let receiptWritten = false;
  const startedAt = Date.now();
  const deadlineAt = startedAt + wholeTimeoutMs;
  const results = [];
  try {
    const keyPath = path.join(area, "key.pem");
    const caPath = path.join(area, "cert.pem");
    const openssl = spawnSync("/usr/bin/openssl", [
      "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", keyPath,
      "-out", caPath, "-days", "1", "-subj", "/CN=localhost",
      "-addext", "subjectAltName=DNS:localhost",
    ], { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 20_000 });
    assert.equal(openssl.error, undefined, `OpenSSL certificate generation failed: ${openssl.error?.message}`);
    assert.equal(openssl.status, 0, `OpenSSL certificate generation failed: ${openssl.stderr}`);
    const cases = [
      { mode: "transient-eof", name: "transient-eof-recovers", trustCertificate: true },
      { mode: "persistent-eof", name: "persistent-eof-is-bounded", trustCertificate: true },
      { mode: "normal", name: "normal-body-digest-passes", trustCertificate: true },
      { mode: "wrong-body", name: "wrong-body-digest-rejects", trustCertificate: true },
      { mode: "normal", name: "bad-certificate-tls-rejects", trustCertificate: false },
    ];
    for (const testCase of cases) {
      assert.ok(Date.now() < deadlineAt, "whole TLS retry suite exceeded its deadline");
      results.push(await runCase({ area, caPath, deadlineAt, ...generated, ...testCase }));
    }
    const lifecycleProbe = await runCase({
      area,
      caPath,
      deadlineAt,
      ...generated,
      expectTimeout: true,
      mode: "slow-body",
      name: "timeout-kills-owned-process-group",
      timeoutMs: 1_000,
      trustCertificate: true,
    });
    receipt = {
      action_path: generated.actionPath,
      action_sha256: generated.actionSha256,
      curl: curlVersion,
      digest_gate_sha256: crypto.createHash("sha256").update(generated.digestCheck).digest("hex"),
      fixture_sha256: fixtureSha256,
      get_function_sha256: generated.getFunctionSha256,
      bun: Bun.version,
      openssl: opensslVersion,
      process_group_cleanup: lifecycleProbe,
      results,
      scope: "generated restored-Rust verifier download bytes and adjacent channel digest gate; not full verifier or Rust toolchain validation",
      shasum: shasumVersion,
      total_elapsed_ms: Date.now() - startedAt,
    };
    fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
    fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx", mode: 0o600 });
    receiptWritten = true;
    process.stdout.write(`${JSON.stringify(receipt)}\n`);
    const failures = results.filter((result) => !result.passed);
    if (!lifecycleProbe.passed) failures.push(lifecycleProbe);
    if (failures.length > 0) {
      throw new Error(`${failures.length} TLS retry regression case(s) failed: ${failures.map((result) => result.name).join(", ")}`);
    }
  } catch (error) {
    if (!receiptWritten) {
      receipt = {
        action_path: generated.actionPath,
        action_sha256: generated.actionSha256,
        error: error.message,
        bun: Bun.version,
        results,
        scope: "generated restored-Rust verifier download bytes and adjacent channel digest gate; not full verifier or Rust toolchain validation",
      };
      fs.mkdirSync(path.dirname(receiptPath), { recursive: true });
      fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`, { flag: "wx", mode: 0o600 });
      process.stderr.write(`${JSON.stringify(receipt)}\n`);
    }
    throw error;
  } finally {
    fs.rmSync(area, { recursive: true, force: true });
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack ?? error}\n`);
  process.exitCode = 1;
});
