#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

assert.equal(process.versions.bun, Bun.version, "validator tests must run with Bun");
assert.ok(Bun.semver.satisfies(Bun.version, ">=1.4.4"), "validator tests require Bun 1.4.4 or newer");

const [helper, lockPath] = process.argv.slice(2);
if (!helper || !lockPath || process.argv.length !== 4) {
  process.stderr.write("usage: test-qualification-validator-tools.cjs HELPER LOCK\n");
  process.exit(64);
}

const original = fs.readFileSync(lockPath);
const originalSha = crypto.createHash("sha256").update(original).digest("hex");
const root = fs.mkdtempSync(path.join(os.tmpdir(), "velnor-validator-tools-test-"));
fs.chmodSync(root, 0o700);
const mise = process.env.MISE_BIN || "mise";
const options = {
  env: { ...process.env, MISE_BIN: mise },
  encoding: "utf8",
  maxBuffer: 256 * 1024,
  timeout: 30000,
};

function run(bytes, platform, name, versions = ["2.3.0", "0.11.0"]) {
  const fixture = path.join(root, `${name}.toml`);
  const receipt = path.join(root, `${name}.json`);
  fs.writeFileSync(fixture, bytes, { flag: "wx", mode: 0o600 });
  const result = spawnSync(process.execPath, [helper, "plan", fixture, platform, ...versions, receipt], options);
  return { fixture, receipt, result };
}

function reject(bytes, platform, name, versions = ["2.3.0", "0.11.0"]) {
  const { receipt, result } = run(bytes, platform, name, versions);
  assert.notEqual(result.status, 0, `${name} must fail closed`);
  assert.equal(result.stdout, "", `${name} must not emit install specs`);
  assert.equal(result.stderr, "validator tool lock rejected\n");
  assert.equal(fs.existsSync(receipt), false, `${name} must not create a success receipt`);
}

function replaceOnce(text, before, after) {
  assert.equal(text.split(before).length, 2, `fixture anchor must be unique: ${before}`);
  return text.replace(before, after);
}

