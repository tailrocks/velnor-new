#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";
import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type State = { machines: string[]; link: string | null; mise: boolean; packages: boolean; user: boolean; useraddTries: number; holder: number; shared: boolean; namespace: boolean; masked: string[]; busy: number };
type Failure = { readonly command: string; readonly operand?: string };
type Config = {
  readonly state: string;
  readonly log: string;
  readonly who: string;
  readonly bytes: string;
  readonly mountInfo: string;
  readonly copyCorrupt?: boolean;
  readonly truncateRead?: boolean;
  readonly swapAfterReap?: boolean;
  readonly survivors?: number;
  readonly rawTail?: readonly number[];
  readonly catFail?: boolean;
  readonly catStarted?: string;
  readonly catGate?: string;
  readonly list?: string;
  readonly hideMachines?: boolean;
  readonly failOn?: Failure;
  readonly miseBadDownload?: boolean;
  readonly tainted?: string;
  readonly useraddFailures?: number;
  readonly cargoOutsideWrapper?: boolean;
  readonly collectRejected?: boolean;
  readonly maskIneffective?: boolean;
};
type Call = { command: string; operands: string[]; user: string; assignments: Record<string, string>; outer: Record<string, string>; namespaced: boolean };

const GATE_LIMIT_MS = 30_000;
const MOUNT = "/mnt/mac";
const GUEST_STORE = "/var/lib/velnor";
const HOLDER = "/run/velnor-build-namespace";
const NAMESPACE = `${HOLDER}/mnt`;
const SECRET = "root-only secret\n";

const config = JSON.parse(readFileSync(join(import.meta.dir, "config.json"), "utf8")) as Config;
const fresh: State = { machines: [], link: null, mise: false, packages: false, user: false, useraddTries: 0, holder: 0, shared: false, namespace: false, masked: [], busy: 0 };
const state: State = existsSync(config.state) ? { ...fresh, ...(JSON.parse(readFileSync(config.state, "utf8")) as Partial<State>) } : fresh;
const save = (): void => writeFileSync(config.state, JSON.stringify(state));
const sha = (data: Uint8Array | string): string => createHash("sha256").update(data).digest("hex");
const fail = (message: string, status = 1): never => {
  process.stderr.write(`fake orb: ${message}\n`);
  process.exit(status);
};

const args = Bun.argv.slice(2);
const stdinBytes = async (): Promise<Uint8Array> => new Uint8Array(await Bun.stdin.arrayBuffer());
const record = (extra: Record<string, unknown> = {}): void =>
  appendFileSync(config.log, `${JSON.stringify({ who: config.who, args, env: Object.keys(process.env), ...extra })}\n`);

function hostPath(guest: string): string {
  if (state.link === null || !state.link.startsWith(MOUNT) || !guest.startsWith(`${GUEST_STORE}/`)) {
    return fail(`${guest} is not under the linked store`);
  }
  return `${state.link.slice(MOUNT.length)}${guest.slice(GUEST_STORE.length)}`;
}

async function copyOut(destination: string): Promise<void> {
  const target = hostPath(destination);
  const stored = readFileSync(`${config.state}.out`);
  const half = stored.subarray(0, Math.floor(stored.length / 2));
  writeFileSync(target, half);
  if (config.catFail === true) {
    process.exit(1);
  }
  if (config.catStarted !== undefined) {
    writeFileSync(config.catStarted, "");
  }
  const gate = config.catGate;
  const deadline = Date.now() + GATE_LIMIT_MS;
  while (gate !== undefined && !existsSync(gate) && Date.now() < deadline) {
    await Bun.sleep(20);
  }
  if (gate !== undefined && !existsSync(gate)) {
    fail("the gate never opened");
  }
  appendFileSync(target, stored.subarray(half.length));
  if (config.copyCorrupt === true) {
    appendFileSync(target, "!");
  }
}

const payload = (): Buffer => Buffer.concat([Buffer.from(config.bytes), Buffer.from(config.rawTail ?? [])]);

function readBuilt(operands: string[]): void {
  if (config.swapAfterReap !== true) {
    process.stdout.write(config.truncateRead === true ? payload().subarray(0, Math.floor(payload().length / 2)) : payload());
  } else if (operands.includes("iflag=nofollow")) {
    fail("dd: failed to open: Too many levels of symbolic links");
  } else {
    process.stdout.write(SECRET);
  }
}

function version(command: string): void {
  const host = hostPath(command);
  if (!existsSync(host)) {
    fail(`${host}: no such file`);
  }
  const line = readFileSync(host, "utf8")
    .split("\n")
    .find((candidate) => candidate.startsWith("velnor-runner "));
  if (line === undefined) {
    fail("not a velnor-runner binary");
  }
  process.stdout.write(`${line}\n`);
}

