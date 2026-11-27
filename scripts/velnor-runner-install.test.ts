// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { expect, test } from "bun:test";
import { writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  alive,
  binary,
  checkout,
  descendants,
  entries,
  gateFile,
  half,
  install,
  installCleanup,
  killAll,
  LOCK_FILE,
  machines,
  mode,
  sandbox,
  settled,
  start,
  temps,
  TEMP_PREFIX,
  TEST_TIMEOUT_MS,
  waitFor,
  WAIT_LIMIT_MS,
  within,
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
    expect(await entries(box)).toEqual(settled(repo));
    expect(await machines(box)).toEqual(["velnor"]);
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
    expect(await entries(box)).toEqual(settled(repo));
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
    expect(await entries(box)).toEqual(settled(repo));
  },
  TEST_TIMEOUT_MS,
);

test(
  "installs of two checkouts share the bin lock: the second reports that it waits and leaves the first run's temp file alone",
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
    await waitFor("B to report that it waits for the bin lock", async () => {
      if (await Bun.file(bStarted).exists()) {
        throw new Error("B started copying while A held the lock");
      }
      if (b.finished()) {
        throw new Error(`B exited while A held the lock: ${b.stderr()}`);
      }
      return b.stderr().includes(`waiting for ${join(box.bin, LOCK_FILE)}`);
    });
    const found = await temps(box);
    expect(found).toHaveLength(1);
    expect(await Bun.file(join(box.bin, found[0] ?? "")).text()).toBe(half(binary("0.1.81")));
    await writeFile(gate, "");
    const [first, second] = await within("A and B to finish", WAIT_LIMIT_MS, Promise.all([a.done, b.done]));
    expect(first.status, first.stderr).toBe(0);
    expect(second.status, second.stderr).toBe(0);
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await temps(box)).toEqual([]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a holder killed with SIGKILL releases both locks and the next install of the same checkout proceeds without waiting",
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
    const b = await within("B to finish after A was killed", WAIT_LIMIT_MS, install(box, repo, binary("0.1.82"), {}, "B"));
    expect(b.status, b.stderr).toBe(0);
    expect(b.stderr).not.toContain("waiting for");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await entries(box)).toEqual(settled(repo));
  },
  TEST_TIMEOUT_MS,
);