function replaceBlockLine(text, header, before, after) {
  const start = text.indexOf(header);
  assert.notEqual(start, -1, `table anchor must exist: ${header}`);
  assert.equal(text.indexOf(header, start + header.length), -1, `table anchor must be unique: ${header}`);
  const next = text.slice(start + header.length).search(/\n\[/);
  const end = next < 0 ? text.length : start + header.length + next + 1;
  const block = text.slice(start, end);
  assert.equal(block.split(before).length, 2, `field anchor must be unique: ${before}`);
  return `${text.slice(0, start)}${block.replace(before, after)}${text.slice(end)}`;
}

function removeBlock(text, header) {
  const start = text.indexOf(header);
  assert.notEqual(start, -1, `table anchor must exist: ${header}`);
  assert.equal(text.indexOf(header, start + header.length), -1, `table anchor must be unique: ${header}`);
  const next = text.slice(start + header.length).search(/\n\[/);
  const end = next < 0 ? text.length : start + header.length + next + 1;
  return `${text.slice(0, start)}${text.slice(end)}`;
}

try {
  const source = new TextDecoder("utf-8", { fatal: true }).decode(original);
  const url = {
    jactionlint: "https://github.com/jdx/jactionlint/releases/download/v2.3.0/jactionlint_2.3.0_linux_amd64.tar.gz",
    shellcheck: "https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.linux.x86_64.tar.xz",
  };

  function positive(bytes, platform, name) {
    const { receipt, result } = run(bytes, platform, name);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, "");
    assert.equal(result.stdout.endsWith("\n"), true);
    const specs = result.stdout.slice(0, -1).split("\n");
    assert.equal(specs.length, 2);
    assert.match(specs[0], /^jactionlint\[platform_(linux_x64|macos_arm64)_url=https:\/\/github\.com\/jdx\/jactionlint\/releases\/download\/v2\.3\.0\/.+,checksum=sha256:[0-9a-f]{64}\]@2\.3\.0$/);
    assert.match(specs[1], /^shellcheck\[platform_(linux_x64|macos_arm64)_url=https:\/\/github\.com\/koalaman\/shellcheck\/releases\/download\/v0\.11\.0\/.+,checksum=sha256:[0-9a-f]{64}\]@0\.11\.0$/);
    const parsed = JSON.parse(fs.readFileSync(receipt, "utf8"));
    assert.equal(parsed.lock_sha256, crypto.createHash("sha256").update(bytes).digest("hex"));
    assert.equal(parsed.tools.every(({ platform: selectedPlatform }) => selectedPlatform === platform), true);
    const canonical = parsed.tools.map(({ tool, version, root_backend, platform: selectedPlatform, url: toolUrl, checksum }) => ({
      tool,
      version,
      backend: root_backend,
      platform: selectedPlatform,
      url: toolUrl,
      checksum,
    }));
    assert.equal(parsed.canonical_sha256, crypto.createHash("sha256").update(JSON.stringify(canonical)).digest("hex"));
    assert.match(parsed.projection_sha256, /^[0-9a-f]{64}$/);
    assert.deepEqual(parsed.tools.map(({ mise_spec }) => mise_spec), specs);
    assert.deepEqual(parsed.tools.map(({ root_backend, installer_backend }) => [root_backend, installer_backend]), [
      ["github:jdx/jactionlint", "github:jdx/jactionlint"],
      ["aqua:koalaman/shellcheck", "github:koalaman/shellcheck"],
    ]);
    assert.equal(parsed.tools[0].url, platform === "linux-x64"
      ? url.jactionlint
      : "https://github.com/jdx/jactionlint/releases/download/v2.3.0/jactionlint_2.3.0_darwin_arm64.tar.gz");
    assert.equal(parsed.tools[1].url, platform === "linux-x64"
      ? url.shellcheck
      : "https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.darwin.aarch64.tar.xz");
  }

  positive(original, "linux-x64", "positive-linux");
  positive(original, "macos-arm64", "positive-macos");

  const commentHeader = replaceOnce(source, "[[tools.jactionlint]]\n", "[[tools.jactionlint]] # valid TOML comment\n");
  positive(Buffer.from(commentHeader), "linux-x64", "positive-comment-header");

  const singleQuotedKeys = replaceOnce(source, 'version = "2.3.0"\n', "'version' = \"2.3.0\"\n");
  const singleQuotedBoth = replaceOnce(singleQuotedKeys, 'backend = "github:jdx/jactionlint"\n', "'backend' = \"github:jdx/jactionlint\"\n");
  positive(Buffer.from(singleQuotedBoth), "linux-x64", "positive-single-quoted-keys");

  const jactionlintHeader = '[[tools.jactionlint]]\n';
  const jactionlintPlatformHeader = '[tools.jactionlint."platforms.linux-x64"]\n';
  const linuxJactionlint = source.match(/\[tools\.jactionlint\."platforms\.linux-x64"\]\n([\s\S]*?)(?=\n\[|$)/)[1];
  const linuxChecksum = linuxJactionlint.match(/^checksum = "sha256:([0-9a-f]{64})"$/m)[1];
  const checksumLine = `checksum = "sha256:${linuxChecksum}"`;
  const jactionlintUrlLine = `url = "${url.jactionlint}"`;
  const jactionlintUrlHeader = `${jactionlintPlatformHeader}${checksumLine}\n${jactionlintUrlLine}`;

  reject(Buffer.from(replaceOnce(source, `${jactionlintHeader}version = "2.3.0"\n`, `${jactionlintHeader}version = "2.3.0"\n"version" = "9.9.9"\n`)), "linux-x64", "duplicate-quoted-version");
  reject(Buffer.from(replaceOnce(source, `${jactionlintHeader}version = "2.3.0"\n`, `${jactionlintHeader}version = "2.3.0"\n'version' = "9.9.9"\n`)), "linux-x64", "duplicate-single-quoted-version");
  reject(Buffer.from(`${source}\n[[tools.jactionlint]] # duplicate\nversion = "9.9.9"\nbackend = "github:attacker/tool"\n`), "linux-x64", "duplicate-commented-table");

  reject(Buffer.from(replaceOnce(source, `${jactionlintPlatformHeader}checksum =`, `${jactionlintPlatformHeader}url = "https://attacker.invalid/tool.tar.gz" # duplicate\nchecksum =`)), "linux-x64", "duplicate-url-inline-comment");
  reject(Buffer.from(replaceOnce(source, `${jactionlintHeader}version = "2.3.0"\n`, `${jactionlintHeader}version = "2.3.0"\nversion = "9.9.9" # duplicate\n`)), "linux-x64", "duplicate-version-inline-comment");
  reject(Buffer.from(source.replace('backend = "aqua:koalaman/shellcheck"', 'backend = "core:asdf"')), "linux-x64", "wrong-backend");
  reject(original, "linux-x64", "wrong-configured-version", ["2.3.1", "0.11.0"]);
  reject(original, "windows-x64", "unsupported-platform");
  reject(Buffer.from(replaceOnce(source, jactionlintUrlHeader, `${jactionlintPlatformHeader}checksum = "sha256:bad"\n${jactionlintUrlLine}`)), "linux-x64", "malformed-checksum");
  reject(Buffer.from(replaceOnce(source, `${jactionlintPlatformHeader}${checksumLine}\n`, jactionlintPlatformHeader)), "linux-x64", "missing-checksum");
  reject(Buffer.from(`${source}\n${jactionlintPlatformHeader}checksum = "sha256:${"0".repeat(64)}"\n${jactionlintUrlLine}\n`), "linux-x64", "duplicate-platform");
  reject(Buffer.from(removeBlock(source, jactionlintPlatformHeader)), "linux-x64", "missing-platform");

  const template = replaceBlockLine(source, jactionlintPlatformHeader, jactionlintUrlLine, 'url = "{{exec(\'bad\')}}"');
  const marker = path.join(root, "template-marker");
  reject(Buffer.from(template.replace("{{exec('bad')}}", `{{exec('touch ${marker}')}}`)), "linux-x64", "template-marker");
  assert.equal(fs.existsSync(marker), false);
  const fakeHeader = `${source}\nprobe = """\n[[tools.jactionlint]]\n"""\n`;
  reject(Buffer.from(fakeHeader), "linux-x64", "multiline-fake-header");

  const optionDelimiter = replaceBlockLine(source, jactionlintPlatformHeader, jactionlintUrlLine, `url = "${url.jactionlint},checksum=sha256:${"0".repeat(64)}"`);
  reject(Buffer.from(optionDelimiter), "linux-x64", "option-delimiter");
  reject(Buffer.concat([original, Buffer.from([0])]), "linux-x64", "control-delimiter");
  reject(Buffer.concat([original, Buffer.from([0xff])]), "linux-x64", "invalid-utf8");
  reject(Buffer.alloc(1024 * 1024 + 1, 0x20), "linux-x64", "oversized-lock");

  const symlink = path.join(root, "linked.toml");
  fs.symlinkSync(path.resolve(lockPath), symlink);
  const linkedReceipt = path.join(root, "linked.json");
  const linked = spawnSync(process.execPath, [helper, "plan", symlink, "linux-x64", "2.3.0", "0.11.0", linkedReceipt], options);
  assert.notEqual(linked.status, 0);
  assert.equal(linked.stdout, "");
  assert.equal(linked.stderr, "validator tool lock rejected\n");
  assert.equal(fs.existsSync(linkedReceipt), false);

  assert.equal(crypto.createHash("sha256").update(fs.readFileSync(lockPath)).digest("hex"), originalSha);
  process.stdout.write("qualification validator tool native TOML admission tests passed\n");
} finally {
  fs.rmSync(root, { recursive: true, force: true });
}
