// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { afterEach, expect } from "bun:test";
import type { Subprocess } from "bun";
import { realpathSync } from "node:fs";
import { chmod, copyFile, cp, mkdir, mkdtemp, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

if (process.platform !== "darwin") {
  throw new Error("the velnor-runner install tests need macOS (/usr/bin/lockf)");
}

const SCRIPTS = import.meta.dir;
const REPOSITORY = join(SCRIPTS, "..");
const FAKE_ORB = join(SCRIPTS, "fixtures/velnor-runner-install-orb.ts");
const FAKE_GIT = join(SCRIPTS, "fixtures/velnor-runner-install-git.ts");
export const FORCE_LINUX = join(SCRIPTS, "fixtures/velnor-runner-install-linux.ts");
const INSTALLER_FILES = [
  "velnor-runner-install.ts",
  "velnor-runner-install-lib.ts",
  "velnor-runner-install-config.ts",
  "velnor-runner-install-guest.ts",
  "velnor-runner-install-host.ts",
  "velnor-runner-install-source.ts",
];
const COPIED_INPUTS = ["crates/velnor-runner-host/src/rust_runner/mod.rs", "crates/velnor-render/src/mise_digest.rs", "mise.toml", "rust-toolchain.toml"];

export const UNDER_LOCK = "--run-under-install-lock";
export const TEMP_PREFIX = ".velnor-runner.tmp-";
export const WAIT_LIMIT_MS = 8_000;
export const TEST_TIMEOUT_MS = 30_000;

export type Outcome = { readonly stdout: string; readonly stderr: string; readonly status: number };
export type Running = {
  readonly child: Subprocess<"ignore", "pipe", "pipe">;
  readonly stderr: () => string;
  readonly finished: () => boolean;
  readonly done: Promise<Outcome>;
};
export type Box = { readonly root: string; readonly store: string; readonly bin: string; readonly state: string; readonly log: string; readonly home: string };
export type Repo = { readonly dir: string; readonly script: string };
export type Launch = { readonly preload?: string; readonly args?: readonly string[] };
export type Extra = Record<string, unknown>;
export type Call = {
  who: string;
  args: string[];
  env: string[];
  command?: string;
  operands?: string[];
  user?: string;
  assignments?: Record<string, string>;
  outer?: Record<string, string>;
  namespaced?: boolean;
  stdinSha256?: string;
  checked?: string;
  checkedSha?: string;
};

export const MOUNT_INFO = [
  "253 92 0:38 /scon/containers/AAA/rootfs / rw,noatime shared:242 - btrfs /dev/vdb1 rw,subvol=/scon",
  "410 253 0:20 /opt/orbstack-guest /opt/orbstack-guest ro,noatime shared:296 - overlay orbstack rw,lowerdir=/",
  "413 253 0:37 / /mnt/mac rw,relatime shared:299 - virtiofs mac rw",
  "414 253 0:37 /Applications /Applications rw,relatime shared:300 - virtiofs mac rw",
  "415 253 0:37 /Library /Library rw,relatime shared:301 - virtiofs mac rw",
  "416 253 0:37 /Users /Users rw,relatime shared:302 - virtiofs mac rw",
  "417 253 0:37 /Volumes /Volumes rw,relatime shared:306 - virtiofs mac rw",
  "418 253 0:37 /private /private rw,relatime shared:307 - virtiofs mac rw",
  "425 416 0:61 /Shared/Nested /Users/Nested rw,relatime shared:310 - virtiofs extra rw",
  "426 253 0:62 /Shared /Extra\\040Disk rw,relatime shared:311 - virtiofs extra rw",
  "419 253 0:50 / /mnt/machines rw,relatime shared:308 - tmpfs tmpfs rw",
  "420 419 0:38 /scon/containers/BBB/rootfs /mnt/machines/other rw,noatime shared:309 - btrfs /dev/vdb1 rw",
  "424 416 0:38 /scon/containers/BBB/rootfs /Users/someone/OrbStack/other rw,noatime shared:304 - btrfs /dev/vdb1 rw",
  "",
].join("\n");
export const MASKED_MOUNTS = ["/Extra Disk", "/Applications", "/Library", "/Users", "/Volumes", "/mnt/mac", "/mnt/machines", "/private"];

const scratch: string[] = [];
const gates: string[] = [];
const running: Running[] = [];
let launches = 0;

function processTable(): Array<readonly [number, number]> {
  const listing = Bun.spawnSync(["/bin/ps", "-ax", "-o", "pid=,ppid="]).stdout.toString();
  return listing
    .split("\n")
    .map((line) => line.trim().split(/\s+/).map(Number))
    .flatMap(([pid, parent]) => (pid === undefined || parent === undefined || Number.isNaN(pid) || Number.isNaN(parent) ? [] : [[pid, parent] as const]));
}

export function descendants(root: number): number[] {
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

export function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export function killAll(pids: readonly number[]): void {
  for (const pid of pids) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      continue;
    }
  }
}

export async function waitFor(what: string, check: () => boolean | Promise<boolean>, limitMs = WAIT_LIMIT_MS): Promise<void> {
  const deadline = Date.now() + limitMs;
  while (!(await check())) {
    if (Date.now() >= deadline) {
      throw new Error(`timed out after ${limitMs} ms waiting for ${what}`);
    }
    await Bun.sleep(20);
  }
}

