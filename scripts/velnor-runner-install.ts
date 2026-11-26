// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { chmod, mkdir, readdir, readFile, rename, rm, writeFile } from "node:fs/promises";
import { basename, isAbsolute, join, resolve } from "node:path";

class InstallError extends Error {}

const REPOSITORY = resolve(import.meta.dir, "..");
const MACHINE_SOURCE = join(REPOSITORY, "crates/velnor-runner-host/src/rust_runner/mod.rs");
const TEMP_PREFIX = ".velnor-runner.tmp-";
const LOCK_NAME = ".velnor-runner.lock";
const LOCK_WAIT_MS = 120_000;
const GUEST_BUILD_DIRECTORY = "/var/tmp/velnor-runner-build";
const GUEST_TARGET_TRIPLE = "aarch64-unknown-linux-gnu";
const APT_PACKAGES = "build-essential rsync curl ca-certificates pkg-config libssl-dev";

type Machine = {
  readonly name: string;
  readonly arch: string;
  readonly distro: string;
  readonly guestStore: string;
  readonly guestRunner: string;
  readonly macMount: string;
  readonly hostStore: string;
};

function rustConst(source: string, name: string): string {
  const match = new RegExp(`^const ${name}: &str = "([^"]*)";$`, "m").exec(source);
  if (match?.[1] === undefined) {
    throw new InstallError(`${MACHINE_SOURCE} no longer defines const ${name}`);
  }
  return match[1];
}

async function loadMachine(): Promise<Machine> {
  const source = await readFile(MACHINE_SOURCE, "utf8");
  const override = process.env.VELNOR_HOST_STORE;
  if (override !== undefined && !isAbsolute(override)) {
    throw new InstallError(`VELNOR_HOST_STORE must be an absolute path, got '${override}'`);
  }
  return {
    name: rustConst(source, "MACHINE_NAME"),
    arch: rustConst(source, "MACHINE_ARCH"),
    distro: rustConst(source, "MACHINE_DISTRO"),
    guestStore: rustConst(source, "GUEST_STORE"),
    guestRunner: rustConst(source, "GUEST_RUNNER"),
    macMount: rustConst(source, "MAC_MOUNT"),
    hostStore: override ?? `/${rustConst(source, "DEFAULT_HOST_STORE_UNDER_ROOT")}`,
  };
}

async function miseVersion(): Promise<string> {
  const match = /^min_version = "([^"]+)"$/m.exec(await readFile(join(REPOSITORY, "mise.toml"), "utf8"));
  if (match?.[1] === undefined) {
    throw new InstallError("mise.toml does not pin min_version");
  }
  return match[1];
}

type Orb = { readonly args: readonly string[] };

async function orb({ args }: Orb): Promise<string> {
  const child = Bun.spawn(["orb", ...args], { stdin: "ignore", stdout: "pipe", stderr: "inherit" });
  const [output, status] = await Promise.all([new Response(child.stdout).text(), child.exited]);
  if (status !== 0) {
    throw new InstallError(`orb ${args.slice(0, 4).join(" ")} exited with ${status}`);
  }
  return output;
}

const inMachine = (machine: Machine, ...args: string[]): Orb => ({ args: ["-m", machine.name, ...args] });
const asRoot = (machine: Machine, ...args: string[]): Orb => ({ args: ["-m", machine.name, "-u", "root", ...args] });
const bash = (script: string, ...positional: string[]): string[] => ["bash", "-euo", "pipefail", "-c", script, "_", ...positional];

async function machineExists(machine: Machine): Promise<boolean> {
  const listed: unknown = JSON.parse(await orb({ args: ["list", "-f", "json"] }));
  if (!Array.isArray(listed)) {
    throw new InstallError("orb list did not return a list");
  }
  return listed.some((entry: unknown) => (entry as { name?: unknown }).name === machine.name);
}

async function ensureMachine(machine: Machine, hostBin: string): Promise<void> {
  if (!(await machineExists(machine))) {
    await orb({ args: ["create", "-a", machine.arch, machine.distro, machine.name] });
  }
  await mkdir(hostBin, { recursive: true });
  await orb(asRoot(machine, "ln", "-sfn", `${machine.macMount}${machine.hostStore}`, machine.guestStore));
}

