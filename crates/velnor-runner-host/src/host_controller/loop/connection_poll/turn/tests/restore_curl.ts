#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";

import { existsSync, writeFileSync } from "node:fs";

const root = process.env["VELNOR_RESTORE_TIMEOUT_FIXTURE"];
if (!root) throw new Error("missing fixture directory");
const config = await Bun.stdin.text();
const url = config.match(/^url = "([^"]+)"/m)?.[1];
if (!url) throw new Error("missing URL");
let body: unknown;
let status = 200;
if (url.includes("/repos/owner/repo")) {
  body = { id: 42, name: "repo", full_name: "owner/repo", private: true, owner: { login: "owner" } };
} else if (url.includes("/actions/runner-registration")) {
  body = { url: "https://actions.test", token: "admin-test" };
} else if (url.includes("/sessions/session-test")) {
  const marker = `${root}/refresh-started`;
  if (!existsSync(marker)) {
    writeFileSync(marker, "started");
    const began = Date.now();
    while (!existsSync(`${root}/journal-held`)) {
      if (Date.now() - began >= 30_000) throw new Error("journal guard was not acquired");
      await Bun.sleep(2);
    }
    await Bun.sleep(11_000);
  }
  body = { sessionId: "session-test", messageQueueUrl: "https://actions.test/queue", messageQueueAccessToken: "queue-test" };
} else if (url.includes("/queue")) {
  status = 202;
} else {
  throw new Error(`unexpected URL ${url}`);
}
process.stdout.write(`${body === undefined ? "" : JSON.stringify(body)}${status}`);
