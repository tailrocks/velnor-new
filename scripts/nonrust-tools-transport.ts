// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

"use strict";

import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { decodeSnappy } from "./nonrust-bundle-snappy";

type Mode = "record" | "replay";

async function bundleBytes(response: Response): Promise<Uint8Array> {
  const limit = 16 * 1024 * 1024;
  const declared = response.headers.get("content-length");
  if (declared !== null && (!/^\d+$/.test(declared) || Number(declared) > limit)) throw new Error("bundle response exceeds limit");
  const reader = response.body?.getReader();
  if (!reader) throw new Error("bundle response body missing");
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const chunk = await reader.read();
      if (chunk.done) break;
      length += chunk.value.length;
      if (length > limit) { await reader.cancel(); throw new Error("bundle response exceeds limit"); }
      chunks.push(chunk.value);
    }
  } finally { reader.releaseLock(); }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  return bytes;
}

async function originResponse(original: string, method: string, headers: Headers, token: string | undefined, observed: (url: URL) => void): Promise<Response> {
  let url = new URL(original);
  for (let hop = 0; hop <= 5; hop++) {
    if (url.protocol !== "https:" || url.username || url.password || url.port && url.port !== "443") throw new Error("inadmissible fixture origin");
    const forwarded = new Headers(headers);
    if (token && url.hostname === "api.github.com") forwarded.set("authorization", `Bearer ${token}`);
    observed(url);
    const response = await fetch(url, { method, headers: forwarded, redirect: "manual", signal: AbortSignal.timeout(30_000) });
    if (![301, 302, 303, 307, 308].includes(response.status)) return response;
    const location = response.headers.get("location");
    if (location === null) throw new Error("fixture origin redirect has no destination");
    url = new URL(location, url);
  }
  throw new Error("fixture origin redirect limit exceeded");
}

