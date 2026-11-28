// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { type Connection, config as shared, configOf, connectionNamed, headers, method, offers, phase, record, reply, scriptedReply, unexpected } from "./fake_github_core.ts";
import { pullJson, repository as repositoryJson, runJson, treeId, trees } from "./fake_github_shapes.ts";

export function api(path: string): never {
  if (method === "POST" && path === "/actions/runner-registration") {
    const token = /registration-token\.([^\s]+)$/.exec(headers.get("authorization") ?? "")?.[1];
    reply(200, { url: shared.admin_url, token: `admin-token.${token}` });
  }
  if (method === "POST" && /^\/orgs\/[^/]+\/actions\/runners\/registration-token$/.test(path)) reply(201, { token: `registration-token.${shared.repo}` });
  const owned = /^\/repos\/[^/]+\/([^/]+)/.exec(path);
  const connection: Connection | undefined = connectionNamed(owned?.[1]);
  if (!connection) return unexpected();
  const config = configOf(connection);
  const base = `/repos/${config.owner}/${config.repo}`;
  if (method === "POST" && path === `${base}/actions/runners/registration-token`) reply(201, { token: `registration-token.${connection.repo}` });
  if (method === "GET" && path === base) {
    const scripted = scriptedReply("repo.get");
    record("repo.get", { status: scripted?.status ?? 200, phase: phase() || "none" });
    if (scripted?.status && scripted.status !== 200) reply(scripted.status, { message: "unavailable" });
    reply(200, { ...repositoryJson(config), ...scripted?.patch });
  }
  const run = /^\/repos\/[^/]+\/[^/]+\/actions\/runs\/(\d+)(?:\/attempts\/\d+)?$/.exec(path);
  if (method === "GET" && run?.[1]) {
    const scripted = scriptedReply("run.get");
    record("run.get", { run: run[1], status: scripted?.status ?? 200 });
    if (scripted?.status && scripted.status !== 200) reply(scripted.status, { message: "unavailable" }, scripted.headers);
    const offer = offers().find((candidate) => candidate.run_id === Number(run[1]));
    if (offer) reply(200, runJson(config, offer, path.includes("/attempts/")));
  }
  const pull = /^\/repos\/[^/]+\/[^/]+\/pulls\/(\d+)$/.exec(path);
  if (method === "GET" && pull?.[1]) {
    const offer = offers().find((candidate) => candidate.pr?.number === Number(pull[1]));
    if (offer?.pr) reply(200, pullJson(config, offer.pr, offer));
  }
  const commit = /^\/repos\/[^/]+\/[^/]+\/git\/commits\/([0-9a-f]{40})$/.exec(path);
  if (method === "GET" && commit?.[1]) {
    const offer = offers().find((candidate) => candidate.sha === commit[1]);
    if (offer) {
      const parents = offer.pr ? (offer.pr.merge_parents ?? [offer.pr.base_sha, offer.pr.head_sha]) : [];
      reply(200, { sha: offer.sha, message: "commit", tree: { sha: treeId(offer, "root") }, parents: parents.map((parent) => ({ sha: parent })) });
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
