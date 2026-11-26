// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { chmod, mkdir, readdir, rename, rm } from "node:fs/promises";
import { join, resolve } from "node:path";

const MACHINE = "velnor";
const HOST_STORE = "/Users/Shared/velnor";
const HOST_BIN = join(HOST_STORE, "bin");
const DESTINATION = join(HOST_BIN, "velnor-runner");
const GUEST_STORE = "/var/lib/velnor";
const GUEST_RUNNER = `${GUEST_STORE}/bin/velnor-runner`;
const GUEST_TARGET = "/var/tmp/velnor-runner-build";
const APT_PACKAGES = "build-essential curl ca-certificates pkg-config libssl-dev";
const TEMP_PREFIX = ".velnor-runner.tmp-";
const REPOSITORY = resolve(import.meta.dir, "..");

type Orb = { readonly args: readonly string[]; readonly stdin?: string };

class InstallError extends Error {}

async function orb({ args, stdin }: Orb): Promise<string> {
  const child = Bun.spawn(["orb", ...args], {
    stdin: stdin === undefined ? "ignore" : new TextEncoder().encode(stdin),
    stdout: "pipe",
    stderr: "inherit",
  });
  const [output, status] = await Promise.all([new Response(child.stdout).text(), child.exited]);
  if (status !== 0) {
    throw new InstallError(`orb ${args.slice(0, 4).join(" ")} exited with ${status}`);
  }
  return output;
}

const inMachine = (...args: string[]): Orb => ({ args: ["-m", MACHINE, ...args] });
const asRoot = (...args: string[]): Orb => ({ args: ["-m", MACHINE, "-u", "root", ...args] });
const shell = (script: string): Orb => inMachine("bash", "-euo", "pipefail", "-c", script);
const rootShell = (script: string): Orb => asRoot("bash", "-euo", "pipefail", "-c", script);

async function machineExists(): Promise<boolean> {
  const listed: unknown = JSON.parse(await orb({ args: ["list", "-f", "json"] }));
  if (!Array.isArray(listed)) {
    throw new InstallError("orb list did not return a list");
  }
  return listed.some((entry: unknown) => (entry as { name?: unknown }).name === MACHINE);
}

async function ensureMachine(): Promise<void> {
  if (!(await machineExists())) {
    await orb({ args: ["create", "-a", "arm64", "ubuntu", MACHINE] });
  }
  await mkdir(HOST_BIN, { recursive: true });
  await orb(asRoot("ln", "-sfn", `/mnt/mac${HOST_STORE}`, GUEST_STORE));
}

async function ensureToolchain(): Promise<void> {
  await orb(
    rootShell(
      `dpkg -s ${APT_PACKAGES} >/dev/null 2>&1 || ` +
        `(apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq ${APT_PACKAGES})`,
    ),
  );
  await orb(
    shell(
      'export PATH="$HOME/.cargo/bin:$PATH"; command -v rustup >/dev/null || ' +
        "(curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none)",
    ),
  );
}

async function build(): Promise<string> {
  const source = `/mnt/mac${REPOSITORY}`;
  const binary = `${GUEST_TARGET}/release/velnor-runner`;
  await orb(
    shell(
      `export PATH="$HOME/.cargo/bin:$PATH"; cd '${source}' && ` +
        `cargo build --release --locked -p velnor-runner --target-dir '${GUEST_TARGET}'`,
    ),
  );
  return binary;
}

async function removeStaleTemps(): Promise<void> {
  for (const name of await readdir(HOST_BIN)) {
    if (name.startsWith(TEMP_PREFIX)) {
      await rm(join(HOST_BIN, name), { force: true });
    }
  }
}

async function swap(guestBinary: string): Promise<void> {
  await removeStaleTemps();
  const temp = join(HOST_BIN, `${TEMP_PREFIX}${process.pid}`);
  try {
    const child = Bun.spawn(["orb", "-m", MACHINE, "cat", guestBinary], {
      stdin: "ignore",
      stdout: Bun.file(temp),
      stderr: "inherit",
    });
    if ((await child.exited) !== 0) {
      throw new InstallError("copying the binary out of the machine failed");
    }
    await chmod(temp, 0o755);
    await rename(temp, DESTINATION);
  } finally {
    await rm(temp, { force: true });
  }
}

async function main(): Promise<void> {
  await ensureMachine();
  await ensureToolchain();
  await swap(await build());
  process.stdout.write(await orb(inMachine(GUEST_RUNNER, "--version")));
}

main().catch((error: unknown) => {
  process.stderr.write(`velnor-runner:install failed: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});
