#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import {
  accessSync,
  appendFileSync,
  chmodSync,
  constants,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import { spawn } from "node:child_process";
import { basename } from "node:path";

const RUNNER = "/var/lib/velnor/bin/velnor-runner";
const GUEST = "/var/lib/velnor";
const root = import.meta.dir;
const settings = new Map<string, string>();
for (const line of readFileSync(`${root}/fake_orb.env`, "utf8").split("\n")) {
  const at = line.indexOf("=");
  if (at > 0) settings.set(line.slice(0, at), line.slice(at + 1));
}

function setting(name: string): string {
  return settings.get(name) ?? "";
}

const seen = setting("FAKE_ORB_SEEN");
const table = setting("FAKE_ORB_PS");
const exits: Array<() => void> = [];
let call = 0;

function finish(code: number): never {
  for (const hook of exits) hook();
  if (call > 0) writeFileSync(`${seen}/calls/${call}.status`, `${code}\n`);
  process.exit(code);
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

function gone(): boolean {
  const owner = setting("FAKE_TEST_PID");
  return !existsSync(root) || (owner !== "" && !alive(Number(owner)));
}

function lock(): void {
  const directory = `${seen}/.lock`;
  for (let tries = 0; ; ) {
    try {
      mkdirSync(directory);
      writeFileSync(`${directory}/owner`, String(process.pid));
      return;
    } catch {
      const owner = existsSync(`${directory}/owner`) ? Number(readFileSync(`${directory}/owner`, "utf8")) : 0;
      if (owner > 0 && !alive(owner)) {
        rmSync(directory, { recursive: true, force: true });
        continue;
      }
      if (gone()) finish(75);
      tries += 1;
      if (tries > 3000) finish(75);
      Bun.sleepSync(10);
    }
  }
}

function unlock(): void {
  rmSync(`${seen}/.lock`, { recursive: true, force: true });
}

function counter(name: string): number {
  lock();
  const file = `${seen}/${name}`;
  const next = (existsSync(file) ? Number(readFileSync(file, "utf8")) : 0) + 1;
  writeFileSync(file, `${next}\n`);
  unlock();
  return next;
}

function environment(): string {
  return `${Object.entries(process.env)
    .map(([key, value]) => `${key}=${value ?? ""}`)
    .join("\n")}\n`;
}

function inTable(pid: string): boolean {
  return existsSync(table) && readFileSync(table, "utf8").split("\n").some((line) => line.trimStart().startsWith(`${pid} `));
}

function dropFromTable(pid: string): void {
  lock();
  if (existsSync(table)) {
    const kept = readFileSync(table, "utf8")
      .split("\n")
      .filter((line) => line !== "" && !line.trimStart().startsWith(`${pid} `));
    writeFileSync(`${table}.tmp`, kept.map((line) => `${line}\n`).join(""));
    renameSync(`${table}.tmp`, table);
  }
  unlock();
}

function addToTable(line: string): void {
  lock();
  appendFileSync(table, `${line}\n`);
  unlock();
}

function hang(name: string): void {
  if (setting("FAKE_ORB_HANG") !== name) return;
  writeFileSync(`${seen}/hang.${call}.pid`, `${process.pid}\n`);
  while (!existsSync(`${seen}/unhang`) && !gone()) Bun.sleepSync(50);
  finish(70);
}

function holdStdout(): void {
  const mode = setting("FAKE_ORB_GRANDCHILD");
  if (mode === "") return;
  const child = spawn("sleep", ["30"], {
    stdio: ["ignore", "inherit", "ignore"],
    detached: mode === "escaped",
  });
  writeFileSync(`${seen}/grandchild.${call}.pid`, `${child.pid}\n`);
  child.unref();
}

function immortal(pid: string): boolean {
  if (existsSync(`${seen}/release`)) return false;
  return setting("FAKE_ORB_PS_IMMORTAL").split(",").includes(pid);
}

function text(value: string | undefined): string {
  return value ?? "";
}

function inGuest(args: string[]): never {
  let rest = args.slice(2);
  if (rest[0] === "-u") rest = rest.slice(2);
  if (rest[0] === "unshare") {
    const start = rest.indexOf(RUNNER);
    if (start < 0) finish(64);
    rest = rest.slice(start);
  }
  switch (rest[0]) {
    case "ln":
      return finish(0);
    case "test": {
      try {
        accessSync(`${setting("FAKE_ORB_STORE")}${text(rest[2]).replace(GUEST, "")}`, constants.X_OK);
      } catch {
        return finish(1);
      }
      return finish(0);
    }
    case "ps":
      return listProcesses();
    case "kill":
      for (const pid of rest.slice(2)) {
        appendFileSync(`${seen}/kills`, `${pid}\n`);
        if (!immortal(pid)) dropFromTable(pid);
      }
      return finish(0);
    case RUNNER:
      return runner(rest);
    default:
      return finish(64);
  }
}

function listProcesses(): never {
  const failures = Number(setting("FAKE_ORB_PS_FAIL") || "0");
  if (failures > 0 && inTable("7001")) {
    const file = `${seen}/ps.failed`;
    const failed = existsSync(file) ? Number(readFileSync(file, "utf8")) : 0;
    if (failed < failures) {
      writeFileSync(file, `${failed + 1}\n`);
      finish(1);
    }
  }
  const lines = [
    ...(setting("FAKE_ORB_PS_NO_INIT") === "" ? ["1 /sbin/init\n"] : []),
    ...(existsSync(table) ? [readFileSync(table, "utf8")] : []),
  ].join("");
  writeFileSync(`${seen}/calls/${call}.out`, lines);
  writeSync(1, lines);
  return finish(0);
}

function runner(rest: string[]): never {
  const jit = `${setting("FAKE_ORB_STORE")}${text(rest[3]).replace(`@${GUEST}`, "")}`;
  const work = `${setting("FAKE_ORB_STORE")}${text(rest[5]).replace(GUEST, "")}`;
  const name = basename(jit);
  copyFileSync(jit, `${seen}/${name}`);
  writeFileSync(`${seen}/${name}.env`, environment());
  writeFileSync(`${seen}/${name}.mode`, `${(statSync(jit).mode & 0o777).toString(8)}\n`);
  mkdirSync(work, { recursive: true });
  const standIn = setting("FAKE_RUNNER_PS_PID");
  addToTable(`${standIn === "" ? process.pid : standIn} ${rest.join(" ")}`);
  if (standIn === "") exits.push(() => dropFromTable(String(process.pid)));
  if (setting("FAKE_RUNNER_LOCK") !== "") {
    mkdirSync(`${work}/locked`, { recursive: true });
    writeFileSync(`${work}/locked/keep`, "");
    chmodSync(`${work}/locked`, 0o500);
  }
  const held = setting("FAKE_RUNNER_HOLD") !== "";
  const child = held ? Bun.spawn(["sleep", "60"], { stdin: "ignore", stdout: "ignore", stderr: "ignore" }) : undefined;
  if (child) writeFileSync(`${seen}/${name}.child`, `${child.pid}\n`);
  writeFileSync(`${seen}/${name}.pid`, `${process.pid}\n`);
  if (child) {
    while (!existsSync(`${seen}/release-${name}`) && !gone()) Bun.sleepSync(50);
    child.kill();
  }
  return finish(Number(setting("FAKE_RUNNER_EXIT") || "0"));
}

const args = process.argv.slice(2);
appendFileSync(setting("FAKE_ORB_LOG"), `${args.join(" ")}\n`);
mkdirSync(`${seen}/calls`, { recursive: true });
call = counter("calls.count");
if (setting("FAKE_REQUESTS") !== "") {
  lock();
  const file = `${seen}/seq`;
  const sequence = (existsSync(file) ? Number(readFileSync(file, "utf8")) : 0) + 1;
  writeFileSync(file, `${sequence}\n`);
  appendFileSync(setting("FAKE_REQUESTS"), `${sequence} - ORB ${call} ${args.join(" ")}\n`);
  unlock();
}
writeFileSync(`${seen}/calls/${call}.env`, environment());
switch (args[0]) {
  case "list":
    hang("list");
    holdStdout();
    writeSync(1, readFileSync(setting("FAKE_ORB_LIST")));
    finish(0);
  case "create":
    writeFileSync(setting("FAKE_ORB_LIST"), '[{"name":"velnor","image":{"distro":"ubuntu","arch":"arm64"}}]\n');
    finish(0);
  case "-m":
    inGuest(args);
  default:
    finish(64);
}
