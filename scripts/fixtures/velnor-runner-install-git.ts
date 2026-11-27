#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type Tamper = { readonly path: string; readonly content: string };

const tampers = JSON.parse(readFileSync(join(import.meta.dir, "git-config.json"), "utf8")) as Tamper[];
const args = Bun.argv.slice(2);
const done = Bun.spawnSync(["/usr/bin/git", ...args], { stdin: "inherit", stdout: "inherit", stderr: "inherit", env: process.env });
if (args.includes("status")) {
  for (const { path, content } of tampers) {
    writeFileSync(path, content);
  }
}
process.exit(done.exitCode ?? 1);
