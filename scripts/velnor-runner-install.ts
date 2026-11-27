// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { chmod, mkdir, open } from "node:fs/promises";
import { basename, join } from "node:path";
import { checkoutKey, loadMachine, miseBinary, REPOSITORY, type Machine } from "./velnor-runner-install-config";
import { build, guestArgv, layout, provision, step, type Context } from "./velnor-runner-install-guest";
import { EX_TEMPFAIL, hostEnvironment, InstallError, machineNames, orb, run } from "./velnor-runner-install-lib";
import { archiveCommit, cleanCommit, isObjectName } from "./velnor-runner-install-source";

const LOCKF = "/usr/bin/lockf";
const LOCKED = "--locked";
const TEMP_PREFIX = ".velnor-runner.tmp-";
const LOCK_NAME = ".velnor-runner.lock";
const LOCK_WAIT_SECONDS = 900;
const COPY_FAILED = 70;
const INTEGRITY_FAILED = 71;

const SWAP_SCRIPT = `set -eu
bin=$1 destination=$2 prefix=$3 expected=$4 temp=$5
shift 5
trap 'rm -f "$temp"' EXIT
trap 'exit 1' HUP INT TERM
for stale in "$bin"/"$prefix"*; do rm -f "$stale"; done
"$@" || exit ${COPY_FAILED}
actual=$(/usr/bin/shasum -a 256 "$temp" | cut -d' ' -f1)
[ "$actual" = "$expected" ] || { echo "velnor-runner:install: the copy has sha256 $actual, the machine built $expected" >&2; exit ${INTEGRITY_FAILED}; }
chmod 755 "$temp"
mv -f "$temp" "$destination"
`;

async function touchLock(path: string): Promise<void> {
  await (await open(path, "a", 0o666)).close();
  try {
    await chmod(path, 0o666);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EPERM") {
      throw error;
    }
  }
}

async function underLock(lock: string, command: readonly string[], output: "stderr" | "stdout", extra: Record<string, string> = {}): Promise<number> {
  await touchLock(lock);
  const env = hostEnvironment(extra);
  if ((await run([LOCKF, "-s", "-k", "-t", "0", lock, "/usr/bin/true"], { env, output: "capture" })).status === EX_TEMPFAIL) {
    process.stderr.write(`velnor-runner:install: waiting for ${lock}\n`);
  }
  const held = await run([LOCKF, "-k", "-t", String(LOCK_WAIT_SECONDS), lock, ...command], { env, output });
  if (held.status === EX_TEMPFAIL) {
    throw new InstallError(`${lock} stayed locked for ${LOCK_WAIT_SECONDS} seconds`);
  }
  return held.status;
}

async function refuseRepoint(context: Context): Promise<void> {
  const { machine } = context;
  if (!(await machineNames()).includes(machine.name)) {
    return;
  }
  const current = await step(context, "store-link", ["readlink", machine.guestStore], { capture: true, accept: [1] });
  const target = current.stdout.trim();
  if (current.status === 0 && target !== machine.link) {
    throw new InstallError(`step 'store-link': ${machine.name} links ${machine.guestStore} to ${target}; refusing to repoint it to ${machine.link} (VELNOR_HOST_STORE)`);
  }
}

async function ensureMachine(context: Context): Promise<void> {
  const { machine } = context;
  if (!(await machineNames()).includes(machine.name)) {
    const created = await orb(["create", "-a", machine.arch, machine.distro, machine.name], { output: "capture" });
    if (created.status !== 0 && !created.stderr.includes("already exists")) {
      throw new InstallError(`step 'machine-create': orb create exited with ${created.status}`);
    }
  }
  await mkdir(machine.hostBin, { recursive: true });
  await step(context, "store-link", ["ln", "-sfn", machine.link, machine.guestStore]);
}

async function swap(context: Context, sha256: string): Promise<void> {
  const { machine, paths } = context;
  const destination = join(machine.hostBin, basename(machine.guestRunner));
  const name = `${TEMP_PREFIX}${context.installId}`;
  const copy = ["orb", ...guestArgv(context, ["cp", paths.output, `${machine.guestBin}/${name}`])];
  const swapArguments = ["/bin/sh", "-c", SWAP_SCRIPT, "_", machine.hostBin, destination, TEMP_PREFIX, sha256, join(machine.hostBin, name), ...copy];
  const status = await underLock(join(machine.hostBin, LOCK_NAME), swapArguments, "stderr");
  if (status === COPY_FAILED) {
    throw new InstallError("step 'copy': copying the binary out of the machine failed");
  }
  if (status === INTEGRITY_FAILED) {
    throw new InstallError("step 'integrity': the copied binary does not match the digest the machine reported");
  }
  if (status !== 0) {
    throw new InstallError(`step 'copy': installing the binary exited with ${status}`);
  }
}

async function install(machine: Machine, commit: string): Promise<void> {
  const context: Context = { machine, paths: layout(checkoutKey()), installId: crypto.randomUUID() };
  const source = await archiveCommit(REPOSITORY, commit);
  await ensureMachine(context);
  try {
    await provision(context, await miseBinary(), source);
    await swap(context, await build(context));
    process.stdout.write((await step(context, "version", [machine.guestRunner, "--version"], { capture: true })).stdout);
  } finally {
    await step(context, "cleanup", ["rm", "-rf", context.paths.work]).catch((error: unknown) => {
      process.stderr.write(`velnor-runner:install: ${error instanceof Error ? error.message : String(error)}\n`);
    });
  }
}

async function main(): Promise<void> {
  const machine = await loadMachine();
  if (process.argv[2] === LOCKED) {
    const commit = process.argv[3] ?? "";
    if (!isObjectName(commit)) {
      throw new InstallError(`${LOCKED} needs a commit id, got '${commit}'`);
    }
    return install(machine, commit);
  }
  const commit = await cleanCommit(REPOSITORY);
  process.stderr.write(`velnor-runner:install: building ${commit}\n`);
  await refuseRepoint({ machine, paths: layout(checkoutKey()), installId: "" });
  await mkdir(machine.hostBin, { recursive: true });
  const lock = join(machine.hostBin, `.velnor-runner.${checkoutKey()}.lock`);
  const forwarded = process.env.VELNOR_HOST_STORE === undefined ? {} : { VELNOR_HOST_STORE: process.env.VELNOR_HOST_STORE };
  const status = await underLock(lock, [process.execPath, import.meta.path, LOCKED, commit], "stdout", forwarded);
  process.exit(status);
}

main().catch((error: unknown) => {
  process.stderr.write(`velnor-runner:install failed: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});