async function ensureToolchain(machine: Machine): Promise<void> {
  await orb(
    asRoot(
      machine,
      ...bash(
        'dpkg -s $1 >/dev/null 2>&1 || (apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq $1)',
        APT_PACKAGES,
      ),
    ),
  );
  const version = await miseVersion();
  await orb(
    inMachine(
      machine,
      ...bash(
        'mkdir -p "$HOME/.local/bin"; ' +
          '[ "$("$HOME/.local/bin/mise" --version 2>/dev/null | cut -d" " -f1)" = "$1" ] || ' +
          '(curl --proto "=https" --tlsv1.2 -fsSL "https://github.com/jdx/mise/releases/download/v$1/mise-v$1-linux-arm64" ' +
          '-o "$HOME/.local/bin/mise.download" && chmod +x "$HOME/.local/bin/mise.download" && ' +
          'mv "$HOME/.local/bin/mise.download" "$HOME/.local/bin/mise")',
        version,
      ),
    ),
  );
}

async function build(machine: Machine): Promise<string> {
  await orb(
    inMachine(
      machine,
      ...bash(
        'export PATH="$HOME/.local/bin:$PATH" MISE_TRUSTED_CONFIG_PATHS="$2"; mkdir -p "$2" && ' +
          'rsync -a --delete --exclude /target --exclude /.git --exclude node_modules "$1/" "$2/" && cd "$2" && ' +
          "mise install --locked && mise exec -- mbx doctor && " +
          'mise exec -- cargo build --release --locked -p velnor-runner --target "$3"',
        `${machine.macMount}${REPOSITORY}`,
        GUEST_BUILD_DIRECTORY,
        GUEST_TARGET_TRIPLE,
      ),
    ),
  );
  return `${GUEST_BUILD_DIRECTORY}/target/${GUEST_TARGET_TRIPLE}/release/velnor-runner`;
}

function isAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

async function acquireLock(hostBin: string): Promise<string> {
  const lock = join(hostBin, LOCK_NAME);
  const deadline = Date.now() + LOCK_WAIT_MS;
  for (;;) {
    try {
      await mkdir(lock);
      await writeFile(join(lock, "pid"), String(process.pid));
      return lock;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
        throw error;
      }
    }
    const holder = Number.parseInt(await readFile(join(lock, "pid"), "utf8").catch(() => ""), 10);
    if (Number.isInteger(holder) && !isAlive(holder)) {
      const claimed = `${lock}.stale-${process.pid}`;
      await rename(lock, claimed).catch(() => undefined);
      await rm(claimed, { recursive: true, force: true });
      continue;
    }
    if (Date.now() > deadline) {
      throw new InstallError(`${lock} is held by another install`);
    }
    await Bun.sleep(100);
  }
}

async function removeStaleTemps(hostBin: string): Promise<void> {
  for (const name of await readdir(hostBin)) {
    if (name.startsWith(TEMP_PREFIX)) {
      await rm(join(hostBin, name), { force: true });
    }
  }
}

async function swap(machine: Machine, hostBin: string, destination: string, guestBinary: string): Promise<void> {
  const lock = await acquireLock(hostBin);
  const temp = join(hostBin, `${TEMP_PREFIX}${process.pid}`);
  try {
    await removeStaleTemps(hostBin);
    const child = Bun.spawn(["orb", "-m", machine.name, "cat", guestBinary], {
      stdin: "ignore",
      stdout: Bun.file(temp),
      stderr: "inherit",
    });
    if ((await child.exited) !== 0) {
      throw new InstallError("copying the binary out of the machine failed");
    }
    await chmod(temp, 0o755);
    await rename(temp, destination);
  } finally {
    await rm(temp, { force: true });
    await rm(lock, { recursive: true, force: true });
  }
}

async function main(): Promise<void> {
  const machine = await loadMachine();
  const hostBin = join(machine.hostStore, "bin");
  await ensureMachine(machine, hostBin);
  await ensureToolchain(machine);
  await swap(machine, hostBin, join(hostBin, basename(machine.guestRunner)), await build(machine));
  process.stdout.write(await orb(inMachine(machine, machine.guestRunner, "--version")));
}

main().catch((error: unknown) => {
  process.stderr.write(`velnor-runner:install failed: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});
