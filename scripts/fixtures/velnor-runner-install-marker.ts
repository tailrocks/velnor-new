// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { writeFileSync } from "node:fs";

writeFileSync(Bun.argv[2] ?? "", "");
if (Bun.argv[3] === "filter") {
  process.stdout.write(new Uint8Array(await Bun.stdin.arrayBuffer()));
}
