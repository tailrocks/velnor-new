// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { afterEach, expect, test } from "bun:test";
import { mkdir, mkdtemp, readdir, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const FAKE_ORB = `#!/usr/bin/env bun
"use strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
type State = { machines: string[]; link: string | null };
const env = (name: string): string => {
  const value = process.env[name];
  if (value === undefined) throw new Error("missing " + name);
  return value;
};
const statePath = env("FAKE_ORB_STATE");
const state: State = existsSync(statePath) ? JSON.parse(readFileSync(statePath, "utf8")) : { machines: [], link: null };
const save = (): void => writeFileSync(statePath, JSON.stringify(state));
const fail = (message: string): never => {
  process.stderr.write("fake orb: " + message + "\\n");
  process.exit(1);
};
const args = Bun.argv.slice(2);
if (args[0] === "list") {
  process.stdout.write(JSON.stringify(state.machines.map((name) => ({ name }))));
} else if (args[0] === "create") {
  const name = args[args.length - 1] ?? "";
  if (state.machines.includes(name)) fail("machine '" + name + "' already exists");
  state.machines.push(name);
  save();
} else if (args[0] === "-m") {
  if (!state.machines.includes(args[1] ?? "")) fail("no such machine");
  let rest = args.slice(2);
  if (rest[0] === "-u") rest = rest.slice(2);
  const [command = "", ...operands] = rest;
  if (command === "ln") {
    if (operands[0] !== "-sfn" || operands[2] !== "/var/lib/velnor") fail("unexpected ln " + operands.join(" "));
    state.link = operands[1] ?? null;
    save();
  } else if (command === "bash") {
    process.exit(0);
  } else if (command === "cat") {
    const bytes = env("FAKE_ORB_BYTES");
    const half = bytes.slice(0, Math.floor(bytes.length / 2));
    if (process.env.FAKE_ORB_CAT_FAIL !== undefined) {
      process.stdout.write(half);
      process.exit(1);
    }
    process.stdout.write(half);
    const started = process.env.FAKE_ORB_CAT_STARTED;
    if (started !== undefined) writeFileSync(started, "");
    const gate = process.env.FAKE_ORB_CAT_GATE;
    while (gate !== undefined && !existsSync(gate)) await Bun.sleep(20);
    process.stdout.write(bytes.slice(half.length));
  } else if (command.startsWith("/var/lib/velnor/") && operands[0] === "--version") {
    if (state.link === null || !state.link.startsWith("/mnt/mac")) fail("/var/lib/velnor is not linked to the host store");
    const host = state.link.slice("/mnt/mac".length) + command.slice("/var/lib/velnor".length);
    if (!existsSync(host)) fail(host + ": no such file");
    const line = readFileSync(host, "utf8").split("\\n").find((candidate) => candidate.startsWith("velnor-runner "));
    if (line === undefined) fail("not a velnor-runner binary");
    process.stdout.write(line + "\\n");
  } else {
    fail("unexpected command " + command);
  }
} else {
  fail("unexpected arguments " + args.join(" "));
}
`;

const SCRIPT = join(import.meta.dir, "velnor-runner-install.ts");
const scratch: string[] = [];
afterEach(async () => {
  await Promise.all(scratch.splice(0).map((path) => rm(path, { recursive: true, force: true })));
});

type Sandbox = { readonly store: string; readonly bin: string; readonly state: string; readonly fakes: string };
async function sandbox(): Promise<Sandbox> {
  const root = await mkdtemp(join(tmpdir(), "velnor-runner-install-"));
  scratch.push(root);
  const fakes = join(root, "fakes");
  await mkdir(fakes);
  await writeFile(join(fakes, "orb"), FAKE_ORB, { mode: 0o755 });
  const store = join(root, "store");
  return { store, bin: join(store, "bin"), state: join(root, "orb-state.json"), fakes };
}

const binary = (version: string): string => `#!fake-binary-${version}\nvelnor-runner ${version}\npayload-${version}\n`;

function launch(box: Sandbox, built: string, extra: Record<string, string> = {}) {
  const env: Record<string, string> = {
    ...(process.env as Record<string, string>),
    PATH: [box.fakes, dirname(process.execPath), process.env.PATH ?? ""].join(":"),
    VELNOR_HOST_STORE: box.store,
    FAKE_ORB_STATE: box.state,
    FAKE_ORB_BYTES: built,
  };
  const child = Bun.spawn([process.execPath, SCRIPT], { env: { ...env, ...extra }, stdin: "ignore", stdout: "pipe", stderr: "pipe" });
  return Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]).then(([stdout, stderr, status]) => ({
    stdout,
    stderr,
    status,
  }));
}

