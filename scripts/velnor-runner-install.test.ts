// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { chmod, mkdir, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  alive,
  binary,
  checkout,
  descendants,
  entries,
  FORCE_LINUX,
  gateFile,
  half,
  holdLock,
  install,
  installCleanup,
  killAll,
  calls,
  lockFile,
  machines,
  mode,
  sandbox,
  settled,
  start,
  temps,
  TEMP_PREFIX,
  TEST_TIMEOUT_MS,
  waitFor,
  type Box,
} from "./velnor-runner-install.harness";

installCleanup();

test(
  "installing twice converges: same version, identical 755 binary, one machine, no temp files",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const built = binary("0.1.82");
    const first = await install(box, repo, built);
    const second = await install(box, repo, built);
    expect(first.status, first.stderr).toBe(0);
    expect(second.status, second.stderr).toBe(0);
    expect(first.stdout).toBe("velnor-runner 0.1.82\n");
    expect(second.stdout).toBe(first.stdout);
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(built);
    expect(await mode(join(box.bin, "velnor-runner"))).toBe(0o755);
    expect(await entries(box)).toEqual(settled());
    expect(await machines(box)).toEqual(["velnor"]);
    expect(await readdir(box.tmp)).toEqual([]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a copy that fails partway keeps the old binary, prints no version and leaves no temp file",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const old = binary("0.1.81");
    expect(half(old)).not.toBe(half(binary("0.1.82")));
    expect((await install(box, repo, old)).status).toBe(0);
    const failed = await install(box, repo, binary("0.1.82"), { catFail: true });
    expect(failed.status).not.toBe(0);
    expect(failed.stdout).toBe("");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(old);
    expect(await entries(box)).toEqual(settled());
  },
  TEST_TIMEOUT_MS,
);

test(
  "a temp file from a crashed run is swept and an existing binary is replaced",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    expect((await install(box, repo, binary("0.1.81"))).status).toBe(0);
    await writeFile(join(box.bin, `${TEMP_PREFIX}99999`), "half-written");
    const replaced = await install(box, repo, binary("0.1.82"));
    expect(replaced.status, replaced.stderr).toBe(0);
    expect(replaced.stdout).toBe("velnor-runner 0.1.82\n");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await entries(box)).toEqual(settled());
  },
  TEST_TIMEOUT_MS,
);

test(
  "installs of two checkouts share one machine lock: the second reports that it waits, makes no guest call and leaves the first run's temp file alone",
  async () => {
    const box = await sandbox();
    const one = await checkout(box, "one");
    const two = await checkout(box, "two");
    expect((await install(box, one, binary("0.1.80"))).status).toBe(0);
    const aStarted = join(box.root, "a-started");
    const bStarted = join(box.root, "b-started");
    const gate = gateFile(box, "a-gate");
    const a = await start(box, one, binary("0.1.81"), { catStarted: aStarted, catGate: gate }, "A");
    await waitFor("A's copy to start", () => Bun.file(aStarted).exists());
    await waitFor("A's temp file to hold the first half of 0.1.81", async () => {
      const found = await temps(box);
      return found.length === 1 && (await Bun.file(join(box.bin, found[0] ?? "")).text()) === half(binary("0.1.81"));
    });
    const b = await start(box, two, binary("0.1.82"), { catStarted: bStarted }, "B");
    await waitFor("B to report that it waits for the install lock", async () => {
      if (await Bun.file(bStarted).exists()) {
        throw new Error("B started copying while A held the lock");
      }
      if (b.finished()) {
        throw new Error(`B exited while A held the lock: ${b.stderr()}`);
      }
      return b.stderr().includes(`waiting for ${lockFile(box)}`);
    });
    expect((await calls(box)).filter((call) => call.who === "B")).toEqual([]);
    const found = await temps(box);
    expect(found).toHaveLength(1);
    expect(await Bun.file(join(box.bin, found[0] ?? "")).text()).toBe(half(binary("0.1.81")));
    await writeFile(gate, "");
    const [first, second] = await Promise.all([a.done, b.done]);
    expect(first.status, first.stderr).toBe(0);
    expect(second.status, second.stderr).toBe(0);
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await temps(box)).toEqual([]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a holder killed with SIGKILL releases the lock and the next install proceeds without waiting",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    expect((await install(box, repo, binary("0.1.80"))).status).toBe(0);
    const aStarted = join(box.root, "a-started");
    const a = await start(box, repo, binary("0.1.81"), { catStarted: aStarted, catGate: gateFile(box, "a-gate") }, "A");
    await waitFor("A's copy to start", () => Bun.file(aStarted).exists());
    const holder = [a.child.pid, ...descendants(a.child.pid)];
    killAll(holder);
    await waitFor("A's processes to exit", () => holder.every((pid) => !alive(pid)));
    const b = await install(box, repo, binary("0.1.82"), {}, "B");
    expect(b.status, b.stderr).toBe(0);
    expect(b.stderr).not.toContain("waiting for");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await entries(box)).toEqual(settled());
  },
  TEST_TIMEOUT_MS,
);

