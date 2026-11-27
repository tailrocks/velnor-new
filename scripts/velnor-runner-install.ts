// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";
import { lstat, mkdir, readFile } from "node:fs/promises";
import { basename, isAbsolute, join, resolve } from "node:path";

class InstallError extends Error {}

const REPOSITORY = resolve(import.meta.dir, "..");
const MACHINE_SOURCE = join(REPOSITORY, "crates/velnor-runner-host/src/rust_runner/mod.rs");
const MISE_DIGESTS = join(REPOSITORY, "crates/velnor-render/src/mise_digest.rs");
const GUEST_BUILD_SCRIPT = join(import.meta.dir, "velnor-runner-guest-build.sh");
const LOCKF = "/usr/bin/lockf";
const SH = "/bin/sh";
const TEMP_PREFIX = ".velnor-runner.tmp-";
const LOCK_NAME = ".velnor-runner.lock";
const LOCK_WAIT_SECONDS = 120;
const EX_TEMPFAIL = 75;
const COPY_FAILED = 70;
const GUEST_BUILD_ROOT = "/var/lib/velnor-build";
const GUEST_TARGET_TRIPLE = "aarch64-unknown-linux-gnu";
const MISE_PLATFORM = "Linux-ARM64";
const APT_PACKAGES = "build-essential rsync curl ca-certificates pkg-config libssl-dev";
const PRESERVED_ENVIRONMENT = ["PATH", "HOME"] as const;

const SWAP_SCRIPT = `set -eu
bin=$1 destination=$2 machine=$3 source=$4 prefix=$5
temp=$bin/$prefix$$
trap 'rm -f "$temp"' EXIT
trap 'exit 1' HUP INT TERM
for stale in "$bin"/"$prefix"*; do rm -f "$stale"; done
orb -m "$machine" -u root cat "$source" > "$temp" || exit ${COPY_FAILED}
chmod 755 "$temp"
mv -f "$temp" "$destination"
`;

type Machine = {
  readonly name: string;
  readonly arch: string;
  readonly distro: string;
  readonly guestStore: string;
  readonly guestRunner: string;
  readonly macMount: string;
  readonly hostStore: string;
  readonly hostBin: string;
};

type MiseBinary = { readonly version: string; readonly sha256: string };

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
  const guestStore = rustConst(source, "GUEST_STORE");
  const guestRunner = rustConst(source, "GUEST_RUNNER");
  const binDirectory = rustConst(source, "BIN_DIRECTORY");
  if (dirnameOf(guestRunner) !== `${guestStore}/${binDirectory}`) {
    throw new InstallError(`GUEST_RUNNER ${guestRunner} is not under ${guestStore}/${binDirectory}`);
  }
  const hostStore = override ?? `/${rustConst(source, "DEFAULT_HOST_STORE_UNDER_ROOT")}`;
  return {
    name: rustConst(source, "MACHINE_NAME"),
    arch: rustConst(source, "MACHINE_ARCH"),
    distro: rustConst(source, "MACHINE_DISTRO"),
    guestStore,
    guestRunner,
    macMount: rustConst(source, "MAC_MOUNT"),
    hostStore,
    hostBin: join(hostStore, binDirectory),
  };
}

const dirnameOf = (path: string): string => path.slice(0, path.lastIndexOf("/"));

async function miseBinary(): Promise<MiseBinary> {
  const pinned = /^min_version = "([^"]+)"$/m.exec(await readFile(join(REPOSITORY, "mise.toml"), "utf8"));
  const version = pinned?.[1];
  if (version === undefined || !/^\d{4}\.\d{1,2}\.\d{1,3}$/.test(version)) {
    throw new InstallError("mise.toml does not pin a mise release in min_version");
  }
  const table = await readFile(MISE_DIGESTS, "utf8");
  const row = new RegExp(`version: "${version.replaceAll(".", "\\.")}",\\s*sha256: \\[([^\\]]*)\\]`).exec(table);
  const sha256 = new RegExp(`"${MISE_PLATFORM}",\\s*"([0-9a-f]{64})"`).exec(row?.[1] ?? "")?.[1];
  if (sha256 === undefined) {
    throw new InstallError(`${MISE_DIGESTS} has no ${MISE_PLATFORM} digest for mise ${version}`);
  }
  return { version, sha256 };
}

function childEnvironment(): Record<string, string> {
  const environment: Record<string, string> = {};
  for (const name of PRESERVED_ENVIRONMENT) {
    const value = process.env[name];
    if (value !== undefined) {
      environment[name] = value;
    }
  }
  return environment;
}

async function orb(args: readonly string[], stdin?: Uint8Array): Promise<string> {
  const child = Bun.spawn(["orb", ...args], {
    cwd: "/",
    env: childEnvironment(),
    stdin: stdin ?? "ignore",
    stdout: "pipe",
    stderr: "inherit",
  });
  const [output, status] = await Promise.all([new Response(child.stdout).text(), child.exited]);
  if (status !== 0) {
    throw new InstallError(`orb ${args.slice(0, 4).join(" ")} exited with ${status}`);
  }
  return output;
}

