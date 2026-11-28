// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync } from "node:fs";
import {
  type Connection,
  body,
  config,
  connectionNamed,
  connectionOfSet,
  github,
  headers,
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
  setId,
  unexpected,
  url,
} from "./fake_github_core.ts";

const SETS = "/_apis/runtime/runnerscalesets";
const AGENTS = "/_apis/distributedtask/pools/0/agents";

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

function view(connection: Connection, labels: string[] = connection.labels): object {
  return {
    id: setId(connection),
    name: connection.scale_set,
    labels: labels.map((name) => ({ name, type: "System" })),
    runnerGroupId: 1,
    runnerSetting: { disableUpdate: true },
  };
}

function intended(connection: Connection): boolean {
  const directory = `${config.state}/rust-runner-scale-sets`;
  if (!config.state || !existsSync(directory)) return false;
  return readdirSync(directory)
    .filter((name) => !name.startsWith("."))
    .map((name) => JSON.parse(readFileSync(`${directory}/${name}`, "utf8")) as { name: string; scale_set_id: number | null })
    .some((record) => record.name === connection.scale_set && record.scale_set_id === null);
}

function caller(): Connection | undefined {
  return connectionNamed(/admin-token\.([^\s]+)$/.exec(headers.get("authorization") ?? "")?.[1]);
}

function sets(connection: Connection): never {
  if (method === "GET") {
    const scripted = scriptedReply("scaleset.get");
    const state = readState();
    const listed = state.scale_sets[connection.repo] !== undefined && !state.hidden_from_listing;
    if (state.hidden_from_listing) mutate((current) => (current.hidden_from_listing = false));
    record("scaleset.get", { repo: connection.repo, name: connection.scale_set, found: listed, status: scripted?.status ?? 200 });
    if (scripted?.status && scripted.status !== 200) reply(scripted.status, { message: "unavailable" });
    reply(200, { count: listed ? 1 : 0, value: listed ? [view(connection, config.existing_labels)] : [] });
  }
  if (method === "POST") {
    const refused = refusal("scaleset.create");
    const lost = refusal("scaleset.create.lost");
    record("scaleset.create", { repo: connection.repo, name: connection.scale_set, intent: intended(connection), status: refused?.status ?? lost?.status ?? 200 });
    if (refused) reply(refused.status, { message: "create refused" });
    mutate((state) => (state.scale_sets[connection.repo] = setId(connection)));
    if (lost) reply(lost.status, { message: "response lost" });
    reply(200, view(connection));
  }
  return unexpected();
}

function deleteSet(id: number): never {
  const target = connectionOfSet(id);
  if (!target) return unexpected();
  const refused = refusal("set.delete");
  const present = readState().scale_sets[target.repo] !== undefined;
  record("scaleset.delete", { repo: target.repo, id, status: refused?.status ?? (present ? 204 : 404) });
  if (refused?.status === 404 && refusal("set.hide")?.status === 1) mutate((state) => (state.hidden_from_listing = true));
  if (refused) reply(refused.status, { message: "scale set delete scripted" });
  if (!present) reply(404, { message: "scale set not found" });
  mutate((state) => {
    state.hidden_from_listing = false;
    delete state.scale_sets[target.repo];
    state.runners = state.runners.filter((runner) => runner.runnerScaleSetId !== id);
  });
  reply(204, undefined);
}

function byId(id: number): never {
  const target = connectionOfSet(id);
  const stall = refusal("set.stall");
  if (stall) Bun.sleepSync(stall.status);
  const scripted = refusal("set.get_id")?.status === 404;
  const found = !scripted && target !== undefined && readState().scale_sets[target.repo] === id;
  record("scaleset.get_id", { id, found, status: found ? 200 : 404 });
  if (!found || !target) reply(404, { message: "not found" });
  reply(200, view(target, config.existing_labels));
}