test(
  "no internal mode exists: the old mode argument is ignored, so the install waits for an unrelated holder of the lock and then proceeds",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const gate = gateFile(box, "holder-gate");
    const holder = await holdLock(box);
    const outcome = await start(box, repo, binary("0.1.82"), {}, "B", { args: ["--run-under-install-lock"] });
    await waitFor("B to report that it waits for the install lock", async () => {
      if (outcome.finished()) {
        throw new Error(`B exited while an unrelated process held the lock: ${outcome.stderr()}`);
      }
      return outcome.stderr().includes(`waiting for ${lockFile(box)}`);
    });
    expect(await calls(box)).toEqual([]);
    expect(existsSync(box.store)).toBe(false);
    await writeFile(gate, "");
    const [held, done] = await Promise.all([holder.done, outcome.done]);
    expect(held.status, held.stderr).toBe(0);
    expect(done.status, done.stderr).toBe(0);
    expect(done.stdout).toBe("velnor-runner 0.1.82\n");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
  },
  TEST_TIMEOUT_MS,
);

test(
  "a symlinked lock file is refused and its target keeps its mode; a lock directory that others can read is refused",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const directory = join(box.home, ".velnor-runner-install");
    const victim = join(box.root, "victim");
    await mkdir(directory, { mode: 0o700 });
    await writeFile(victim, "x");
    await chmod(victim, 0o644);
    await symlink(victim, join(directory, "install.lock"));
    const linked = await install(box, repo, binary("0.1.82"));
    expect(linked.status).not.toBe(0);
    expect(linked.stderr).toContain("install.lock");
    expect(await mode(victim)).toBe(0o644);
    await rm(join(directory, "install.lock"));
    await chmod(directory, 0o755);
    const readable = await install(box, repo, binary("0.1.82"));
    expect(readable.status).not.toBe(0);
    expect(readable.stderr).toContain("the lock directory");
    expect(await calls(box)).toEqual([]);
    expect(existsSync(box.store)).toBe(false);
  },
  TEST_TIMEOUT_MS,
);

const target = async (box: Box): Promise<string> => {
  const path = join(box.root, "real");
  await mkdir(path, { mode: 0o755 });
  return path;
};

async function symlinkedStore(box: Box): Promise<Box> {
  await symlink(await target(box), box.store);
  return box;
}

async function groupWritableStore(box: Box): Promise<Box> {
  await mkdir(box.store);
  await chmod(box.store, 0o775);
  return box;
}

async function symlinkedBin(box: Box): Promise<Box> {
  await mkdir(box.store);
  await symlink(await target(box), box.bin);
  return box;
}

const rootStore = async (box: Box): Promise<Box> => ({ ...box, store: "/usr/share", bin: "/usr/share/bin" });

const unsafeStores: Array<[string, (box: Box) => Promise<Box>]> = [
  ["store symlink", symlinkedStore],
  ["store writable by the group", groupWritableStore],
  ["bin symlink", symlinkedBin],
  ["store owned by root", rootStore],
];

test(
  "a host store or bin directory that is a symlink, writable by others or owned by someone else is refused before any machine call",
  async () => {
    for (const [name, arrange] of unsafeStores) {
      const box = await arrange(await sandbox());
      const outcome = await install(box, await checkout(box), binary("0.1.82"));
      expect(outcome.status, name).not.toBe(0);
      expect(outcome.stderr, name).toContain("must be a real directory owned by");
      expect(await calls(box), name).toEqual([]);
    }
  },
  TEST_TIMEOUT_MS,
);

test(
  "off macOS the installer stops before it takes a lock or calls orb",
  async () => {
    const box = await sandbox();
    const outcome = await install(box, await checkout(box), binary("0.1.82"), {}, "run", { preload: FORCE_LINUX });
    expect(outcome.status).not.toBe(0);
    expect(outcome.stderr).toContain("runs on macOS");
    expect(await calls(box)).toEqual([]);
    expect(existsSync(join(box.home, ".velnor-runner-install"))).toBe(false);
  },
  TEST_TIMEOUT_MS,
);
