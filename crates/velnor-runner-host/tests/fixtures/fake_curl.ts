#!/usr/bin/env bun
// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmdirSync,
  writeFileSync,
  writeSync,
} from "node:fs";

interface Config {
  owner: string;
  repo: string;
  repository_id: number;
  scale_set: string;
  labels: string[];
  canary_tag: string;
  queue_url: string;
  admin_url: string;
  store: string;
  ps_table: string;
}
interface Pull {
  number: number;
  base_sha: string;
  head_sha: string;
  base_ref: string;
  head_ref: string;
  merge_parents?: string[];
}
interface Offer {
  request_id: number;
  event: "push" | "pull_request";
  run_id: number;
  ref: string;
  branch: string;
  sha: string;
  workflow_content: string;
  pr?: Pull;
  acquire_returns?: "none";
}
interface Message {
  id: number;
  offers: Offer[];
}
interface Runner {
  id: number;
  name: string;
  runnerScaleSetId: number;
}
interface State {
  scale_set_id: number | null;
  sessions: string[];
  runners: Runner[];
  deleted: number[];
  attempts: Record<string, number>;
  next_runner_id: number;
  next_session: number;
}
interface Refusal {
  statuses: number[];
  repeat_last: boolean;
  until?: string;
}

const root = process.env.VELNOR_FAKE_DIR;
if (!root) throw new Error("VELNOR_FAKE_DIR is not set");
const github = `${root}/github`;
const seen = `${root}/seen`;
const config = JSON.parse(readFileSync(`${github}/config.json`, "utf8")) as Config;
const repository = `${config.owner}/${config.repo}`;
const input = await Bun.stdin.text();
const fields = new Map<string, string>();
const headers = new Map<string, string>();
for (const line of input.split("\n")) {
  const match = /^(request|url|data-binary|header) = "(.*)"$/.exec(line);
  if (!match?.[1] || match[2] === undefined) continue;
  if (match[1] === "header") {
    const split = match[2].indexOf(": ");
    headers.set(match[2].slice(0, split).toLowerCase(), match[2].slice(split + 2));
  } else {
    fields.set(match[1], match[2]);
  }
}
const method = fields.get("request") ?? "";
const url = new URL(fields.get("url") ?? "");
const bodyPath = fields.get("data-binary")?.replace(/^@/, "");
const body = bodyPath ? readFileSync(bodyPath, "utf8") : "";

function locked<T>(directory: string, work: () => T): T {
  for (;;) {
    try {
      mkdirSync(directory);
      break;
    } catch {
      Bun.sleepSync(5);
    }
  }
  try {
    return work();
  } finally {
    rmdirSync(directory);
  }
}

function record(label: string, detail: Record<string, string | number | boolean>): number {
  return locked(`${seen}/.lock`, () => {
    const counter = `${seen}/seq`;
    const next = (existsSync(counter) ? Number(readFileSync(counter, "utf8")) : 0) + 1;
    writeFileSync(counter, String(next));
    const text = Object.entries(detail)
      .map(([key, value]) => `${key}=${value}`)
      .join(" ");
    appendFileSync(`${seen}/requests`, `${next} ${Date.now()} GH ${label} ${text}\n`);
    return next;
  });
}

function reply(status: number, value: unknown): never {
  const text = value === undefined ? "" : typeof value === "string" ? value : JSON.stringify(value);
  writeSync(1, `${text}${status}`);
  process.exit(0);
}

function unexpected(): never {
  appendFileSync(`${seen}/unexpected`, `${method} ${url.href}\n`);
  process.stderr.write(`unexpected request: ${method} ${url.href}\n`);
  process.exit(3);
}

function messages(): Message[] {
  return readdirSync(`${github}/messages`)
    .filter((name) => name.endsWith(".json"))
    .map((name) => JSON.parse(readFileSync(`${github}/messages/${name}`, "utf8")) as Message)
    .sort((left, right) => left.id - right.id);
}

function offers(): Offer[] {
  return messages().flatMap((message) => message.offers);
}

function readState(): State {
  return JSON.parse(readFileSync(`${github}/state.json`, "utf8")) as State;
}

function mutate<T>(work: (state: State) => T): T {
  return locked(`${github}/.lock`, () => {
    const state = readState();
    const result = work(state);
    writeFileSync(`${github}/state.json.tmp`, JSON.stringify(state));
    renameSync(`${github}/state.json.tmp`, `${github}/state.json`);
    return result;
  });
}