function sessions(id: number, owned: RegExpExecArray): void {
  const session = owned[3];
  if (method === "POST" && owned[2] === "sessions" && !session) {
    const created = mutate((state) => {
      const next = `session-${state.next_session++}`;
      state.sessions.push(next);
      return next;
    });
    record("session.create", { id: created, set: id });
    reply(200, { sessionId: created, messageQueueUrl: `${config.queue_url}/${connectionOfSet(id)?.repo}`, messageQueueAccessToken: `queue-token-${created}` });
  }
  if (method === "PATCH" && owned[2] === "sessions" && session) {
    const refused = refusal("session.refresh");
    const scripted = scriptedReply("session.refreshed");
    record("session.refresh", { id: session, status: refused?.status ?? 200 });
    if (refused) reply(refused.status, { message: "refresh refused" });
    reply(200, { sessionId: session, messageQueueUrl: `${config.queue_url}/${connectionOfSet(id)?.repo}`, messageQueueAccessToken: `queue-token-${session}-refreshed`, ...scripted?.patch });
  }
  if (method === "DELETE" && owned[2] === "sessions" && session) {
    const stall = refusal("session.stall");
    record("session.delete.begin", { set: id });
    if (stall) Bun.sleepSync(stall.status);
    held("session");
    const refused = refusal("session.delete");
    record("session.delete", { id: session, set: id, status: refused?.status ?? 204 });
    if (refused) reply(refused.status, { message: "session delete scripted" });
    mutate((state) => (state.sessions = state.sessions.filter((candidate) => candidate !== session)));
    reply(204, undefined);
  }
}

function jitAttempt(requestId: number): number {
  return mutate((state) => (state.attempts[`jit-${requestId}`] = (state.attempts[`jit-${requestId}`] ?? 0) + 1));
}

function jobs(id: number, owned: RegExpExecArray): void {
  if (method === "POST" && owned[2] === "acquirejobs") {
    const requested = JSON.parse(body) as number[];
    snapshot(record("acquire", { set: id, ids: requested.join(",") }));
    mutate((state) => (state.pending_jit = requested));
    held("acquire");
    const returned = (request: number): number[] => {
      const shape = offers().find((offer) => offer.request_id === request)?.acquire_returns;
      if (shape === "none") return [];
      if (shape === "duplicate") return [request, request];
      if (shape === "extra") return [request, request + 1000];
      return [request];
    };
    const value = requested.flatMap(returned);
    reply(200, { count: value.length, value });
  }
  if (method === "POST" && owned[2] === "generatejitconfig") {
    const { name } = JSON.parse(body) as { name: string };
    const canary = `JIT-CANARY-${config.canary_tag}-${name}`;
    writeFileSync(`${github}/jits/${name}`, canary);
    const runner = mutate((state) => {
      const created = { id: state.next_runner_id++, name, runnerScaleSetId: id };
      state.runners.push(created);
      return created;
    });
    const pending = readState().pending_jit ?? [];
    const scripted = offers().find((offer) => offer.request_id !== undefined && pending.includes(offer.request_id) && (offer.jit_failures !== undefined || offer.jit_identity !== undefined));
    const failing = scripted?.jit_failures !== undefined && jitAttempt(scripted.request_id) <= scripted.jit_failures;
    record("jit", { name, status: failing ? 500 : 200 });
    if (failing) reply(500, { message: "jit failed" });
    const wrong = { id: { ...runner, id: 0 }, scale_set: { ...runner, runnerScaleSetId: 99 }, name: { ...runner, name: `${name}-other` } };
    reply(200, { encodedJITConfig: canary, runner: scripted?.jit_identity ? wrong[scripted.jit_identity] : runner });
  }
}

export function admin(path: string): never {
  if (method === "GET" && path === "/_apis/runtime/runnergroups") reply(200, { count: 1, value: [{ id: 1, name: "Default", isDefaultGroup: true }] });
  if (path === SETS) {
    const connection = caller();
    return connection ? sets(connection) : unexpected();
  }
  const owned = new RegExp(`^${SETS}/(\\d+)(?:/(sessions|acquirejobs|generatejitconfig)(?:/([^/]+))?)?$`).exec(path);
  if (owned) {
    const id = Number(owned[1]);
    if (method === "DELETE" && !owned[2]) return deleteSet(id);
    if (method === "GET" && !owned[2]) return byId(id);
    sessions(id, owned);
    jobs(id, owned);
  }
  if (method === "GET" && path === AGENTS) {
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