export async function transport(directory: string, mode: Mode, token?: string) {
  if (mode === "replay" && (token !== undefined || process.env.VELNOR_FIXTURE_ORIGIN_TOKEN !== undefined)) throw new Error("replay refuses origin credentials");
  const root = resolve(directory, "responses");
  await mkdir(root, { recursive: true });
  const recording = new Map<string, Promise<void>>();
  const metrics = { requests: 0, misses: 0, archiveRequests: 0, metadataRequests: 0, bytes: 0, metadataBytes: 0, normalizedBundles: 0, originRequests: 0, originGithubRequests: 0 };
  const observedOrigin = (url: URL): void => { metrics.originRequests += 1; if (url.hostname === "api.github.com") metrics.originGithubRequests += 1; };
  let denyArchives = false;
  let corruptBundles = false;
  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    async fetch(request) {
      const parsed = new URL(request.url);
      if (parsed.pathname === "/__metrics") return Response.json(metrics);
      if (parsed.pathname === "/__deny-archives" && request.method === "POST") { denyArchives = true; return new Response("denied"); }
      if (parsed.pathname === "/__allow-archives" && request.method === "POST") { denyArchives = false; return new Response("allowed"); }
      if (parsed.pathname === "/__corrupt-bundles" && request.method === "POST") { corruptBundles = true; return new Response("corrupting"); }
      metrics.requests += 1;
      if (!parsed.pathname.startsWith("/https/")) return new Response("unrecorded transport request", { status: 502 });
      const original = `https://${parsed.pathname.slice(7)}${parsed.search}`;
      const key = new Bun.CryptoHasher("sha256").update(`${request.method} ${original} ${request.headers.get("accept") ?? ""} ${request.headers.get("range") ?? ""} ${request.headers.get("if-none-match") ?? ""} ${request.headers.get("if-modified-since") ?? ""}`).digest("hex");
      const metadata = resolve(root, `${key}.json`);
      const body = resolve(root, `${key}.body`);
      if (mode === "record") {
        let pending = recording.get(key);
        if (!pending) {
          pending = (async () => {
        const forwarded = new Headers();
        for (const name of ["accept", "range", "user-agent", "x-github-api-version", "if-none-match", "if-modified-since"]) {
          const value = request.headers.get(name);
          if (value !== null) forwarded.set(name, value);
        }
        if (!["GET", "HEAD"].includes(request.method)) throw new Error("fixture origin method is not read-only");
        const upstream = await originResponse(original, request.method, forwarded, token, observedOrigin);
        const attestationApi = new URL(original).hostname === "api.github.com" && original.includes("/attestations/");
        let bytes = attestationApi ? await bundleBytes(upstream) : new Uint8Array(await upstream.arrayBuffer());
        if (attestationApi) {
          await Bun.write(resolve(root, `${key}.original.body`), bytes);
          const payload: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
          if (typeof payload !== "object" || payload === null || !("attestations" in payload) || !Array.isArray(payload.attestations)) throw new Error("invalid genuine attestation response");
          for (const [index, attestation] of payload.attestations.entries()) {
            if (typeof attestation !== "object" || attestation === null || !("bundle" in attestation)) throw new Error("invalid genuine attestation entry");
            if (attestation.bundle !== null) continue;
            if (!("bundle_url" in attestation) || typeof attestation.bundle_url !== "string") throw new Error("missing genuine bundle source");
            const source = new URL(attestation.bundle_url);
            if (source.protocol !== "https:" || source.hostname !== "tmaproduction.blob.core.windows.net") throw new Error("unexpected genuine bundle source");
            const bundleResponse = await originResponse(source.href, "GET", new Headers(), undefined, observedOrigin);
            if (!bundleResponse.ok) throw new Error(`genuine bundle source failed: ${bundleResponse.status}`);
            const originalBundle = await bundleBytes(bundleResponse);
            const decoded = bundleResponse.headers.get("content-type") === "application/x-snappy" ? decodeSnappy(originalBundle) : originalBundle;
            const bundle: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(decoded));
            if (typeof bundle !== "object" || bundle === null || !("mediaType" in bundle) || !("verificationMaterial" in bundle) || !("dsseEnvelope" in bundle)) throw new Error("malformed genuine bundle");
            await Bun.write(resolve(root, `${key}.bundle.${index}.body`), originalBundle);
            await writeFile(resolve(root, `${key}.bundle.${index}.json`), `${JSON.stringify({ source: source.href, status: bundleResponse.status, sha256: new Bun.CryptoHasher("sha256").update(originalBundle).digest("hex"), decodedSha256: new Bun.CryptoHasher("sha256").update(decoded).digest("hex"), signedEnvelopeSha256: new Bun.CryptoHasher("sha256").update(JSON.stringify(bundle.dsseEnvelope)).digest("hex") })}\n`);
            attestation.bundle = bundle;
            metrics.normalizedBundles += 1;
          }
          bytes = new TextEncoder().encode(JSON.stringify(payload));
        }
        const headers = [...upstream.headers].filter(([name]) => !["content-encoding", "content-length", "transfer-encoding"].includes(name));
        await Bun.write(body, bytes);
        await writeFile(metadata, `${JSON.stringify({ method: request.method, original, status: upstream.status, headers })}\n`);
          })();
          recording.set(key, pending);
        }
        await pending;
      }
      if (!await Bun.file(metadata).exists()) { metrics.misses += 1; return new Response(`unrecorded ${request.method} ${original}`, { status: 502 }); }
      const record: unknown = await Bun.file(metadata).json();
      if (typeof record !== "object" || record === null || !("status" in record) || typeof record.status !== "number" || !("method" in record) || record.method !== request.method || !("original" in record) || record.original !== original || !("headers" in record) || !Array.isArray(record.headers)) throw new Error("invalid fixture response identity");
      const headers = new Headers();
      for (const pair of record.headers) {
        if (!Array.isArray(pair) || pair.length !== 2 || typeof pair[0] !== "string" || typeof pair[1] !== "string") throw new Error("invalid fixture response header");
        headers.set(pair[0], pair[1]);
      }
      const archive = request.method === "GET" && /\.(?:tar(?:\.gz|\.xz|\.bz2)?|tgz|txz|zip|gz)(?:["';?\s]|$)/i.test(`${original} ${headers.get("content-disposition") ?? ""}`);
      if (archive) metrics.archiveRequests += 1; else metrics.metadataRequests += 1;
      if (archive && denyArchives) { metrics.misses += 1; return new Response("archive transport is unavailable", { status: 502 }); }
      const empty = request.method === "HEAD" || [204, 304].includes(record.status);
      let altered: Uint8Array | undefined;
      if (corruptBundles && original.includes("/attestations/") && record.status === 200) {
        const payload: unknown = await Bun.file(body).json();
        if (typeof payload !== "object" || payload === null || !("attestations" in payload) || !Array.isArray(payload.attestations)) throw new Error("invalid attestation fixture");
        let changed = 0;
        for (const attestation of payload.attestations) {
          const bundle: unknown = typeof attestation === "object" && attestation !== null && "bundle" in attestation ? attestation.bundle : null;
          const envelope: unknown = typeof bundle === "object" && bundle !== null && "dsseEnvelope" in bundle ? bundle.dsseEnvelope : null;
          if (typeof envelope !== "object" || envelope === null || !("signatures" in envelope) || !Array.isArray(envelope.signatures)) throw new Error("missing authentic signature");
          for (const signature of envelope.signatures) {
            if (typeof signature !== "object" || signature === null || !("sig" in signature) || typeof signature.sig !== "string" || signature.sig.length === 0) throw new Error("missing signature bytes");
            signature.sig = `${signature.sig.startsWith("A") ? "B" : "A"}${signature.sig.slice(1)}`;
            changed += 1;
          }
        }
        if (changed === 0) throw new Error("no authentic signature to corrupt");
        altered = new TextEncoder().encode(JSON.stringify(payload));
      }
      const size = empty ? 0 : altered?.length ?? Bun.file(body).size;
      metrics.bytes += size;
      if (!archive) metrics.metadataBytes += size;
      return new Response(empty ? null : altered ?? Bun.file(body), { status: record.status, headers });
    },
  });
  return { server, metrics, replacements: JSON.stringify({ "https://": `http://127.0.0.1:${server.port}/https/` }), metricsUrl: `http://127.0.0.1:${server.port}/__metrics` };
}

if (import.meta.main) {
  const directory = process.argv[2];
  if (!directory) throw new Error("fixture directory required");
  const result = await transport(directory, "replay");
  process.stdout.write(`${JSON.stringify({ replacements: result.replacements, metricsUrl: result.metricsUrl })}\n`);
}
