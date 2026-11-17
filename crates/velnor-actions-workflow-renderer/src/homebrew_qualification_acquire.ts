// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";

import { createHash, randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
function object(value: Json): { [key: string]: Json } {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw Error("object required");
  return value;
}
function text(value: Json | undefined): string {
  if (typeof value !== "string" || value.length === 0) throw Error("string required");
  return value;
}
function integer(value: Json | undefined): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value <= 0) throw Error("positive integer required");
  return value;
}
function array(value: Json | undefined): Json[] {
  if (!Array.isArray(value)) throw Error("array required");
  return value;
}
function sha(value: Json | undefined): string {
  const result = text(value);
  if (!/^[0-9a-f]{40}$/.test(result)) throw Error("commit required");
  return result;
}
const repository = "repos/jackin-project/jackin";
const acceptJson = "application/vnd.github+json";
const root = process.env["VELNOR_HOMEBREW_EVIDENCE_DIR"];
const token = process.env["GH_TOKEN"];
const deadline = Number(process.env["VELNOR_HOME_BUDGET_DEADLINE_EPOCH"]) * 1000;
if (!root || !token || !Number.isSafeInteger(deadline) || deadline <= Date.now()) throw Error("acquisition environment required");
const evidenceRoot = root;
await mkdir(evidenceRoot, { mode: 0o700 });
const key = (accept: string, route: string): string => createHash("sha256").update(`${accept}\n${route}`).digest("hex");
let total = 0;
let requests = 0;
const pageLinks = new Map<string, string>();
async function store(accept: string, route: string, bytes: Uint8Array): Promise<void> {
  total += bytes.length;
  if (total > 12 * 1024 ** 3) throw Error("evidence size bound");
  await writeFile(join(evidenceRoot, key(accept, route)), bytes, { flag: "wx", mode: 0o600 });
}
function storage(url: URL): boolean {
  return url.protocol === "https:" && url.username === "" && url.password === "" && (url.port === "" || url.port === "443") &&
    (url.hostname.endsWith(".blob.core.windows.net") || url.hostname.endsWith(".actions.githubusercontent.com") || url.hostname === "release-assets.githubusercontent.com");
}
async function download(url: URL, accept: string, limit: number, route?: string): Promise<Uint8Array> {
  for (let redirects = 0; redirects <= 3; redirects++) {
    if (++requests > 2000) throw Error("request bound");
    const remaining = Math.min(90_000, deadline - Date.now());
    if (remaining <= 0) throw Error("acquisition deadline");
    if (url.username !== "" || url.password !== "") throw Error("URL credentials forbidden");
    const api = url.origin === "https://api.github.com" && (url.pathname === `/${repository}` || url.pathname.startsWith(`/${repository}/`));
    if (!api && !storage(url)) throw Error("foreign evidence URL");
    const response = await fetch(url, {
      redirect: "manual", signal: AbortSignal.timeout(remaining),
      headers: api ? { Accept: accept, Authorization: `Bearer ${token}`, "X-GitHub-Api-Version": "2026-03-10", "Cache-Control": "no-cache, no-store", Pragma: "no-cache", "Accept-Encoding": "identity" } : { "Accept-Encoding": "identity" },
    });
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location) throw Error("redirect location");
      url = new URL(location, url);
      continue;
    }
    if (response.status !== 200 || !response.body) throw Error(`evidence status ${response.status}`);
    if (route && api) pageLinks.set(route, response.headers.get("link") ?? "");
    const reader = response.body.getReader();
    const chunks: Uint8Array[] = [];
    let size = 0;
    for (;;) {
      const result = await reader.read();
      if (result.done) break;
      size += result.value.length;
      if (size > limit || Date.now() >= deadline) { await reader.cancel(); throw Error("response bound"); }
      chunks.push(result.value);
    }
    return Buffer.concat(chunks, size);
  }
  throw Error("redirect bound");
}
async function request(route: string, accept = acceptJson, limit = 1024 ** 2): Promise<Uint8Array> {
  if (!(route === repository || route.startsWith(`${repository}/`)) || route.includes("#")) throw Error("foreign route");
  const url = new URL(`https://api.github.com/${route}`);
  url.searchParams.set("_velnor_read_nonce", randomUUID());
  return download(url, accept, limit, route);
}
const observed = new Map<string, Uint8Array>();
async function capture(route: string, accept = acceptJson, limit = 1024 ** 2): Promise<Uint8Array> {
  const identity = key(accept, route);
  const found = observed.get(identity);
  if (found) return found;
  const bytes = await request(route, accept, limit);
  await store(accept, route, bytes);
  if (limit <= 8 * 1024 ** 2) observed.set(identity, bytes);
  return bytes;
}
const decode = (bytes: Uint8Array): Json => JSON.parse(Buffer.from(bytes).toString("utf8"));
const json = async (suffix: string): Promise<Json> => decode(await capture(`${repository}/${suffix}`));
type MutableKind = "branch" | "release" | "refs" | "tag" | "assets";
function project(value: Json, kind: MutableKind): Json {
  if (kind === "refs") return array(value).map(entry => {
    const item = object(entry);
    const target = object(item["object"] ?? null);
    return [item["ref"] ?? null, target["sha"] ?? null, target["type"] ?? null];
  });
  if (kind === "assets") return array(value).map(entry => {
    const item = object(entry);
    return [item["id"] ?? null, item["name"] ?? null, item["size"] ?? null, item["digest"] ?? null, item["state"] ?? null];
  });
  const item = object(value);
  if (kind === "branch") return [item["name"] ?? null, item["protected"] ?? null, object(item["commit"] ?? null)["sha"] ?? null];
  if (kind === "release") return [item["id"] ?? null, item["tag_name"] ?? null, item["draft"] ?? null, item["prerelease"] ?? null, item["immutable"] ?? null, item["body"] ?? null];
  const target = object(item["object"] ?? null);
  return [item["sha"] ?? null, item["tag"] ?? null, item["message"] ?? null, target["sha"] ?? null, target["type"] ?? null];
}
const mutable = new Map<string, { bytes: Uint8Array; kind: MutableKind }>();
async function mutableJson(suffix: string, kind: MutableKind): Promise<Json> {
  const route = `${repository}/${suffix}`;
  const bytes = await capture(route);
  mutable.set(route, { bytes, kind });
  return decode(bytes);
}
async function ancestry(base: string, head: string): Promise<void> {
  if (base === head) return;
  const route = `compare/${base}...${head}?per_page=1&page=2`;
  const comparison = object(await json(route));
  const count = integer(comparison["total_commits"]);
  if (count > 1_000_000) throw Error("compare bound");
  if (count === 1) await json(`git/commits/${head}`);
  else await json(`compare/${base}...${head}?per_page=1&page=${count}`);
}
async function jobs(id: number, attempt: number): Promise<void> {
  for (let page = 1; page <= 20; page++) {
    const result = object(await json(`actions/runs/${id}/attempts/${attempt}/jobs?per_page=100&page=${page}`));
    const entries = array(result["jobs"]);
    if (entries.length > 100 || integer(result["total_count"]) > 2000) throw Error("jobs bound");
    if (entries.length < 100) return;
  }
  throw Error("jobs pages");
}
async function run(id: number, attempt: number): Promise<void> {
  await json(`actions/runs/${id}/attempts/${attempt}`);
  await jobs(id, attempt);
}
async function attestations(digest: string): Promise<void> {
  const identity = `attestations/sha256:${digest}`;
  if (observed.has(key(acceptJson, `${repository}/${identity}`))) return;
  const pages: Json[] = [];
  let bytesTotal = 0;
  for (let page = 1; page <= 20; page++) {
const route = `${repository}/${identity}?per_page=100&predicate_type=https%3A%2F%2Fslsa.dev%2Fprovenance%2Fv1&page=${page}`;
    const raw = await request(route, acceptJson, 4 * 1024 ** 2);
    bytesTotal += raw.length;
    if (bytesTotal > 4 * 1024 ** 2) throw Error("attestations bound");
    const result = decode(raw);
    const entries = array(object(result)["attestations"]);
    if (entries.length > 100) throw Error("attestations page bound");
    for (const entry of entries) {
      const item = object(entry);
      if (item["bundle"] == null) {
        const url = text(item["bundle_url"]);
        if (!storage(new URL(url))) throw Error("foreign bundle URL");
        const bundle = await download(new URL(url), "application/octet-stream", 4 * 1024 ** 2);
        const name = key("bundle", url);
        if (!observed.has(name)) { await store("bundle", url, bundle); observed.set(name, bundle); }
      }
    }
    pages.push(result);
    const next = /<([^>]+)>;\s*rel="next"/.exec(pageLinks.get(route) ?? "");
    if (next) {
      const nextUrl = new URL(text(next[1]));
      if (nextUrl.origin !== "https://api.github.com" || nextUrl.pathname !== `/${repository}/${identity}` ||
          nextUrl.searchParams.get("page") !== String(page + 1) || nextUrl.searchParams.get("per_page") !== "100" ||
          nextUrl.searchParams.get("predicate_type") !== "https://slsa.dev/provenance/v1") throw Error("attestation pagination identity");
    } else {
      const bytes = Buffer.from(JSON.stringify(pages));
      await store(acceptJson, `${repository}/${identity}`, bytes);
      observed.set(key(acceptJson, `${repository}/${identity}`), bytes);
      return;
    }
  }
  throw Error("attestations pages");
}
await capture(repository);
const branch = object(await mutableJson("branches/main", "branch"));
const tip = sha(object(branch["commit"] ?? null)["sha"]);
const records = array(JSON.parse("__VELNOR_HOMEBREW_CATALOG__"));
const authorities = new Set<string>();
const runs = new Map<number, number>();
for (const raw of records) {
  const record = object(raw);
  const release = object(record["release"] ?? null);
  const producer = object(record["producer"] ?? null);
  const source = sha(release["source_sha"]);
  const authority = sha(producer["authority_sha"]);
  authorities.add(source);
  authorities.add(authority);
  await ancestry(source, authority);
  const entries = [record["source_ci"], record["producer"]];
  if (record["recovery"] != null) {
    const recovery = object(record["recovery"]);
    const recoveryAuthority = sha(recovery["authority_sha"]);
    authorities.add(recoveryAuthority);
    await ancestry(authority, recoveryAuthority);
    entries.push(record["recovery"]);
  }
  for (const entry of entries) {
    const identity = object(entry ?? null);
    const id = integer(identity["run_id"]);
    const attempt = integer(identity["attempt"]);
    if (runs.has(id) && runs.get(id) !== attempt) throw Error("run attempt collision");
    runs.set(id, attempt);
  }
}
for (const commit of [...authorities, tip]) await capture(`${repository}/commits/${commit}`, "application/vnd.github.sha");
for (const authority of authorities) await ancestry(authority, tip);
for (const filename of ["binary-preview.yml", "ci.yml"]) await json(`actions/workflows/${filename}`);
for (const [id, attempt] of runs) await run(id, attempt);
for (const rawRecord of records) {
  const record = object(rawRecord);
  const expectedRelease = object(record["release"] ?? null);
  const tag = text(expectedRelease["tag"]);
  if (!/^preview-[0-9a-f]{40}$/.test(tag)) throw Error("historical tag");
  const release = object(await mutableJson(`releases/tags/${tag}`, "release"));
  const id = integer(release["id"]);
  if (id !== integer(expectedRelease["id"])) throw Error("historical release id");
  const refs = array(await mutableJson(`git/matching-refs/tags/${tag}`, "refs"));
  for (const ref of refs) {
    const target = object(object(ref)["object"] ?? null);
    if (target["type"] === "tag") await mutableJson(`git/tags/${sha(target["sha"])}`, "tag");
  }
  const digests = new Map<string, string>();
  let receipt: Json = null;
  for (let page = 1; page <= 42; page++) {
    const assets = array(await mutableJson(`releases/${id}/assets?per_page=100&page=${page}`, "assets"));
    if (assets.length > 100) throw Error("asset page bound");
    for (const raw of assets) {
      const asset = object(raw);
      const size = integer(asset["size"]);
      if (size > 1024 ** 3) throw Error("asset size bound");
      const bytes = await capture(`${repository}/releases/assets/${integer(asset["id"])}`, "application/octet-stream", size);
      if (bytes.length !== size) throw Error("asset size mismatch");
      const name = text(asset["name"]);
      if (digests.has(name)) throw Error("duplicate release asset");
      digests.set(name, createHash("sha256").update(bytes).digest("hex"));
      if (name === "release.json") receipt = decode(bytes);
    }
    if (assets.length < 100) break;
    if (page === 42) throw Error("asset pages");
  }
for (const name of array(object(object(receipt)["evidence"] ?? null)["attested_subject_names"])) {
    const digest = digests.get(text(name));
    if (!digest) throw Error("attested subject missing");
    await attestations(digest);
  }
}
for (const [route, { bytes, kind }] of mutable) {
  const final = await request(route);
  if (JSON.stringify(project(decode(final), kind)) !== JSON.stringify(project(decode(bytes), kind))) throw Error("historical evidence changed during acquisition");
}
