// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";

import { createHash } from "node:crypto";
import { appendFileSync } from "node:fs";
const tip = "a".repeat(40);
const releases = new Map([
  ["41bad147b5ba9db09be99b07248531ce1bd867be", { id: 422399645, asset: 670727432 }],
  ["5094139b2658e793d36306a34a7a135554c4f7eb", { id: 422676197, asset: 672027695 }],
]);
const receipt = JSON.stringify({ evidence: { attested_subject_names: ["release.json"] } });
const digest = createHash("sha256").update(receipt).digest("hex");
let branchReads = 0;
let assetReads = 0;
function response(value: unknown): Response { return new Response(JSON.stringify(value), { status: 200 }); }
globalThis.fetch = async (input, init): Promise<Response> => {
  const url = new URL(String(input));
  const headers = new Headers(init?.headers);
  if (url.origin !== "https://api.github.com") {
    if (headers.has("Authorization")) throw Error("off-origin credential violation");
    appendFileSync(process.env["API_MARKER"] ?? "", `FOREIGN:${url}\n`);
    return response({ raw: "signature evidence" });
  }
  if (headers.get("Authorization") !== "Bearer acquisition-only") throw Error("credential boundary violation");
  appendFileSync(process.env["API_MARKER"] ?? "", `${url.pathname}\n`);
  const route = url.pathname.replace("/repos/jackin-project/jackin", "");
  if (route === "") {
    if (process.env["FIXTURE_MODE"] === "oversized") return new Response("x".repeat(1024 ** 2 + 1));
    return response({ id: 1197700841 });
  }
  if (route === "/branches/main") {
    branchReads++;
    return response({ name: "main", protected: true, commit: { sha: process.env["FIXTURE_MODE"] === "changed-main" && branchReads > 1 ? "b".repeat(40) : tip } });
  }
  if (route.startsWith("/commits/")) return new Response(route.slice("/commits/".length));
  if (route.startsWith("/compare/")) return response({ total_commits: 2 });
  if (route.startsWith("/actions/")) return response(route.endsWith("/jobs") ? { total_count: 1, jobs: [{}] } : {});
  if (route.startsWith("/git/matching-refs/tags/")) {
    const tag = route.slice("/git/matching-refs/tags/".length);
    return response([{ ref: `refs/tags/${tag}`, object: { sha: tag.slice("preview-".length), type: "tag" } }]);
  }
  if (route.startsWith("/git/tags/")) return response({ sha: route.slice("/git/tags/".length), tag: `preview-${route.slice("/git/tags/".length)}`, message: "raw signed identity", object: { sha: route.slice("/git/tags/".length), type: "commit" } });
  for (const [source, release] of releases) {
    if (route === `/releases/tags/preview-${source}`) return response({ id: release.id, tag_name: `preview-${source}`, draft: false, prerelease: true, immutable: true, body: "raw envelope", assets: [{ download_count: assetReads }] });
    if (route === `/releases/${release.id}/assets`) {
      assetReads++;
      return response([{ id: release.asset, name: "release.json", state: "uploaded", size: receipt.length, digest: `sha256:${digest}`, download_count: assetReads }]);
    }
    if (route === `/releases/assets/${release.asset}`) return new Response(receipt);
  }
  if (route.startsWith("/attestations/sha256:")) return response({ attestations: [process.env["FIXTURE_MODE"] === "foreign-bundle" ? { bundle_url: "https://evil.example/bundle" } : { bundle: { raw: "signature evidence" } }] });
  throw Error(`unexpected external API route ${route}`);
};