async function checkDigest(): Promise<void> {
  const line = new TextDecoder().decode(await stdinBytes()).trim();
  const [checkedSha = "", path = ""] = line.split(/\s+/);
  record({ checked: path, checkedSha });
  const good = path.includes("mise.download") ? config.miseBadDownload !== true : state.mise;
  process.exit(good ? 0 : 1);
}

function runMise(operands: string[], env: Record<string, string>): void {
  const tail = operands.slice(operands.indexOf("--") + 1).join(" ");
  if (tail.startsWith("cargo build")) {
    state.busy = config.survivors ?? 0;
    save();
  }
  if (operands[0] === "exec" && tail === "which -a cargo") {
    const wrapper = `${env.MISE_DATA_DIR ?? ""}/command-wrappers/bin/cargo`;
    process.stdout.write(config.cargoOutsideWrapper === true ? `/usr/bin/cargo\n${wrapper}\n` : `${wrapper}\n/somewhere/else/cargo\n`);
  } else if (operands[0] !== "install" && operands[0] !== "exec") {
    fail(`unexpected mise ${operands.join(" ")}`);
  }
}

function assignmentsOf(rest: string[]): { env: Record<string, string>; rest: string[] } {
  if (rest[0] !== "env" || rest[1] !== "-i") {
    return fail(`a guest command must start with env -i, got ${rest.slice(0, 3).join(" ")}`);
  }
  let tail = rest.slice(2);
  if (tail[0] === "-C") {
    tail = tail.slice(2);
  }
  const env: Record<string, string> = {};
  while (/^[A-Z_]+=/.test(tail[0] ?? "")) {
    const [name = "", ...value] = (tail.shift() ?? "").split("=");
    env[name] = value.join("=");
  }
  return { env, rest: tail };
}

function parse(rest: string[]): { call: Call; command: string; operands: string[] } {
  const outer = assignmentsOf(rest);
  let { env, rest: tail } = outer;
  let user = "root";
  let namespaced = false;
  if (tail[0] === "nsenter") {
    if (tail[1] !== `--mount=${NAMESPACE}` || tail[2] !== "--") {
      return fail(`nsenter must enter ${NAMESPACE}, got ${tail.slice(0, 3).join(" ")}`);
    }
    if (!state.namespace) {
      return fail("nsenter: reassociate to namespace 'ns/mnt' failed: Invalid argument");
    }
    namespaced = true;
    tail = tail.slice(3);
    if (tail[0] === "setpriv") {
      const switched = tail.indexOf("--");
      const options = tail.slice(1, switched);
      user = options.find((option) => option.startsWith("--reuid="))?.slice(8) ?? "";
      const required = [`--regid=${user}`, "--init-groups", "--no-new-privs", "--bounding-set=-all", "--inh-caps=-all"];
      if (!required.every((option) => options.includes(option))) {
        return fail(`setpriv is missing a hardening option: ${options.join(" ")}`);
      }
      const inner = assignmentsOf(tail.slice(switched + 1));
      env = inner.env;
      tail = inner.rest;
    }
  }
  const [command = "", ...operands] = tail;
  return { call: { command, operands, user, assignments: env, outer: outer.env, namespaced }, command, operands };
}

const matches = (failure: Failure, call: Call): boolean => call.command.endsWith(failure.command) && (failure.operand === undefined || call.operands.includes(failure.operand));

function findOutput(operands: string[]): string {
  if (operands.includes("-perm")) {
    return config.tainted ?? "";
  }
  if (operands.includes("-mindepth")) {
    const roots = operands.slice(0, operands.indexOf("-mindepth"));
    const exposed = roots.find((root) => !state.masked.includes(root));
    return exposed === undefined ? "" : `${exposed}/leaked\n`;
  }
  return config.collectRejected === true ? "" : `${operands[0] ?? ""}\n`;
}

