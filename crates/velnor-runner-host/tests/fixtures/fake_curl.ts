#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { admin } from "./fake_github_admin.ts";
import { api } from "./fake_github_api.ts";
import { config, failed, unexpected, url } from "./fake_github_core.ts";
import { queue } from "./fake_github_queue.ts";

try {
  if (url.host === "api.github.com") api(url.pathname);
  if (url.host === new URL(config.admin_url).host) admin(url.pathname);
  if (url.host === new URL(config.queue_url).host) await queue(url.pathname);
  unexpected();
} catch (error) {
  failed(error);
}
