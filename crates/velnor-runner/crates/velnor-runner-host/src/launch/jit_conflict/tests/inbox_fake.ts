#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { appendFileSync } from "node:fs";

const directory = process.env.VELNOR_JIT_INBOX_FAKE;
if (!directory) throw new Error("missing isolated fake directory");
const config = await Bun.stdin.text();
const fields: Record<string, string> = {};
for (const line of config.split("\n")) {
  const match = /^(request|url) = "([^"\n]+)"$/.exec(line);
  if (match?.[1] && match[2]) fields[match[1]] = match[2];
}
const { request, url } = fields;
if (!url?.startsWith("https://queue.example.com/messages")) {
  throw new Error(`unexpected external URL: ${url}`);
}
appendFileSync(`${directory}/requests`, `${request} ${url}\n`);
if (request === "GET" && url.includes("?")) {
  const body = await Bun.file(`${directory}/body`).text();
  await Bun.write(Bun.stdout, `${JSON.stringify({ messageId: 80, messageType: "RunnerScaleSetJobMessages", body })}200`);
} else if (request === "DELETE" && url.endsWith("/80")) {
  await Bun.write(Bun.stdout, "204");
} else {
  throw new Error(`unexpected external request: ${request} ${url}`);
}