export async function within<T>(what: string, limitMs: number, work: Promise<T>): Promise<T> {
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

export function installCleanup(): void {
  afterEach(async () => {
    await Promise.all(gates.splice(0).map((gate) => writeFile(gate, "")));
    const active = running.splice(0);
    const everything = active.flatMap((run) => [run.child.pid, ...descendants(run.child.pid)]);
    killAll(everything);
    await Promise.all(active.map((run) => run.child.exited));
    await waitFor(`processes ${everything.join(",")} to exit`, () => everything.every((pid) => !alive(pid)));
    await Promise.all(scratch.splice(0).map((path) => rm(path, { recursive: true, force: true })));
  });
}

export async function sandbox(): Promise<Box> {
  const root = await mkdtemp(join(realpathSync(tmpdir()), "velnor-runner-install-"));
  scratch.push(root);
  const home = join(root, "home");
  await mkdir(home);
  const store = join(root, "store");
  return { root, store, bin: join(store, "bin"), state: join(root, "orb-state.json"), log: join(root, "orb-log.jsonl"), home };
}

export const gitEnvironment = (box: Box): Record<string, string> => ({ PATH: "/usr/bin:/bin", HOME: box.home, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1" });

export function git(box: Box, dir: string, ...args: string[]): string {
  const done = Bun.spawnSync(["git", "-c", "user.name=test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false", "-C", dir, ...args], { env: gitEnvironment(box) });
  expect(done.exitCode, done.stderr.toString()).toBe(0);
  return done.stdout.toString();
}

export async function checkout(box: Box, name = "checkout"): Promise<Repo> {
  const dir = join(box.root, name);
  await mkdir(join(dir, "scripts"), { recursive: true });
  for (const file of INSTALLER_FILES) {
    await copyFile(join(SCRIPTS, file), join(dir, "scripts", file));
  }
  for (const file of COPIED_INPUTS) {
    await mkdir(dirname(join(dir, file)), { recursive: true });
    await cp(join(REPOSITORY, file), join(dir, file));
  }
  await writeFile(join(dir, "mise.lock"), "# lock\n");
  await writeFile(join(dir, "a.txt"), "a\n");
  await writeFile(join(dir, "b.txt"), "b\n");
  git(box, dir, "init", "-q");
  git(box, dir, "add", "-A");
  git(box, dir, "commit", "-q", "-m", "initial");
  return { dir, script: join(dir, "scripts/velnor-runner-install.ts") };
}

export const gateFile = (box: Box, name: string): string => {
  const gate = join(box.root, name);
  gates.push(gate);
  return gate;
};

async function fakeOrb(box: Box, who: string, built: string, extra: Extra): Promise<string> {
  launches += 1;
  const fakes = join(box.root, `fakes-${launches}`);
  await mkdir(fakes);
  await copyFile(FAKE_ORB, join(fakes, "orb"));
  await chmod(join(fakes, "orb"), 0o755);
  await writeFile(join(fakes, "config.json"), JSON.stringify({ state: box.state, log: box.log, who, bytes: built, mountInfo: MOUNT_INFO, ...extra }));
  if (extra.gitTamper !== undefined) {
    await copyFile(FAKE_GIT, join(fakes, "git"));
    await chmod(join(fakes, "git"), 0o755);
    await writeFile(join(fakes, "git-config.json"), JSON.stringify(extra.gitTamper));
  }
  const path = [fakes, dirname(process.execPath), "/usr/bin", "/bin"].join(":");
  expect(Bun.which("orb", { PATH: path })).toBe(join(fakes, "orb"));
  return path;
}

export const binary = (version: string): string => `#!fake-binary-${version}\nvelnor-runner ${version}\npayload-${version}\n`;
export const half = (text: string): string => text.slice(0, Math.floor(text.length / 2));

export async function start(box: Box, repo: Repo, built: string, extra: Extra = {}, who = "run", launch: Launch = {}): Promise<Running> {
  const command = [process.execPath, ...(launch.preload === undefined ? [] : ["--preload", launch.preload]), repo.script, ...(launch.args ?? [])];
  const child = Bun.spawn(command, {
    env: { PATH: await fakeOrb(box, who, built, extra), HOME: box.home, VELNOR_HOST_STORE: box.store, SSH_AUTH_SOCK: join(box.root, "agent.sock"), ORBENV: "LEAK" },
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

export const install = async (box: Box, repo: Repo, built: string, extra: Extra = {}, who = "run", launch: Launch = {}): Promise<Outcome> =>
  (await start(box, repo, built, extra, who, launch)).done;
export const entries = async (box: Box): Promise<string[]> => (await readdir(box.bin)).sort();
export const temps = async (box: Box): Promise<string[]> => (await readdir(box.bin)).filter((name) => name.startsWith(TEMP_PREFIX));
export const mode = async (path: string): Promise<number> => (await stat(path)).mode & 0o777;
export const machines = async (box: Box): Promise<string[]> => (JSON.parse(await Bun.file(box.state).text()) as { machines: string[] }).machines;
export const lockFile = (box: Box): string => join(box.home, ".velnor-runner-install", "install.lock");
export const settled = (): string[] => ["velnor-runner"];

export async function calls(box: Box): Promise<Call[]> {
  if (!(await Bun.file(box.log).exists())) {
    return [];
  }
  return (await Bun.file(box.log).text())
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line) as Call);
}