function sha(text: string): string {
  return createHash("sha1").update(text).digest("hex");
}

function repo(): object {
  return {
    id: config.repository_id,
    name: config.repo,
    full_name: repository,
    owner: { login: config.owner },
    private: true,
    default_branch: "main",
  };
}

function runJson(offer: Offer): object {
  const pull = offer.pr;
  const short = { id: config.repository_id, name: config.repo };
  return {
    id: offer.run_id,
    event: offer.event,
    path: ".github/workflows/ci.yml",
    head_branch: pull ? pull.head_ref : offer.branch,
    head_sha: pull ? pull.head_sha : offer.sha,
    workflow_id: 7,
    run_attempt: 1,
    repository: repo(),
    head_repository: pull ? repo() : undefined,
    pull_requests: pull
      ? [
          {
            number: pull.number,
            head: { ref: pull.head_ref, sha: pull.head_sha, repo: short },
            base: { ref: pull.base_ref, sha: pull.base_sha, repo: short },
          },
        ]
      : [],
    referenced_workflows: [],
  };
}

function pullJson(pull: Pull, offer: Offer): object {
  return {
    number: pull.number,
    state: "open",
    merged: false,
    head: { ref: pull.head_ref, sha: pull.head_sha, repo: repo() },
    base: { ref: pull.base_ref, sha: pull.base_sha, repo: repo() },
    merge_commit_sha: offer.sha,
  };
}

function trees(offer: Offer): Record<string, object> {
  const content = Buffer.from(offer.workflow_content);
  const blob = createHash("sha1").update(`blob ${content.length}\0`).update(content).digest("hex");
  const [top, dot, flows] = [sha(`root:${offer.sha}`), sha(`gh:${offer.sha}`), sha(`wf:${offer.sha}`)];
  const entry = (path: string, mode: string, type: string, id: string, size?: number) => ({ path, mode, type, sha: id, size });
  return {
    [top]: { sha: top, truncated: false, tree: [entry(".github", "040000", "tree", dot)] },
    [dot]: { sha: dot, truncated: false, tree: [entry("workflows", "040000", "tree", flows)] },
    [flows]: { sha: flows, truncated: false, tree: [entry("ci.yml", "100644", "blob", blob, content.length)] },
    [blob]: { sha: blob, encoding: "base64", size: content.length, content: content.toString("base64") },
  };
}

function api(path: string): never {
  const base = `/repos/${repository}`;
  if (method === "POST" && path === `${base}/actions/runners/registration-token`) reply(201, { token: "registration-token" });
  if (method === "POST" && path === "/actions/runner-registration") reply(200, { url: config.admin_url, token: "admin-token" });
  if (method === "GET" && path === base) reply(200, repo());
  const run = /^\/repos\/[^/]+\/[^/]+\/actions\/runs\/(\d+)(?:\/attempts\/\d+)?$/.exec(path);
  if (method === "GET" && run?.[1]) {
    const offer = offers().find((candidate) => candidate.run_id === Number(run[1]));
    if (offer) reply(200, runJson(offer));
  }
  const pull = /^\/repos\/[^/]+\/[^/]+\/pulls\/(\d+)$/.exec(path);
  if (method === "GET" && pull?.[1]) {
    const offer = offers().find((candidate) => candidate.pr?.number === Number(pull[1]));
    if (offer?.pr) reply(200, pullJson(offer.pr, offer));
  }
  const commit = /^\/repos\/[^/]+\/[^/]+\/git\/commits\/([0-9a-f]{40})$/.exec(path);
  if (method === "GET" && commit?.[1]) {
    const offer = offers().find((candidate) => candidate.sha === commit[1]);
    if (offer) {
      const parents = offer.pr ? (offer.pr.merge_parents ?? [offer.pr.base_sha, offer.pr.head_sha]) : [];
      reply(200, { sha: offer.sha, message: "commit", tree: { sha: sha(`root:${offer.sha}`) }, parents: parents.map((parent) => ({ sha: parent })) });
    }
  }
  const object = /^\/repos\/[^/]+\/[^/]+\/git\/(?:trees|blobs)\/([0-9a-f]{40})$/.exec(path);
  if (method === "GET" && object?.[1]) {
    for (const offer of offers()) {
      const found = trees(offer)[object[1]];
      if (found) reply(200, found);
    }
  }
  return unexpected();
}

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