const inMachine = (machine: Machine, ...args: string[]): string[] => ["-m", machine.name, ...args];
const asRoot = (machine: Machine, ...args: string[]): string[] => ["-m", machine.name, "-u", "root", ...args];
const bash = (script: string, ...positional: string[]): string[] => ["bash", "-euo", "pipefail", "-c", script, "_", ...positional];

async function machineExists(machine: Machine): Promise<boolean> {
  const listed: unknown = JSON.parse(await orb(["list", "-f", "json"]));
  if (!Array.isArray(listed)) {
    throw new InstallError("orb list did not return a list");
  }
  return listed.some((entry: unknown) => (entry as { name?: unknown }).name === machine.name);
}

async function ensureMachine(machine: Machine): Promise<void> {
  if (!(await machineExists(machine))) {
    await orb(["create", "-a", machine.arch, machine.distro, machine.name]);
  }
  await mkdir(machine.hostBin, { recursive: true });
  await orb(asRoot(machine, "ln", "-sfn", `${machine.macMount}${machine.hostStore}`, machine.guestStore));
}

async function ensurePackages(machine: Machine): Promise<void> {
  await orb(
    asRoot(
      machine,
      ...bash(
        '{ dpkg -s $1 >/dev/null 2>&1 || (apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq $1); } >&2',
        APT_PACKAGES,
      ),
    ),
  );
}

async function trackedFiles(): Promise<Uint8Array> {
  const child = Bun.spawn(["git", "-C", REPOSITORY, "ls-files", "-z"], {
    cwd: "/",
    env: childEnvironment(),
    stdin: "ignore",
    stdout: "pipe",
    stderr: "inherit",
  });
  const [listing, status] = await Promise.all([new Response(child.stdout).text(), child.exited]);
  if (status !== 0) {
    throw new InstallError(`git ls-files exited with ${status}`);
  }
  const present: string[] = [];
  for (const name of listing.split("\0").filter((entry) => entry !== "")) {
    if (await lstat(join(REPOSITORY, name)).then(() => true, () => false)) {
      present.push(name);
    }
  }
  return new TextEncoder().encode(present.map((name) => `${name}\0`).join(""));
}

async function build(machine: Machine): Promise<string> {
  const mise = await miseBinary();
  const key = createHash("sha256").update(REPOSITORY).digest("hex").slice(0, 16);
  await orb(
    asRoot(
      machine,
      ...bash(
        await readFile(GUEST_BUILD_SCRIPT, "utf8"),
        `${machine.macMount}${REPOSITORY}`,
        GUEST_BUILD_ROOT,
        key,
        GUEST_TARGET_TRIPLE,
        mise.version,
        mise.sha256,
      ),
    ),
    await trackedFiles(),
  );
  return `${GUEST_BUILD_ROOT}/checkouts/${key}/velnor-runner`;
}

async function lockf(args: readonly string[]): Promise<number> {
  const child = Bun.spawn([LOCKF, ...args], {
    cwd: "/",
    env: childEnvironment(),
    stdin: "ignore",
    stdout: "inherit",
    stderr: "inherit",
  });
  return child.exited;
}

async function swap(machine: Machine, destination: string, guestBinary: string): Promise<void> {
  const lock = join(machine.hostBin, LOCK_NAME);
  if ((await lockf(["-s", "-k", "-t", "0", lock, "/usr/bin/true"])) === EX_TEMPFAIL) {
    process.stderr.write(`velnor-runner:install: waiting for ${lock}\n`);
  }
  const status = await lockf([
    "-k",
    "-t",
    String(LOCK_WAIT_SECONDS),
    lock,
    SH,
    "-c",
    SWAP_SCRIPT,
    "_",
    machine.hostBin,
    destination,
    machine.name,
    guestBinary,
    TEMP_PREFIX,
  ]);
  if (status === EX_TEMPFAIL) {
    throw new InstallError(`${lock} stayed locked for ${LOCK_WAIT_SECONDS} seconds`);
  }
  if (status === COPY_FAILED) {
    throw new InstallError("copying the binary out of the machine failed");
  }
  if (status !== 0) {
    throw new InstallError(`installing the binary exited with ${status}`);
  }
}

async function main(): Promise<void> {
  const machine = await loadMachine();
  await ensureMachine(machine);
  await ensurePackages(machine);
  await swap(machine, join(machine.hostBin, basename(machine.guestRunner)), await build(machine));
  process.stdout.write(await orb(inMachine(machine, machine.guestRunner, "--version")));
}

main().catch((error: unknown) => {
  process.stderr.write(`velnor-runner:install failed: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});