const install = (box: Sandbox, built: string, catFails = false) => launch(box, built, catFails ? { FAKE_ORB_CAT_FAIL: "1" } : {});
const half = (text: string): string => text.slice(0, Math.floor(text.length / 2));

const mode = async (path: string): Promise<number> => (await stat(path)).mode & 0o777;
const machines = async (box: Sandbox): Promise<string[]> => (JSON.parse(await Bun.file(box.state).text()) as { machines: string[] }).machines;

test("installing twice converges: same version, identical 755 binary, one machine, no temp files", async () => {
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
  expect(await readdir(box.bin)).toEqual(["velnor-runner"]);
  expect(await machines(box)).toEqual(["velnor"]);
});

test("a copy that fails partway keeps the old binary, prints no version and leaves no temp file", async () => {
  const box = await sandbox();
  const old = binary("0.1.81");
  expect(half(old)).not.toBe(half(binary("0.1.82")));
  expect((await install(box, old)).status).toBe(0);
  const failed = await install(box, binary("0.1.82"), true);
  expect(failed.status).not.toBe(0);
  expect(failed.stdout).toBe("");
  expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(old);
  expect(await readdir(box.bin)).toEqual(["velnor-runner"]);
});

test("a temp file from a crashed run is swept and an existing binary is replaced", async () => {
  const box = await sandbox();
  expect((await install(box, binary("0.1.81"))).status).toBe(0);
  await writeFile(join(box.bin, ".velnor-runner.tmp-99999"), "half-written");
  const replaced = await install(box, binary("0.1.82"));
  expect(replaced.status, replaced.stderr).toBe(0);
  expect(replaced.stdout).toBe("velnor-runner 0.1.82\n");
  expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
  expect(await readdir(box.bin)).toEqual(["velnor-runner"]);
});

test("a concurrent install waits for the lock and leaves the first run's temp file alone", async () => {
  const box = await sandbox();
  expect((await install(box, binary("0.1.80"))).status).toBe(0);
  const started = join(box.state, "..", "first-started");
  const gate = join(box.state, "..", "first-gate");
  const secondStarted = join(box.state, "..", "second-started");
  const first = launch(box, binary("0.1.81"), { FAKE_ORB_CAT_STARTED: started, FAKE_ORB_CAT_GATE: gate });
  await waitFor(() => Bun.file(started).exists(), 10_000);
  const second = launch(box, binary("0.1.82"), { FAKE_ORB_CAT_STARTED: secondStarted });
  await waitFor(() => Bun.file(secondStarted).exists(), 2_000);
  const midCopy = (await readdir(box.bin)).filter((name) => name.startsWith(".velnor-runner.tmp-"));
  expect(await Bun.file(secondStarted).exists()).toBe(false);
  expect(midCopy.length).toBe(1);
  await writeFile(gate, "");
  const [a, b] = await Promise.all([first, second]);
  expect(a.status, a.stderr).toBe(0);
  expect(b.status, b.stderr).toBe(0);
  expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
  expect(await readdir(box.bin)).toEqual(["velnor-runner"]);
});

async function waitFor(done: () => Promise<boolean>, limitMs: number): Promise<void> {
  const deadline = Date.now() + limitMs;
  while (Date.now() < deadline && !(await done())) await Bun.sleep(20);
}
