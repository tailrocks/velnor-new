#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type State = { machines: string[]; link: string | null };
type Config = {
  readonly state: string;
  readonly bytes: string;
  readonly catFail?: boolean;
  readonly catStarted?: string;
  readonly catGate?: string;
};

const GATE_LIMIT_MS = 30_000;

const config = JSON.parse(readFileSync(join(import.meta.dir, "config.json"), "utf8")) as Config;
const statePath = config.state;
const state: State = existsSync(statePath) ? (JSON.parse(readFileSync(statePath, "utf8")) as State) : { machines: [], link: null };
const save = (): void => writeFileSync(statePath, JSON.stringify(state));
const fail = (message: string): never => {
  process.stderr.write(`fake orb: ${message}\n`);
  process.exit(1);
};

async function copyOut(): Promise<void> {
  const bytes = config.bytes;
  const half = bytes.slice(0, Math.floor(bytes.length / 2));
  process.stdout.write(half);
  if (config.catFail === true) {
    process.exit(1);
  }
  const started = config.catStarted;
  if (started !== undefined) {
    writeFileSync(started, "");
  }
  const gate = config.catGate;
  const deadline = Date.now() + GATE_LIMIT_MS;
  while (gate !== undefined && !existsSync(gate) && Date.now() < deadline) {
    await Bun.sleep(20);
  }
  if (gate !== undefined && !existsSync(gate)) {
    fail("the gate never opened");
  }
  process.stdout.write(bytes.slice(half.length));
}

function version(command: string): void {
  if (state.link === null || !state.link.startsWith("/mnt/mac")) {
    fail("/var/lib/velnor is not linked to the host store");
  }
  const host = `${state.link?.slice("/mnt/mac".length)}${command.slice("/var/lib/velnor".length)}`;
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

const args = Bun.argv.slice(2);
if (args[0] === "list") {
  process.stdout.write(JSON.stringify(state.machines.map((name) => ({ name }))));
} else if (args[0] === "create") {
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
  let rest = args.slice(2);
  if (rest[0] === "-u") {
    rest = rest.slice(2);
  }
  const [command = "", ...operands] = rest;
  if (command === "ln") {
    if (operands[0] !== "-sfn" || operands[2] !== "/var/lib/velnor") {
      fail(`unexpected ln ${operands.join(" ")}`);
    }
    state.link = operands[1] ?? null;
    save();
  } else if (command === "bash") {
    await Bun.stdin.arrayBuffer();
  } else if (command === "cat") {
    await copyOut();
  } else if (command.startsWith("/var/lib/velnor/") && operands[0] === "--version") {
    version(command);
  } else {
    fail(`unexpected command ${command}`);
  }
} else {
  fail(`unexpected arguments ${args.join(" ")}`);
}
