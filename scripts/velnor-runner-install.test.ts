// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { afterEach, expect, test } from "bun:test";
import type { Subprocess } from "bun";
import { chmod, copyFile, mkdir, mkdtemp, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const SCRIPT = join(import.meta.dir, "velnor-runner-install.ts");
const FAKE_ORB = join(import.meta.dir, "fixtures/velnor-runner-install-orb.ts");
const LOCK_FILE = ".velnor-runner.lock";
const TEMP_PREFIX = ".velnor-runner.tmp-";
const WAIT_LIMIT_MS = 8_000;
const TEST_TIMEOUT_MS = 30_000;

type Outcome = { readonly stdout: string; readonly stderr: string; readonly status: number };
type Running = {
  readonly child: Subprocess<"ignore", "pipe", "pipe">;
  readonly stderr: () => string;
  readonly finished: () => boolean;
  readonly done: Promise<Outcome>;
};
type Box = { readonly root: string; readonly store: string; readonly bin: string; readonly state: string };
type Extra = { readonly catFail?: boolean; readonly catStarted?: string; readonly catGate?: string };

const scratch: string[] = [];
const gates: string[] = [];
let launches = 0;
const running: Running[] = [];

function processTable(): Array<readonly [number, number]> {
  const listing = Bun.spawnSync(["/bin/ps", "-ax", "-o", "pid=,ppid="]).stdout.toString();
  return listing
    .split("\n")
    .map((line) => line.trim().split(/\s+/).map(Number))
    .flatMap(([pid, parent]) => (pid === undefined || parent === undefined || Number.isNaN(pid) || Number.isNaN(parent) ? [] : [[pid, parent] as const]));
}

function descendants(root: number): number[] {
  const table = processTable();
  const found: number[] = [];
  const queue = [root];
  for (let parent = queue.pop(); parent !== undefined; parent = queue.pop()) {
    for (const [pid, ppid] of table) {
      if (ppid === parent && !found.includes(pid)) {
        found.push(pid);
        queue.push(pid);
      }
    }
  }
  return found;
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function killAll(pids: readonly number[]): void {
  for (const pid of pids) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      continue;
    }
  }
}

async function waitFor(what: string, check: () => boolean | Promise<boolean>, limitMs = WAIT_LIMIT_MS): Promise<void> {
  const deadline = Date.now() + limitMs;
  while (!(await check())) {
    if (Date.now() >= deadline) {
      throw new Error(`timed out after ${limitMs} ms waiting for ${what}`);
    }
    await Bun.sleep(20);
  }
}

async function within<T>(what: string, limitMs: number, work: Promise<T>): Promise<T> {
  let timer: Timer | undefined;
  const expired = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`timed out after ${limitMs} ms waiting for ${what}`)), limitMs);
  });
  try {
    return await Promise.race([work, expired]);
  } finally {
    clearTimeout(timer);
  }
}

afterEach(async () => {
  await Promise.all(gates.splice(0).map((gate) => writeFile(gate, "")));
  const active = running.splice(0);
  const everything = active.flatMap((run) => [run.child.pid, ...descendants(run.child.pid)]);
  killAll(everything);
  await Promise.all(active.map((run) => run.child.exited));
  await waitFor(`processes ${everything.join(",")} to exit`, () => everything.every((pid) => !alive(pid)));
  await Promise.all(scratch.splice(0).map((path) => rm(path, { recursive: true, force: true })));
});

async function sandbox(): Promise<Box> {
  const root = await mkdtemp(join(tmpdir(), "velnor-runner-install-"));
  scratch.push(root);
  const store = join(root, "store");
  return { root, store, bin: join(store, "bin"), state: join(root, "orb-state.json") };
}

async function fakeOrb(box: Box, built: string, extra: Extra): Promise<string> {
  launches += 1;
  const fakes = join(box.root, `fakes-${launches}`);
  await mkdir(fakes);
  await copyFile(FAKE_ORB, join(fakes, "orb"));
  await chmod(join(fakes, "orb"), 0o755);
  await writeFile(join(fakes, "config.json"), JSON.stringify({ state: box.state, bytes: built, ...extra }));
  const path = [fakes, dirname(process.execPath), "/usr/bin", "/bin"].join(":");
  expect(Bun.which("orb", { PATH: path })).toBe(join(fakes, "orb"));
  return path;
}

const gateFile = (box: Box, name: string): string => {
  const gate = join(box.root, name);
  gates.push(gate);
  return gate;
};

const binary = (version: string): string => `#!fake-binary-${version}\nvelnor-runner ${version}\npayload-${version}\n`;
const half = (text: string): string => text.slice(0, Math.floor(text.length / 2));

