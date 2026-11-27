// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
import { spawnSync } from "node:child_process";
import { renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const USAGE =
  "usage: bun scripts/minimatch-corpus.ts <node binary, v24.22.1, the V8 of the shipped runner> <directory holding node_modules/minimatch@3.1.2, the version bundled in actions/runner v2.338.2> <output.json> [pattern count] [seed]";
const MAX_OUTPUT_BYTES = 256 * 1024 * 1024;

const [node, minimatchDirectory, output, ...options] = process.argv.slice(2);
if (node === undefined || minimatchDirectory === undefined || output === undefined) throw new Error(USAGE);
const evaluated = spawnSync(node, [join(import.meta.dirname, "minimatch-corpus-node.ts"), minimatchDirectory, ...options], {
  encoding: "utf8",
  maxBuffer: MAX_OUTPUT_BYTES,
  stdio: ["ignore", "pipe", "inherit"],
});
if (evaluated.error !== undefined) throw evaluated.error;
if (evaluated.status !== 0) throw new Error(`the Node evaluation of the corpus failed with status ${evaluated.status}`);
const partial = `${output}.partial`;
try {
  writeFileSync(partial, evaluated.stdout);
  renameSync(partial, output);
} catch (error) {
  rmSync(partial, { force: true });
  throw error;
}
