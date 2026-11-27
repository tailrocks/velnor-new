#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";
import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type State = { machines: string[]; link: string | null; mise: boolean; packages: boolean; user: boolean };
type Config = {
  readonly state: string;
  readonly log: string;
  readonly who: string;
  readonly bytes: string;
  readonly guestSha?: string;
  readonly catFail?: boolean;
  readonly catStarted?: string;
  readonly catGate?: string;
  readonly list?: string;
  readonly hideMachines?: boolean;
  readonly failStep?: string;
  readonly miseBadDownload?: boolean;
  readonly tainted?: string;
};

const GATE_LIMIT_MS = 30_000;
const MOUNT = "/mnt/mac";
const GUEST_STORE = "/var/lib/velnor";

const config = JSON.parse(readFileSync(join(import.meta.dir, "config.json"), "utf8")) as Config;
const fresh: State = { machines: [], link: null, mise: false, packages: false, user: false };
const state: State = existsSync(config.state) ? (JSON.parse(readFileSync(config.state, "utf8")) as State) : fresh;
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
  const half = config.bytes.slice(0, Math.floor(config.bytes.length / 2));
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
  appendFileSync(target, config.bytes.slice(half.length));
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
  const path = line.split(/\s+/)[1] ?? "";
  record({ checked: path });
  const good = path.includes("mise.download") ? config.miseBadDownload !== true : state.mise;
  process.exit(good ? 0 : 1);
}

function runMise(operands: string[], env: Record<string, string>): void {
  const tail = operands.slice(operands.indexOf("--") + 1).join(" ");
  if (operands[0] === "exec" && tail === "which -a cargo") {
    process.stdout.write(`${env.MISE_DATA_DIR ?? ""}/command-wrappers/bin/cargo\n/somewhere/else/cargo\n`);
  } else if (operands[0] !== "install" && operands[0] !== "exec") {
    fail(`unexpected mise ${operands.join(" ")}`);
  }
}

async function guestCommand(rest: string[]): Promise<void> {
  const env: Record<string, string> = {};
  let user = "root";
  if (rest[0] === "env") {
    rest = rest.slice(2);
    if (rest[0] === "-C") {
      rest = rest.slice(2);
    }
    while (/^[A-Z_]+=/.test(rest[0] ?? "")) {
      const [name = "", ...value] = (rest.shift() ?? "").split("=");
      env[name] = value.join("=");
    }
  }
  if (rest[0] === "runuser") {
    user = rest[2] ?? "";
    rest = rest.slice(rest.indexOf("--") + 1);
  }
  if (rest[0] === "flock") {
    rest = rest.slice(4);
  }
  const [command = "", ...operands] = rest;
  record({ command, operands, user, assignments: env, ...(command === "tar" ? { stdinSha256: sha(await stdinBytes()) } : {}) });
  if (config.failStep !== undefined && rest.join(" ").includes(config.failStep)) {
    fail("boom", 3);
  }
  if (command === "readlink") {
    process.stdout.write(state.link ?? "");
    process.exit(state.link === null ? 1 : 0);
  } else if (command === "ln" && operands[0] === "-sfn") {
    state.link = operands[1] ?? null;
    save();
  } else if (command === "useradd") {
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
    process.stdout.write(`${config.guestSha ?? sha(config.bytes)}  ${operands[0] ?? ""}\n`);
  } else if (command === "mv" && (operands[2] ?? "").endsWith("/bin/mise")) {
    state.mise = true;
    save();
  } else if (command === "find") {
    process.stdout.write(operands.includes("-perm") ? (config.tainted ?? "") : `${operands[0] ?? ""}\n`);
  } else if (command === "pkill") {
    process.exit(1);
  } else if (command === "mise" || command.endsWith("/mise")) {
    runMise(operands, env);
  } else if (command === "cp") {
    await copyOut(operands[1] ?? "");
  } else if (command.startsWith(`${GUEST_STORE}/`) && operands[0] === "--version") {
    version(command);
  } else if (!["install", "rm", "chmod", "mv", "curl", "ln", "tar"].includes(command)) {
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
  const rest = args.slice(2);
  await guestCommand(rest[0] === "-u" ? rest.slice(2) : rest);
} else {
  fail(`unexpected arguments ${args.join(" ")}`);
}
