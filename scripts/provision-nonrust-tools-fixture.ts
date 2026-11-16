// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

"use strict";

import { appendFile, chmod, mkdir, realpath, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { transport } from "./nonrust-tools-transport";

const root = resolve(import.meta.dir, "..");
const originToken = process.env.VELNOR_FIXTURE_ORIGIN_TOKEN;
delete process.env.VELNOR_FIXTURE_ORIGIN_TOKEN;
const codecTests = Bun.spawn([process.execPath, "test", resolve(import.meta.dir, "nonrust-bundle-snappy.test.ts")], { stdout: "inherit", stderr: "inherit" });
if (await codecTests.exited !== 0) throw new Error("fixture bundle decoder checks failed");
const destination = process.argv[2];
const executable = process.argv[3] ?? Bun.which("mise");
if (!destination || !executable) throw new Error("fixture directory and pinned Mise executable are required");
const fixture = resolve(destination);
if (/[\r\n]/.test(fixture)) throw new Error("fixture path contains a line ending");
const mise = await realpath(executable);
const catalog = await Bun.file(resolve(root, "crates/velnor-actions-mise/src/catalog.rs")).text();
const identities = await Bun.file(resolve(root, "crates/velnor-actions-mise/src/catalog_mise_binary.rs")).text();
const workflowTool = await Bun.file(resolve(root, "crates/velnor-actions-contract/src/config/jactionlint_verifier.rs")).text();
const constant = (name: string): string => {
  const value = new RegExp(`${name}: &str =\\s*"([^"]+)"`).exec(`${catalog}\n${identities}\n${workflowTool}`)?.[1];
  if (!value) throw new Error(`missing catalogue identity: ${name}`);
  return value;
};
const platform = process.platform === "darwin" ? "macos" : process.platform;
const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x64" : "unsupported";
const host = `${platform}-${architecture}`;
if (Bun.version !== constant("BUN_VERSION")) throw new Error("fixture requires the repository's pinned Bun version");
const expected = constant(`MISE_BINARY_SHA256_${platform.toUpperCase()}_${architecture.toUpperCase()}`);
const digest = async (path: string): Promise<string> => {
  const hash = new Bun.CryptoHasher("sha256");
  for await (const chunk of Bun.file(path).stream()) hash.update(chunk);
  return hash.digest("hex");
};
if (await digest(mise) !== expected) throw new Error("fixture Mise binary differs from pinned authority");
const home = resolve(fixture, "home");
await mkdir(home, { recursive: true });
await chmod(fixture, 0o700);
const origin = await transport(fixture, "record", originToken);
const environment = { HOME: home, PATH: "/usr/bin:/bin:/usr/sbin:/sbin", MISE_LOCKFILE: "0", MISE_PYTHON_COMPILE: "false", MISE_USE_VERSIONS_HOST: "false", MISE_URL_REPLACEMENTS: origin.replacements };
const stackTools = ["bun@1.4.4", "python@3.14.9", "uv@0.13.5", "dbt-fusion@2.0.9"];
const command = Bun.spawn([mise, "--no-config", "--no-env", "--no-hooks", "install", `shellcheck@${constant("SHELLCHECK_VERSION")}`, `gh@${constant("GH_VERSION")}`, `jactionlint@${constant("JACTIONLINT_VERSION")}`, ...stackTools], {
  cwd: home,
  env: environment,
  stdout: "inherit",
  stderr: "inherit",
});
const deadline = setTimeout(() => command.kill(), 120_000);
const installed = await command.exited;
clearTimeout(deadline);
if (installed !== 0) throw new Error("genuine non-Rust Mise installation failed");
const project = resolve(fixture, "lock-project");
await mkdir(project, { recursive: true });
await writeFile(resolve(project, "mise.toml"), '[tools]\npython = "3.14.9"\nuv = "0.13.5"\ndbt-fusion = "2.0.9"\n');
const locked = Bun.spawn([mise, "--no-env", "--no-hooks", "lock", "--platform", "linux-x64,linux-arm64,macos-arm64", ...stackTools.slice(1)], {
  cwd: project,
  env: { ...environment, MISE_LOCKFILE: "1", MISE_TRUSTED_CONFIG_PATHS: project },
  stdout: "inherit",
  stderr: "inherit",
});
const lockDeadline = setTimeout(() => locked.kill(), 120_000);
const lockStatus = await locked.exited;
clearTimeout(lockDeadline);
await writeFile(resolve(fixture, "recording-receipt.json"), `${JSON.stringify(origin.metrics, null, 2)}\n`, { mode: 0o600 });
origin.server.stop(true);
if (lockStatus !== 0) throw new Error("genuine non-Rust Mise lock failed");
const cache = resolve(home, platform === "macos" ? "Library/Caches/mise" : ".cache/mise");
const rows = (await Bun.file(resolve(root, "crates/velnor-actions-workflow-renderer/src/native_authority/tool_archive_pins.tsv")).text()).trim().split("\n");
const archives: { tool: string; version: string; url: string; sha256: string; bytes: number }[] = [];
for (const row of rows) {
  const [tool, version, runner, url, sha256] = row.split(" ");
  if (runner !== host) continue;
  if (!tool || !version || !url || !sha256 || !/^[a-f0-9]{64}$/.test(sha256)) throw new Error("invalid authoritative archive row");
  let found = false;
  for await (const path of new Bun.Glob("downloads-cas/blobs/*/artifact").scan({ cwd: cache, absolute: true })) {
    if (await digest(path) === sha256) {
      archives.push({ tool, version, url, sha256, bytes: Bun.file(path).size });
      found = true;
      break;
    }
  }
  if (!found) throw new Error(`genuine Mise download CAS lacks pinned ${tool} archive`);
}
if (archives.length !== 2) throw new Error("fixture must contain both qualified non-Rust archives");
await writeFile(resolve(fixture, "fixture.json"), `${JSON.stringify({ schema: 1, bun: await realpath(process.execPath), mise, miseSha256: expected, miseVersion: constant("MISE_VERSION"), host, home, cache, archives }, null, 2)}\n`, { mode: 0o600 });
if (process.env.GITHUB_ENV) await appendFile(process.env.GITHUB_ENV, `VELNOR_NONRUST_TOOLS_FIXTURE=${fixture}\n`);
process.stdout.write(`${fixture}\n`);
