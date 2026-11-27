// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { existsSync, writeFileSync } from "node:fs";

const HOLD_LIMIT_MS = 30_000;
const [started = "", gate = ""] = Bun.argv.slice(2);
writeFileSync(started, "");
const deadline = Date.now() + HOLD_LIMIT_MS;
while (!existsSync(gate) && Date.now() < deadline) {
  await Bun.sleep(20);
}
