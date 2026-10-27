#!/usr/bin/env node
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const crypto = require("node:crypto");
const { spawnSync } = require("node:child_process");
const { TextDecoder } = require("node:util");

const MAX_LOCK_BYTES = 1024 * 1024;
const MISE_TIMEOUT_MS = 15000;
const MISE_OUTPUT_BYTES = 256 * 1024;
const EXPECTED = {
  jactionlint: {
    version: "2.3.0",
    backend: "github:jdx/jactionlint",
    installerBackend: "github:jdx/jactionlint",
    urls: {
      "linux-x64": "https://github.com/jdx/jactionlint/releases/download/v2.3.0/jactionlint_2.3.0_linux_amd64.tar.gz",
      "macos-arm64": "https://github.com/jdx/jactionlint/releases/download/v2.3.0/jactionlint_2.3.0_darwin_arm64.tar.gz",
    },
  },
  shellcheck: {
    version: "0.11.0",
    backend: "aqua:koalaman/shellcheck",
    installerBackend: "github:koalaman/shellcheck",
    urls: {
      "linux-x64": "https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.linux.x86_64.tar.xz",
      "macos-arm64": "https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.darwin.aarch64.tar.xz",
    },
  },
};

function reject() {
  throw new Error("validator lock rejected");
}

function readRegular(pathname) {
  const stat = fs.lstatSync(pathname);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > MAX_LOCK_BYTES) reject();
  const bytes = fs.readFileSync(pathname);
  if (bytes.length !== stat.size) reject();
  const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  if (/[\u0000-\u0009\u000b-\u001f\u007f]/.test(text)) reject();
  if (/\$\{\{|\{\{|\}\}|<%|%>/.test(text)) reject();
  return { bytes, text };
}

function configGet(mise, configPath, key) {
  const result = spawnSync(mise, [
    "--no-config", "--no-env", "--no-hooks", "config", "get", "-f", configPath, key,
  ], {
    encoding: "buffer",
    env: {
      PATH: process.env.PATH || "",
      HOME: process.env.HOME || "",
      MISE_NO_CONFIG: "1",
    },
    maxBuffer: MISE_OUTPUT_BYTES,
    shell: false,
    timeout: MISE_TIMEOUT_MS,
  });
  if (result.error || result.status !== 0 || result.signal !== null || result.stderr.length !== 0) reject();
  return result.stdout;
}

function runConfigGet(mise, configPath, key, expected) {
  const value = configGet(mise, configPath, key);
  if (!value.equals(Buffer.from(expected, "utf8"))) reject();
}

function projectArrayTable(text, tool) {
  const sourceHeader = `[[tools.${tool}]]`;
  const projectedHeader = `[tools.${tool}]`;
  const headerPattern = new RegExp(`^(\\s*)\\[\\[tools\\.${tool}\\]\\](\\s*(?:#.*)?)$`, "gm");
  const headers = [...text.matchAll(headerPattern)];
  if (headers.length !== 1 || text.split(sourceHeader).length !== 2) reject();
  const projected = text.replace(headerPattern, `$1${projectedHeader}$2`);
  if (projected === text) reject();
  return projected;
}

function getTool(mise, snapshotPath, projectionPath, tool, platform, configuredVersion) {
  const expected = EXPECTED[tool];
  if (!expected || !Object.hasOwn(expected.urls, platform) || configuredVersion !== expected.version) reject();
  runConfigGet(mise, snapshotPath, `tools.${tool}`, "[{...}]\n");
  runConfigGet(mise, projectionPath, `tools.${tool}.version`, `${expected.version}\n`);
  runConfigGet(mise, projectionPath, `tools.${tool}.backend`, `${expected.backend}\n`);
  const platformKey = `tools.${tool}.\"platforms.${platform}\"`;
  const url = expected.urls[platform];
  runConfigGet(mise, projectionPath, `${platformKey}.url`, `${url}\n`);
  const checksumKey = `${platformKey}.checksum`;
  const checksum = getNativeChecksum(mise, projectionPath, tool, checksumKey);
  const spec = `${tool}[platform_${platform.replaceAll("-", "_")}_url=${url},checksum=${checksum}]@${expected.version}`;
  return {
    tool,
    version: expected.version,
    root_backend: expected.backend,
    installer_backend: expected.installerBackend,
    platform,
    url,
    checksum,
    mise_spec: spec,
    binary_sha256: null,
  };
}

function getNativeChecksum(mise, projectionPath, tool, key) {
  if (!EXPECTED[tool]) reject();
  const value = new TextDecoder("utf-8", { fatal: true }).decode(configGet(mise, projectionPath, key));
  const match = value.match(/^sha256:([0-9a-f]{64})\n$/);
  if (!match || !EXPECTED[tool]) reject();
  return `sha256:${match[1]}`;
}

function writeReceipt(pathname, bytes) {
  const descriptor = fs.openSync(pathname, "wx", 0o600);
  let created;
  let closed = false;
  try {
    created = fs.fstatSync(descriptor);
    fs.writeFileSync(descriptor, bytes);
    fs.fsyncSync(descriptor);
    fs.closeSync(descriptor);
    closed = true;
    return { dev: created.dev, ino: created.ino };
  } catch (error) {
    if (!closed) {
      try {
        fs.closeSync(descriptor);
      } catch {}
    }
    try {
      const current = fs.lstatSync(pathname);
      if (!created || (current.dev === created.dev && current.ino === created.ino)) fs.unlinkSync(pathname);
    } catch {}
    throw error;
  }
}

function main() {
  if (process.argv.length !== 8 || process.argv[2] !== "plan") {
    process.stderr.write("validator tool lock rejected\n");
    process.exitCode = 1;
    return;
  }
  const [, , , lockPath, platform, jactionlintVersion, shellcheckVersion, receiptPath] = process.argv;
  let privateDirectory;
  let receiptIdentity;
  try {
    const { bytes, text } = readRegular(lockPath);
    if (!Object.hasOwn(EXPECTED.jactionlint.urls, platform)) reject();
    privateDirectory = fs.mkdtempSync(path.join(os.tmpdir(), "velnor-validator-lock-"));
    fs.chmodSync(privateDirectory, 0o700);
    const snapshotPath = path.join(privateDirectory, "mise.lock");
    fs.writeFileSync(snapshotPath, bytes, { flag: "wx", mode: 0o600 });
    const projection = projectArrayTable(text, "jactionlint");
    const projectionBoth = projectArrayTable(projection, "shellcheck");
    const projectionPath = path.join(privateDirectory, "mise.toml");
    fs.writeFileSync(projectionPath, projectionBoth, { flag: "wx", mode: 0o600 });
    const mise = process.env.MISE_BIN || "mise";
    const entries = [
      getTool(mise, snapshotPath, projectionPath, "jactionlint", platform, jactionlintVersion),
      getTool(mise, snapshotPath, projectionPath, "shellcheck", platform, shellcheckVersion),
    ];
    const projectionSha256 = crypto.createHash("sha256").update(projectionBoth, "utf8").digest("hex");
    const canonicalValues = entries.map(({ tool, version, root_backend, platform: selectedPlatform, url, checksum }) => ({
      tool,
      version,
      backend: root_backend,
      platform: selectedPlatform,
      url,
      checksum,
    }));
    const canonicalSha256 = crypto.createHash("sha256").update(JSON.stringify(canonicalValues), "utf8").digest("hex");
    const receipt = {
      lock_sha256: crypto.createHash("sha256").update(bytes).digest("hex"),
      canonical_sha256: canonicalSha256,
      projection_sha256: projectionSha256,
      platform,
      tools: entries,
    };
    const specs = entries.map((entry) => entry.mise_spec).join("\n") + "\n";
    const receiptBytes = Buffer.from(`${JSON.stringify(receipt, null, 2)}\n`, "utf8");
    fs.rmSync(privateDirectory, { recursive: true, force: true });
    privateDirectory = undefined;
    receiptIdentity = writeReceipt(receiptPath, receiptBytes);
    process.stdout.write(specs);
  } catch {
    process.stderr.write("validator tool lock rejected\n");
    process.exitCode = 1;
  } finally {
    if (privateDirectory) {
      try {
        fs.rmSync(privateDirectory, { recursive: true, force: true });
      } catch {
        process.exitCode = 1;
      }
    }
    if (process.exitCode && process.exitCode !== 0 && receiptIdentity) {
      try {
        const current = fs.lstatSync(receiptPath);
        if (current.dev === receiptIdentity.dev && current.ino === receiptIdentity.ino) fs.unlinkSync(receiptPath);
      } catch {}
    }
  }
}

main();