async function start(box: Box, built: string, extra: Extra = {}): Promise<Running> {
  const child = Bun.spawn([process.execPath, SCRIPT], {
    env: { PATH: await fakeOrb(box, built, extra), HOME: box.root, VELNOR_HOST_STORE: box.store },
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  let stderr = "";
  let finished = false;
  const decoder = new TextDecoder();
  const drain = (async () => {
    for await (const chunk of child.stderr) {
      stderr += decoder.decode(chunk, { stream: true });
    }
  })();
  const done = Promise.all([new Response(child.stdout).text(), drain, child.exited]).then(([stdout, , status]) => {
    finished = true;
    return { stdout, stderr, status };
  });
  const run = { child, stderr: () => stderr, finished: () => finished, done };
  running.push(run);
  return run;
}

const install = async (box: Box, built: string, extra: Extra = {}): Promise<Outcome> => (await start(box, built, extra)).done;
const entries = async (box: Box): Promise<string[]> => (await readdir(box.bin)).sort();
const temps = async (box: Box): Promise<string[]> => (await readdir(box.bin)).filter((name) => name.startsWith(TEMP_PREFIX));
const mode = async (path: string): Promise<number> => (await stat(path)).mode & 0o777;
const machines = async (box: Box): Promise<string[]> => (JSON.parse(await Bun.file(box.state).text()) as { machines: string[] }).machines;

test(
  "installing twice converges: same version, identical 755 binary, one machine, no temp files",
  async () => {
    const box = await sandbox();
    const built = binary("0.1.82");
    const first = await install(box, built);
    const second = await install(box, built);
    expect(first.status, first.stderr).toBe(0);
    expect(second.status, second.stderr).toBe(0);
    expect(first.stdout).toBe("velnor-runner 0.1.82\n");
    expect(second.stdout).toBe(first.stdout);
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(built);
    expect(await mode(join(box.bin, "velnor-runner"))).toBe(0o755);
    expect(await entries(box)).toEqual([LOCK_FILE, "velnor-runner"]);
    expect(await machines(box)).toEqual(["velnor"]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a copy that fails partway keeps the old binary, prints no version and leaves no temp file",
  async () => {
    const box = await sandbox();
    const old = binary("0.1.81");
    expect(half(old)).not.toBe(half(binary("0.1.82")));
    expect((await install(box, old)).status).toBe(0);
    const failed = await install(box, binary("0.1.82"), { catFail: true });
    expect(failed.status).not.toBe(0);
    expect(failed.stdout).toBe("");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(old);
    expect(await entries(box)).toEqual([LOCK_FILE, "velnor-runner"]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a temp file from a crashed run is swept and an existing binary is replaced",
  async () => {
    const box = await sandbox();
    expect((await install(box, binary("0.1.81"))).status).toBe(0);
    await writeFile(join(box.bin, `${TEMP_PREFIX}99999`), "half-written");
    const replaced = await install(box, binary("0.1.82"));
    expect(replaced.status, replaced.stderr).toBe(0);
    expect(replaced.stdout).toBe("velnor-runner 0.1.82\n");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await entries(box)).toEqual([LOCK_FILE, "velnor-runner"]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a concurrent install reports that it waits for the lock and leaves the first run's temp file alone",
  async () => {
    const box = await sandbox();
    expect((await install(box, binary("0.1.80"))).status).toBe(0);
    const aStarted = join(box.root, "a-started");
    const bStarted = join(box.root, "b-started");
    const gate = gateFile(box, "a-gate");
    const a = await start(box, binary("0.1.81"), { catStarted: aStarted, catGate: gate });
    await waitFor("A's copy to start", () => Bun.file(aStarted).exists());
    await waitFor("A's temp file to hold the first half of 0.1.81", async () => {
      const found = await temps(box);
      return found.length === 1 && (await Bun.file(join(box.bin, found[0] ?? "")).text()) === half(binary("0.1.81"));
    });
    const b = await start(box, binary("0.1.82"), { catStarted: bStarted });
    await waitFor("B to report that it waits for the lock", async () => {
      if (await Bun.file(bStarted).exists()) {
        throw new Error("B started copying while A held the lock");
      }
      if (b.finished()) {
        throw new Error(`B exited while A held the lock: ${b.stderr()}`);
      }
      return b.stderr().includes(`waiting for ${join(box.bin, LOCK_FILE)}`);
    });
    expect(await Bun.file(bStarted).exists()).toBe(false);
    const found = await temps(box);
    expect(found).toHaveLength(1);
    expect(await Bun.file(join(box.bin, found[0] ?? "")).text()).toBe(half(binary("0.1.81")));
    await writeFile(gate, "");
    const [first, second] = await within("A and B to finish", WAIT_LIMIT_MS, Promise.all([a.done, b.done]));
    expect(first.status, first.stderr).toBe(0);
    expect(second.status, second.stderr).toBe(0);
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await entries(box)).toEqual([LOCK_FILE, "velnor-runner"]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a holder killed with SIGKILL releases the lock and the next install proceeds without waiting",
  async () => {
    const box = await sandbox();
    expect((await install(box, binary("0.1.80"))).status).toBe(0);
    const aStarted = join(box.root, "a-started");
    const a = await start(box, binary("0.1.81"), { catStarted: aStarted, catGate: gateFile(box, "a-gate") });
    await waitFor("A's copy to start", () => Bun.file(aStarted).exists());
    const holder = [a.child.pid, ...descendants(a.child.pid)];
    killAll(holder);
    await waitFor("A's processes to exit", () => holder.every((pid) => !alive(pid)));
    const b = await within("B to finish after A was killed", WAIT_LIMIT_MS, install(box, binary("0.1.82")));
    expect(b.status, b.stderr).toBe(0);
    expect(b.stderr).not.toContain("waiting for");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
    expect(await entries(box)).toEqual([LOCK_FILE, "velnor-runner"]);
  },
  TEST_TIMEOUT_MS,
);
