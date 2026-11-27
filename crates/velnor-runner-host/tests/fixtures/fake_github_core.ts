// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
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
import type { Offer } from "./fake_github_shapes.ts";

export interface Config {
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
  default_branch?: string;
  organization?: string;
  state?: string;
  existing_labels?: string[];
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
export interface State {
  scale_set_id: number | null;
  sessions: string[];
  runners: Runner[];
  deleted: number[];
  attempts: Record<string, number>;
  next_runner_id: number;
  next_session: number;
  pending_jit?: number[];
}
interface Refusal {
  statuses: number[];
  repeat_last: boolean;
  until?: string;
}
export interface Reply {
  status?: number;
  patch?: Record<string, unknown>;
  headers?: Record<string, string>;
}

const configured = process.env.VELNOR_FAKE_DIR;
if (!configured) throw new Error("VELNOR_FAKE_DIR is not set");
export const root: string = configured;
export const github = `${root}/github`;
export const seen = `${root}/seen`;
export const config = JSON.parse(readFileSync(`${github}/config.json`, "utf8")) as Config;
export const repository = `${config.owner}/${config.repo}`;
const input = await Bun.stdin.text();
const fields = new Map<string, string>();
export const headers = new Map<string, string>();
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
export const method = fields.get("request") ?? "";
export const url = new URL(fields.get("url") ?? "");
const bodyPath = fields.get("data-binary")?.replace(/^@/, "");
export const body = bodyPath ? readFileSync(bodyPath, "utf8") : "";

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

export function record(label: string, detail: Record<string, string | number | boolean>): number {
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

export function reply(status: number, value: unknown, headers: Record<string, string> = {}): never {
  const text = value === undefined ? "" : typeof value === "string" ? value : JSON.stringify(value);
  if (Object.keys(headers).length > 0) writeSync(2, `${headers["retry-after"] ?? ""}\n${headers["x-ratelimit-remaining"] ?? ""}`);
  writeSync(1, `${text}${status}`);
  process.exit(0);
}

export function unexpected(): never {
  appendFileSync(`${seen}/unexpected`, `${method} ${url.href}\n`);
  process.stderr.write(`unexpected request: ${method} ${url.href}\n`);
  process.exit(3);
}

export function failed(error: unknown): never {
  appendFileSync(`${seen}/fake-failure`, `${method} ${url.href}: ${String(error)}\n`);
  process.exit(4);
}

export function offers(): Offer[] {
  return messages().flatMap((message) => message.offers);
}

export function messages(): Message[] {
  return readdirSync(`${github}/messages`)
    .filter((name) => name.endsWith(".json"))
    .map((name) => JSON.parse(readFileSync(`${github}/messages/${name}`, "utf8")) as Message)
    .sort((left, right) => left.id - right.id);
}

export function readState(): State {
  return JSON.parse(readFileSync(`${github}/state.json`, "utf8")) as State;
}

export function mutate<T>(work: (state: State) => T): T {
  return locked(`${github}/.lock`, () => {
    const state = readState();
    const result = work(state);
    writeFileSync(`${github}/state.json.tmp`, JSON.stringify(state));
    renameSync(`${github}/state.json.tmp`, `${github}/state.json`);
    return result;
  });
}

export function phase(): string {
  const file = `${seen}/phase`;
  return existsSync(file) ? readFileSync(file, "utf8").trim() : "";
}

function scripted(name: string): string {
  const current = phase();
  return current ? `${name}@${current}` : name;
}

export function refusal(name: string): { status: number } | undefined {
  const file = `${github}/script.json`;
  if (!existsSync(file)) return undefined;
  const entry = (JSON.parse(readFileSync(file, "utf8")) as Record<string, Refusal>)[name];
  if (!entry || (entry.until && existsSync(`${root}/${entry.until}`))) return undefined;
  const attempt = mutate((state) => (state.attempts[name] = (state.attempts[name] ?? 0) + 1));
  const index = attempt - 1;
  if (index < entry.statuses.length) return { status: entry.statuses[index] ?? 400 };
  return entry.repeat_last ? { status: entry.statuses[entry.statuses.length - 1] ?? 400 } : undefined;
}

export function scriptedReply(base: string): Reply | undefined {
  const name = scripted(base);
  const file = `${github}/script.json`;
  if (!existsSync(file)) return undefined;
  const entry = (JSON.parse(readFileSync(file, "utf8")) as Record<string, { replies?: Reply[]; after_session?: boolean }>)[name];
  const replies = entry?.replies;
  if (!replies || (entry.after_session && readState().sessions.length === 0)) return undefined;
  const attempt = mutate((state) => (state.attempts[name] = (state.attempts[name] ?? 0) + 1));
  return replies[attempt - 1];
}

export function held(marker: string): void {
  for (let waited = 0; existsSync(`${seen}/hold-${marker}`) && !existsSync(`${seen}/release-${marker}`) && existsSync(root); waited += 1) {
    if (waited > 2400) break;
    Bun.sleepSync(50);
  }
}
