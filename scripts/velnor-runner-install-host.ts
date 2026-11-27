// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { constants } from "node:fs";
import { dlopen, FFIType, read, type Pointer } from "bun:ffi";
import { chmod, lstat, mkdir, open, readdir, rename, rm, type FileHandle } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, join } from "node:path";
import type { Machine } from "./velnor-runner-install-config";
import { step, type Context } from "./velnor-runner-install-guest";
import { InstallError } from "./velnor-runner-install-lib";

const LOCK_WAIT_SECONDS = 900;
const LOCK_POLL_MS = 100;
const LOCK_EX = 2;
const LOCK_NB = 4;
const EWOULDBLOCK = 35;
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

async function lockDescriptor(fd: number, lock: string): Promise<void> {
  const { symbols } = dlopen("libSystem.B.dylib", {
    flock: { args: [FFIType.i32, FFIType.i32], returns: FFIType.i32 },
    __error: { args: [], returns: FFIType.ptr },
  });
  const deadline = Date.now() + LOCK_WAIT_SECONDS * 1000;
  for (let announced = false; symbols.flock(fd, LOCK_EX | LOCK_NB) !== 0; announced = true) {
    const errno = read.i32(symbols.__error() as Pointer);
    if (errno !== EWOULDBLOCK) {
      throw new InstallError(`flock of ${lock} failed with errno ${errno}`);
    }
    if (!announced) {
      process.stderr.write(`velnor-runner:install: waiting for ${lock}\n`);
    }
    if (Date.now() >= deadline) {
      throw new InstallError(`${lock} stayed locked for ${LOCK_WAIT_SECONDS} seconds`);
    }
    await Bun.sleep(LOCK_POLL_MS);
  }
}

export async function takeLock(): Promise<FileHandle> {
  const lock = lockPath();
  await ensureDirectory(join(homedir(), LOCK_DIRECTORY), "the lock directory", OTHERS_ANY, true);
  const handle = await open(lock, constants.O_RDWR | constants.O_CREAT | constants.O_NOFOLLOW, 0o600);
  try {
    await handle.chmod(0o600);
    await lockDescriptor(handle.fd, lock);
  } catch (error) {
    await handle.close();
    throw error;
  }
  return handle;
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