function refusal(name: string): { status: number } | undefined {
  const file = `${github}/script.json`;
  if (!existsSync(file)) return undefined;
  const entry = (JSON.parse(readFileSync(file, "utf8")) as Record<string, Refusal>)[name];
  if (!entry || (entry.until && existsSync(`${root}/${entry.until}`))) return undefined;
  const attempt = mutate((state) => (state.attempts[name] = (state.attempts[name] ?? 0) + 1));
  const index = attempt - 1;
  if (index < entry.statuses.length) return { status: entry.statuses[index] ?? 400 };
  return entry.repeat_last ? { status: entry.statuses[entry.statuses.length - 1] ?? 400 } : undefined;
}

function admin(path: string): never {
  const sets = "/_apis/runtime/runnerscalesets";
  const agents = "/_apis/distributedtask/pools/0/agents";
  if (method === "GET" && path === "/_apis/runtime/runnergroups") reply(200, { count: 1, value: [{ id: 1, name: "Default", isDefaultGroup: true }] });
  if (method === "GET" && path === sets) {
    const id = readState().scale_set_id;
    record("scaleset.get", { name: config.scale_set, found: id !== null });
    reply(200, { count: id === null ? 0 : 1, value: id === null ? [] : [scaleSetView(id)] });
  }
  if (method === "POST" && path === sets) {
    const id = mutate((state) => (state.scale_set_id = 7));
    record("scaleset.create", { name: config.scale_set });
    reply(200, scaleSetView(id));
  }
  const session = /^\/_apis\/runtime\/runnerscalesets\/(\d+)\/sessions(?:\/([^/]+))?$/.exec(path);
  if (session && method === "POST" && !session[2]) {
    const id = mutate((state) => {
      const next = `session-${state.next_session++}`;
      state.sessions.push(next);
      return next;
    });
    record("session.create", { id });
    reply(200, { sessionId: id, messageQueueUrl: config.queue_url, messageQueueAccessToken: `queue-token-${id}` });
  }
  if (session?.[2] && method === "DELETE") {
    mutate((state) => (state.sessions = state.sessions.filter((id) => id !== session[2])));
    record("session.delete", { id: session[2] });
    reply(204, undefined);
  }
  if (method === "POST" && /\/acquirejobs$/.test(path)) {
    const requested = JSON.parse(body) as number[];
    snapshot(record("acquire", { ids: requested.join(",") }));
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
    record("jit", { name });
    reply(200, { encodedJITConfig: canary, runner });
  }
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

async function queue(path: string): Promise<never> {
  if (method === "GET" && path === "/messages") {
    const last = Number(url.searchParams.get("lastMessageId") ?? 0);
    const deleted = new Set(readState().deleted);
    const pending = messages().filter((message) => !deleted.has(message.id));
    const next = pending.find((message) => last === 0 || message.id > last);
    record("poll", { capacity: headers.get("x-scalesetmaxcapacity") ?? "none", last: last === 0 ? "none" : last, returned: next?.id ?? "none" });
    if (!next) {
      await Bun.sleep(200);
      reply(202, undefined);
    }
    const jobs = next.offers.map((offer) => ({
      messageType: "JobAvailable",
      runnerRequestId: offer.request_id,
      jobId: `job-${offer.request_id}`,
      ownerName: config.owner,
      repositoryName: config.repo,
      jobWorkflowRef: `${repository}/.github/workflows/ci.yml@${offer.ref}`,
      workflowRunId: offer.run_id,
      eventName: offer.event,
      requestLabels: config.labels,
      scaleSetAssignTime: "2026-11-27T10:00:00Z",
    }));
    reply(200, { messageId: next.id, messageType: "RunnerScaleSetJobMessages", body: JSON.stringify(jobs) });
  }
  const message = /^\/messages\/(\d+)$/.exec(path);
  if (method === "DELETE" && message?.[1]) {
    mutate((state) => state.deleted.push(Number(message[1])));
    record("message.delete", { id: message[1] });
    reply(204, undefined);
  }
  return unexpected();
}

try {
  if (url.host === "api.github.com") api(url.pathname);
  if (url.host === new URL(config.admin_url).host) admin(url.pathname);
  if (url.host === new URL(config.queue_url).host) await queue(url.pathname);
  unexpected();
} catch (error) {
  appendFileSync(`${seen}/fake-failure`, `${method} ${url.href}: ${String(error)}\n`);
  process.exit(4);
}
