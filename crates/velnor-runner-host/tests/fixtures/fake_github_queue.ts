// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { config, headers, messages, method, mutate, readState, record, refusal, reply, repository, unexpected, url } from "./fake_github_core.ts";

export async function queue(path: string): Promise<never> {
  if (method === "GET" && path === "/messages") {
    const expired = refusal("poll.expired");
    if (expired) {
      record("poll", { capacity: headers.get("x-scalesetmaxcapacity") ?? "none", last: "none", returned: "expired" });
      reply(expired.status, { message: "expired" });
    }
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
