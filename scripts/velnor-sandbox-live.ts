// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PROTECTED = ["velnor", "jackin-e2e-subscription", "ruxel-proof"];
const PROVISION = [
  "set -eu",
  "mkdir -p /srv/velnor/gen/work /srv/velnor/gen/stage /var/lib/velnor/bin /var/lib/velnor/tools/key /var/lib/velnor/crates /var/lib/velnor/cache /var/lib/velnor/git /var/lib/velnor/locks",
  "chmod 755 /srv /srv/velnor /srv/velnor/gen /srv/velnor/gen/work /srv/velnor/gen/stage /var/lib /var/lib/velnor /var/lib/velnor/bin /var/lib/velnor/tools /var/lib/velnor/crates",
  "chmod 700 /var/lib/velnor/locks",
  "touch /var/lib/velnor/tools/key/.ready",
].join("\n");
const RUN = [
  "set -eu",
  ". /root/.cargo/env",
  "cd /root/velnor",
  "cargo nextest run --release -p velnor-runner --features live-sandbox --test sandbox_live --no-fail-fast",
].join("\n");

async function run(command: string[], options: { stdin?: Blob } = {}): Promise<void> {
  const child = Bun.spawn(command, { stdin: options.stdin ?? "ignore", stdout: "inherit", stderr: "inherit" });
  const status = await child.exited;
  if (status !== 0) {
    throw new Error(`${command.slice(0, 3).join(" ")} exited with ${status}`);
  }
}

async function text(command: string[]): Promise<string> {
  const child = Bun.spawn(command, { stdout: "pipe", stderr: "inherit" });
  const output = await new Response(child.stdout).text();
  if ((await child.exited) !== 0) {
    throw new Error(`${command.join(" ")} failed`);
  }
  return output;
}

async function main(): Promise<void> {
  const machine = Bun.argv[2];
  if (machine === undefined || PROTECTED.includes(machine)) {
    throw new Error("usage: bun scripts/velnor-sandbox-live.ts <throwaway isolated machine with rustup and cargo-nextest>");
  }
  const listed: Array<{ name: string }> = JSON.parse(await text(["orb", "list", "-f", "json"]));
  if (!listed.some((entry) => entry.name === machine)) {
    throw new Error(`machine ${machine} does not exist; create it with orb create --isolated --isolate-network`);
  }
  const root = (await text(["git", "rev-parse", "--show-toplevel"])).trim();
  const directory = mkdtempSync(join(tmpdir(), "velnor-sandbox-live-"));
  try {
    const archive = join(directory, "source.tar");
    const files = await text(["git", "-C", root, "ls-files", "-co", "--exclude-standard", "-z"]);
    await run(["tar", "-C", root, "--null", "-T", "-", "-cf", archive], { stdin: new Blob([files]) });
    await run(["orb", "push", "-m", machine, archive, "/root/source.tar"]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
  await run(["orb", "-m", machine, "-u", "root", "sh", "-c", `${PROVISION}\nrm -rf /root/velnor && mkdir -p /root/velnor && tar -C /root/velnor -xf /root/source.tar`]);
  await run(["orb", "-m", machine, "-u", "root", "sh", "-c", RUN]);
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
