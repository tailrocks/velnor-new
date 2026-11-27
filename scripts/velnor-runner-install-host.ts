// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { constants } from "node:fs";
import { chmod, lstat, mkdir, open, readdir, rename, rm } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import type { Machine } from "./velnor-runner-install-config";
import { step, type Context } from "./velnor-runner-install-guest";
import { EX_TEMPFAIL, hostEnvironment, InstallError, run } from "./velnor-runner-install-lib";

const LOCKF = "/usr/bin/lockf";
const PS = "/bin/ps";
const LOCK_WAIT_SECONDS = 900;
const LOCK_DIRECTORY = ".velnor-runner-install";
const LOCK_NAME = "install.lock";
const TEMP_PREFIX = ".velnor-runner.tmp-";
const OTHERS_WRITE = 0o022;
const OTHERS_ANY = 0o077;

const lockPath = (): string => join(homedir(), LOCK_DIRECTORY, LOCK_NAME);

async function ensureDirectory(path: string, label: string, forbidden: number, create: boolean): Promise<void> {
  let info = await lstat(path).catch(() => undefined);
  if (info === undefined) {
    if (!create) {
      return;
    }
    await mkdir(path, { recursive: true, mode: forbidden === OTHERS_ANY ? 0o700 : 0o755 });
    info = await lstat(path);
  }
  const uid = process.getuid?.() ?? -1;
  if (!info.isDirectory() || info.uid !== uid || (info.mode & forbidden) !== 0) {
    throw new InstallError(`${label} ${path} must be a real directory owned by uid ${uid} that other users cannot write${forbidden === OTHERS_ANY ? " or read" : ""}`);
  }
}

export async function ensureStore(machine: Machine, create: boolean): Promise<void> {
  await ensureDirectory(machine.hostStore, "the host store", OTHERS_WRITE, create);
  await ensureDirectory(machine.hostBin, "the host bin directory", OTHERS_WRITE, create);
}

export async function prepareLock(): Promise<string> {
  const lock = lockPath();
  await ensureDirectory(join(homedir(), LOCK_DIRECTORY), "the lock directory", OTHERS_ANY, true);
  const handle = await open(lock, constants.O_RDWR | constants.O_CREAT | constants.O_NOFOLLOW, 0o600);
  try {
    await handle.chmod(0o600);
  } finally {
    await handle.close();
  }
  return lock;
}

const lockCommand = (lock: string, command: readonly string[]): string[] => [LOCKF, "-k", "-t", String(LOCK_WAIT_SECONDS), lock, ...command];

const probe = (lock: string): Promise<{ status: number }> => run([LOCKF, "-s", "-k", "-t", "0", lock, "/usr/bin/true"], { env: hostEnvironment(), output: "capture" });

export async function underLock(lock: string, command: readonly string[], extra: Record<string, string>): Promise<number> {
  if ((await probe(lock)).status === EX_TEMPFAIL) {
    process.stderr.write(`velnor-runner:install: waiting for ${lock}\n`);
  }
  const held = await run(lockCommand(lock, command), { env: hostEnvironment(extra), output: "stdout" });
  if (held.status === EX_TEMPFAIL) {
    throw new InstallError(`${lock} stayed locked for ${LOCK_WAIT_SECONDS} seconds`);
  }
  return held.status;
}

export async function requireLockHeld(command: readonly string[]): Promise<void> {
  const parent = await run([PS, "-ww", "-o", "args=", "-p", String(process.ppid)], { env: hostEnvironment(), output: "capture" });
  if (parent.status !== 0 || parent.stdout.trim() !== lockCommand(lockPath(), command).join(" ")) {
    throw new InstallError("this mode runs only under the install lock, as the child of the lockf that holds it; use `mise run velnor-runner:install`");
  }
}

async function sha256Of(path: string): Promise<string> {
  const hasher = new Bun.CryptoHasher("sha256");
  for await (const chunk of Bun.file(path).stream()) {
    hasher.update(chunk);
  }
  return hasher.digest("hex");
}

export async function swap(context: Context, expected: string): Promise<void> {
  const { machine, paths } = context;
  const name = `${TEMP_PREFIX}${context.installId}`;
  const temp = join(machine.hostBin, name);
  for (const stale of (await readdir(machine.hostBin)).filter((entry) => entry.startsWith(TEMP_PREFIX))) {
    await rm(join(machine.hostBin, stale), { force: true });
  }
  try {
    await step(context, "copy", ["cp", paths.output, `${machine.guestBin}/${name}`]);
    const actual = await sha256Of(temp);
    if (actual !== expected) {
      throw new InstallError(`step 'integrity': the copy has sha256 ${actual}, the machine built ${expected}`);
    }
    await chmod(temp, 0o755);
    await rename(temp, join(machine.hostBin, basename(machine.guestRunner)));
  } finally {
    await rm(temp, { force: true });
  }
}