function setUp(command: string, operands: string[]): boolean {
  if (command === "mountpoint") {
    process.exit(state.holder > 0 ? 0 : 32);
  } else if (command === "umount") {
    if (state.holder === 0) {
      fail("umount: not mounted", 1);
    }
    state.holder -= 1;
    if (state.holder === 0) {
      state.shared = false;
      state.namespace = false;
      state.masked = [];
    }
    save();
  } else if (command === "mount" && operands.includes("--make-private")) {
    if (state.holder === 0) {
      fail("mount: not a mount point", 32);
    }
    state.shared = false;
    save();
  } else if (command === "mount" && operands[operands.length - 1] === HOLDER) {
    state.holder += 1;
    state.shared = true;
    save();
  } else if (command === "unshare") {
    if (state.holder === 0 || state.shared || operands[0] !== `--mount=${NAMESPACE}`) {
      fail("unshare: mount /proc/1/ns/mnt failed: Invalid argument");
    }
    state.namespace = true;
    save();
  } else if (command === "mount") {
    if (state.masked.some((mask) => (operands[operands.length - 1] ?? "").startsWith(`${mask}/`))) {
      fail("mount: mount point does not exist");
    }
    if (config.maskIneffective !== true) {
      state.masked.push(operands[operands.length - 1] ?? "");
    }
    save();
  } else {
    return false;
  }
  return true;
}

async function guestCommand(rest: string[]): Promise<void> {
  const { call, command, operands } = parse(rest);
  const writes = command === "tar" || (command === "dd" && operands.some((operand) => operand.startsWith("of=")));
  const input = writes ? await stdinBytes() : undefined;
  record({ ...call, ...(input === undefined ? {} : { stdinSha256: sha(input) }) });
  if (config.failOn !== undefined && matches(config.failOn, call)) {
    fail("boom", 3);
  }
  if (call.user !== "root" && !call.namespaced) {
    fail(`${call.user} must run inside the build namespace`);
  }
  if (setUp(command, operands)) {
    return;
  }
  if (command === "readlink") {
    process.stdout.write(state.link ?? "");
    process.exit(state.link === null ? 1 : 0);
  } else if (command === "ln" && operands[0] === "-sfn") {
    state.link = operands[1] ?? null;
    save();
  } else if (command === "id") {
    process.exit(state.user ? 0 : 1);
  } else if (command === "useradd") {
    if (state.useraddTries < (config.useraddFailures ?? 0)) {
      state.useraddTries += 1;
      save();
      process.exit(1);
    }
    const known = state.user;
    state.user = true;
    save();
    process.exit(known ? 9 : 0);
  } else if (command === "dpkg") {
    process.exit(state.packages ? 0 : 1);
  } else if (command === "apt-get") {
    state.packages = state.packages || operands.includes("install");
    save();
  } else if (command === "sha256sum" && operands.includes("--check")) {
    await checkDigest();
  } else if (command === "sha256sum") {
    process.stdout.write(`${sha(payload())}  ${operands[0] ?? ""}\n`);
  } else if (command === "mv" && (operands[2] ?? "").endsWith("/bin/mise")) {
    state.mise = true;
    save();
  } else if (command === "find") {
    process.stdout.write(findOutput(operands));
  } else if (command === "cat" && operands[0] === "/proc/self/mountinfo") {
    process.stdout.write(config.mountInfo);
  } else if (command === "pkill") {
    const found = state.busy > 0;
    state.busy = Math.max(0, state.busy - 1);
    save();
    process.exit(found ? 0 : 1);
  } else if (command === "pgrep") {
    process.stdout.write(state.busy > 0 ? "4242\n" : "");
    process.exit(state.busy > 0 ? 0 : 1);
  } else if (command === "dd") {
    if (input === undefined) {
      readBuilt(operands);
    } else {
      writeFileSync(`${config.state}.out`, input);
    }
  } else if (command.endsWith("/mise")) {
    runMise(operands, call.assignments);
  } else if (command === "cp") {
    await copyOut(operands[1] ?? "");
  } else if (command.startsWith(`${GUEST_STORE}/`) && operands[0] === "--version") {
    version(command);
  } else if (!["install", "rm", "rmdir", "chmod", "mv", "curl", "ln", "tar", "touch"].includes(command)) {
    fail(`unexpected command ${command}`);
  }
}

if (args[0] === "list") {
  record();
  const listed = config.list ?? JSON.stringify(state.machines.map((name) => ({ name })));
  process.stdout.write(config.hideMachines === true ? "[]" : listed);
} else if (args[0] === "create") {
  record();
  const name = args[args.length - 1] ?? "";
  if (state.machines.includes(name)) {
    fail(`machine '${name}' already exists`);
  }
  state.machines.push(name);
  save();
} else if (args[0] === "-m") {
  if (!state.machines.includes(args[1] ?? "")) {
    fail("no such machine");
  }
  if (args[2] !== "-u" || args[3] !== "root") {
    fail(`every guest call must run -u root, got ${args.slice(2, 4).join(" ")}`);
  }
  await guestCommand(args.slice(4));
} else {
  fail(`unexpected arguments ${args.join(" ")}`);
}
