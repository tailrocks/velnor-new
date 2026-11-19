// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

"use strict";

import { appendFileSync, existsSync, writeFileSync } from "node:fs";

const config: string = await Bun.stdin.text();
const log: string | undefined = process.env.VELNOR_RETIREMENT_CURL_LOG;
const gate: string | undefined = process.env.VELNOR_RETIREMENT_CURL_GATE;
if (!log) throw new Error("missing request log");
appendFileSync(log, config + "\n---\n");
if (config.includes("/repos/owner/repo")) {
  process.stdout.write(JSON.stringify({
    id: 42, name: "repo", full_name: "owner/repo",
    private: true, owner: { login: "owner" },
  }) + "200");
} else if (config.includes("/actions/runner-registration")) {
  process.stdout.write('{"url":"https://queue.example.test","token":"admin-token"}200');
} else if (config.includes('request = "POST"') && config.includes("/sessions")) {
  process.stdout.write('{"sessionId":"session-other","messageQueueUrl":"https://queue.example.test/messages","messageQueueAccessToken":"queue-token"}200');
} else if (config.includes('request = "GET"') && config.includes("/messages")) {
  if (gate) {
    writeFileSync(`${gate}.waiting`, "");
    while (!existsSync(`${gate}.released`)) await Bun.sleep(5);
  }
  process.stdout.write("202");
} else {
  throw new Error("unexpected request " + config);
}
