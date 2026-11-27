#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type Tamper = { readonly path: string; readonly content: string };
type Config = { readonly log: string; readonly tampers: readonly Tamper[] };

const config = JSON.parse(readFileSync(join(import.meta.dir, "git-config.json"), "utf8")) as Config;
const args = Bun.argv.slice(2);
const named = args[args.indexOf("--git-dir") + 1];
const directory = args.includes("--git-dir") ? named : args.includes("init") ? args[args.length - 1] : undefined;
if (directory !== undefined) {
  appendFileSync(config.log, `${directory}\n`);
}
const done = Bun.spawnSync(["/usr/bin/git", ...args], { stdin: "inherit", stdout: "inherit", stderr: "inherit", env: process.env });
if (args.includes("status")) {
  for (const { path, content } of config.tampers) {
    writeFileSync(path, content);
  }
}
process.exit(done.exitCode ?? 1);
