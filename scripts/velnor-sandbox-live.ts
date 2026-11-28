// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PROTECTED = ["velnor", "jackin-e2e-subscription", "ruxel-proof"];
const THROWAWAY = /^velnor-(live|u[a-z0-9]+)-[a-z0-9]+$/;
const MISE_VERSION = "v2026.11.10";
const MISE_ARCHIVE = `mise-${MISE_VERSION}-linux-arm64.tar.gz`;
const MISE_SHA256 = "af305e1efc58433a79b893738bfbf11b4b1662bdea9f0ea71f2ecab990058ad6";
const RAM = "/var/tmp/velnor-ram";
const SOURCE = `${RAM}/src`;
const CACHES = `export XDG_CACHE_HOME=${RAM}/cache CARGO_HOME=${RAM}/cargo`;
const PROVISION = [
  "set -eu",
  `mkdir -p ${RAM}`,
  `mountpoint -q ${RAM} || mount -t tmpfs -o size=20g tmpfs ${RAM}`,
  "mkdir -p /srv/velnor/gen/work /srv/velnor/gen/stage /var/lib/velnor/bin /var/lib/velnor/tools/key /var/lib/velnor/crates /var/lib/velnor/cache /var/lib/velnor/git /var/lib/velnor/locks",
  "chmod 755 /srv /srv/velnor /srv/velnor/gen /srv/velnor/gen/work /srv/velnor/gen/stage /var/lib /var/lib/velnor /var/lib/velnor/bin /var/lib/velnor/tools /var/lib/velnor/crates",
  "chmod 700 /var/lib/velnor/locks",
  "touch /var/lib/velnor/tools/key/.ready",
  "command -v ip >/dev/null && command -v nsenter >/dev/null || { echo 'iproute2 and nsenter are required' >&2; exit 1; }",
  "command -v nft >/dev/null && command -v cc >/dev/null && command -v pkg-config >/dev/null || DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends nftables gcc libc6-dev libssl-dev pkg-config ca-certificates",
  "command -v nft >/dev/null || { echo 'nftables is required' >&2; exit 1; }",
  "command -v cc >/dev/null || { echo 'a C linker is required' >&2; exit 1; }",
].join("\n");
const TOOLCHAIN = [
  "set -eu",
  "if ! command -v mise >/dev/null; then",
  `  curl -fsSL -o /var/tmp/${MISE_ARCHIVE} https://github.com/jdx/mise/releases/download/${MISE_VERSION}/${MISE_ARCHIVE}`,
  `  echo "${MISE_SHA256}  /var/tmp/${MISE_ARCHIVE}" | sha256sum -c -`,
  `  tar -C /var/tmp -xzf /var/tmp/${MISE_ARCHIVE}`,
  "  install -m 0755 /var/tmp/mise/bin/mise /usr/local/bin/mise",
  "fi",
  `cd ${SOURCE}`,
  `export MISE_TRUSTED_CONFIG_PATHS=${SOURCE}`,
  CACHES,
  "mise install rust mr-boxington aqua:nextest-rs/nextest/cargo-nextest",
  'eval "$(mise activate bash --shims)"',
  "wrapper=$(type -a cargo | head -n 1)",
  'case "$wrapper" in *mise/command-wrappers/bin/*) ;; *) echo "cargo does not resolve to the mise command wrapper: $wrapper" >&2; exit 1 ;; esac',
  "mbx doctor",
].join("\n");
const RUN = [
  "set -eu",
  `cd ${SOURCE}`,
  `export MISE_TRUSTED_CONFIG_PATHS=${SOURCE}`,
  CACHES,
  'eval "$(mise activate bash --shims)"',
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
  if (machine === undefined || PROTECTED.includes(machine) || !THROWAWAY.test(machine)) {
    throw new Error("usage: bun scripts/velnor-sandbox-live.ts velnor-live-<suffix> (a throwaway machine made with orb create --isolated --isolate-network --disk 8G -a arm64 ubuntu <name>)");
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
    await run(["orb", "push", "-m", machine, archive, "/var/tmp/velnor-source.tar"]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
  await run(["orb", "-m", machine, "-u", "root", "sh", "-c", `${PROVISION}\nrm -rf ${SOURCE} && mkdir -p ${SOURCE} && tar -C ${SOURCE} -xf /var/tmp/velnor-source.tar`]);
  await run(["orb", "-m", machine, "-u", "root", "bash", "-c", TOOLCHAIN]);
  await run(["orb", "-m", machine, "-u", "root", "bash", "-c", RUN]);
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
