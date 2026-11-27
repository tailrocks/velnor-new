// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync } from "node:fs";
import {
  body,
  config,
  github,
  held,
  method,
  mutate,
  offers,
  readState,
  record,
  refusal,
  reply,
  scriptedReply,
  seen,
  unexpected,
  url,
} from "./fake_github_core.ts";

function listing(directory: string): string {
  return existsSync(directory) ? readdirSync(directory).sort().join(",") : "";
}

function snapshot(seq: number): void {
  mkdirSync(`${seen}/snapshots`, { recursive: true });
  const table = existsSync(config.ps_table) ? readFileSync(config.ps_table, "utf8") : "";
  const runners = readState().runners.map((runner) => runner.name).join(",");
  const text = `jit: ${listing(`${config.store}/jit`)}\nwork: ${listing(`${config.store}/work`)}\nrunners: ${runners}\nps:\n${table}`;
  writeFileSync(`${seen}/snapshots/${seq}.txt`, text);
}

function scaleSetView(id: number): object {
  return {
    id,
    name: config.scale_set,
    labels: config.labels.map((name) => ({ name, type: "System" })),
    runnerGroupId: 1,
    runnerSetting: { disableUpdate: true },
  };
}

function firstJit(requestId: number): boolean {
  return mutate((state) => (state.attempts[`jit-${requestId}`] = (state.attempts[`jit-${requestId}`] ?? 0) + 1)) === 1;
}

function sessions(path: string): void {
  const session = /^\/_apis\/runtime\/runnerscalesets\/(\d+)\/sessions(?:\/([^/]+))?$/.exec(path);
  if (!session) return;
  if (method === "POST" && !session[2]) {
    const id = mutate((state) => {
      const next = `session-${state.next_session++}`;
      state.sessions.push(next);
      return next;
    });
    record("session.create", { id });
    reply(200, { sessionId: id, messageQueueUrl: config.queue_url, messageQueueAccessToken: `queue-token-${id}` });
  }
  if (method === "PATCH" && session[2]) {
    const refused = refusal("session.refresh");
    const scripted = scriptedReply("session.refreshed");
    record("session.refresh", { id: session[2], status: refused?.status ?? 200 });
    if (refused) reply(refused.status, { message: "refresh refused" });
    reply(200, { sessionId: session[2], messageQueueUrl: config.queue_url, messageQueueAccessToken: `queue-token-${session[2]}-refreshed`, ...scripted?.patch });
  }
  if (method === "DELETE" && session[2]) {
    mutate((state) => (state.sessions = state.sessions.filter((id) => id !== session[2])));
    record("session.delete", { id: session[2] });
    reply(204, undefined);
  }
}

function jobs(path: string): void {
  if (method === "POST" && /\/acquirejobs$/.test(path)) {
    const requested = JSON.parse(body) as number[];
    snapshot(record("acquire", { ids: requested.join(",") }));
    mutate((state) => (state.pending_jit = requested));
    held("acquire");
    const none = new Set(offers().filter((offer) => offer.acquire_returns === "none").map((offer) => offer.request_id));
    const value = requested.filter((id) => !none.has(id));
    reply(200, { count: value.length, value });
  }
  if (method === "POST" && /\/generatejitconfig$/.test(path)) {
    const { name } = JSON.parse(body) as { name: string };
    const canary = `JIT-CANARY-${config.canary_tag}-${name}`;
    writeFileSync(`${github}/jits/${name}`, canary);
    const runner = mutate((state) => {
      const created = { id: state.next_runner_id++, name, runnerScaleSetId: state.scale_set_id ?? 0 };
      state.runners.push(created);
      return created;
    });
    const failing = offers().some((offer) => offer.jit_returns === 500 && readState().pending_jit?.includes(offer.request_id) && firstJit(offer.request_id));
    record("jit", { name, status: failing ? 500 : 200 });
    if (failing) reply(500, { message: "jit failed" });
    reply(200, { encodedJITConfig: canary, runner });
  }
}

export function admin(path: string): never {
  const sets = "/_apis/runtime/runnerscalesets";
  const agents = "/_apis/distributedtask/pools/0/agents";
  if (method === "GET" && path === "/_apis/runtime/runnergroups") reply(200, { count: 1, value: [{ id: 1, name: "Default", isDefaultGroup: true }] });
  if (method === "GET" && path === sets) {
    const scripted = scriptedReply("scaleset.get");
    const id = readState().scale_set_id;
    record("scaleset.get", { name: config.scale_set, found: id !== null, status: scripted?.status ?? 200 });
    if (scripted?.status && scripted.status !== 200) reply(scripted.status, { message: "unavailable" });
    reply(200, { count: id === null ? 0 : 1, value: id === null ? [] : [scaleSetView(id)] });
  }
  if (method === "POST" && path === sets) {
    const id = mutate((state) => (state.scale_set_id = 7));
    record("scaleset.create", { name: config.scale_set });
    reply(200, scaleSetView(id));
  }
  sessions(path);
  jobs(path);
  if (method === "GET" && path === agents) {
    const wanted = url.searchParams.get("agentName");
    const found = readState().runners.filter((runner) => wanted === null || runner.name === wanted);
    if (wanted === null) record("runner.list", { names: found.map((runner) => runner.name).join(",") });
    else record("runner.get", { name: wanted, found: found.length > 0 });
    reply(200, { count: found.length, value: found });
  }
  const runner = /\/agents\/(\d+)$/.exec(path);
  if (method === "DELETE" && runner?.[1]) {
    const target = readState().runners.find((candidate) => candidate.id === Number(runner[1]));
    if (!target) reply(204, undefined);
    const refused = refusal(target.name);
    record("runner.delete", { name: target.name, status: refused?.status ?? 204 });
    if (refused) reply(refused.status, { exceptionName: "JobStillRunningException", message: "job is running" });
    mutate((state) => (state.runners = state.runners.filter((candidate) => candidate.id !== target.id)));
    reply(204, undefined);
  }
  return unexpected();
}
