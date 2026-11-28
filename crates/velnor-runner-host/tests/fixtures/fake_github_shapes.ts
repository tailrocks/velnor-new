// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";

export interface Repo {
  owner: string;
  repo: string;
  repository_id: number;
  default_branch?: string;
}
export interface Pull {
  number: number;
  base_sha: string;
  head_sha: string;
  base_ref: string;
  head_ref: string;
  merge_parents?: string[];
}
export interface Offer {
  request_id: number;
  event: "push" | "pull_request" | "merge_group";
  run_id: number;
  ref: string;
  branch: string;
  sha: string;
  workflow_content: string;
  pr?: Pull;
  kind?: "JobAssigned";
  acquire_returns?: "none" | "duplicate" | "extra";
  jit_failures?: number;
  jit_identity?: "id" | "scale_set" | "name";
  head_repository_id?: number;
  run_repository_id?: number;
}

const API = "https://api.github.com";
const TEMPLATE_URL_KEYS = [
  "forks_url", "keys_url", "collaborators_url", "teams_url", "hooks_url", "issue_events_url",
  "events_url", "assignees_url", "branches_url", "tags_url", "blobs_url", "git_tags_url",
  "git_refs_url", "trees_url", "statuses_url", "languages_url", "stargazers_url",
  "contributors_url", "subscribers_url", "subscription_url", "commits_url", "git_commits_url",
  "comments_url", "issue_comment_url", "contents_url", "compare_url", "merges_url", "archive_url",
  "downloads_url", "issues_url", "pulls_url", "milestones_url", "notifications_url", "labels_url",
  "releases_url", "deployments_url",
];
const REPOSITORY_ONLY_KEYS = [
  "created_at", "updated_at", "pushed_at", "git_url", "ssh_url", "clone_url", "svn_url",
  "homepage", "language", "mirror_url", "license", "temp_clone_token", "organization",
];

function account(login: string): object {
  const keys = ["avatar_url", "gravatar_id", "url", "html_url", "followers_url", "following_url", "gists_url", "starred_url", "subscriptions_url", "organizations_url", "repos_url", "events_url", "received_events_url"];
  return {
    login,
    id: 1,
    node_id: "node",
    ...Object.fromEntries(keys.map((key) => [key, `${API}/users/${login}`])),
    type: "User",
    user_view_type: "public",
    site_admin: false,
  };
}

export function minimalRepository(c: Repo): Record<string, unknown> {
  const full = `${c.owner}/${c.repo}`;
  return {
    id: c.repository_id,
    node_id: "node",
    name: c.repo,
    full_name: full,
    private: true,
    owner: account(c.owner),
    html_url: `https://github.com/${full}`,
    description: "lab",
    fork: false,
    url: `${API}/repos/${full}`,
    ...Object.fromEntries(TEMPLATE_URL_KEYS.map((key) => [key, `${API}/repos/${full}/${key}`])),
  };
}

export function repository(c: Repo): Record<string, unknown> {
  return {
    ...minimalRepository(c),
    ...Object.fromEntries(REPOSITORY_ONLY_KEYS.map((key) => [key, null])),
    size: 1,
    stargazers_count: 0,
    watchers_count: 0,
    has_issues: true,
    has_projects: true,
    has_downloads: true,
    has_wiki: true,
    has_pages: false,
    has_discussions: false,
    forks_count: 0,
    archived: false,
    disabled: false,
    open_issues_count: 0,
    allow_forking: true,
    is_template: false,
    web_commit_signoff_required: false,
    has_pull_requests: true,
    pull_request_creation_policy: "all",
    topics: [],
    visibility: "private",
    forks: 0,
    open_issues: 0,
    watchers: 0,
    default_branch: c.default_branch ?? "main",
    custom_properties: {},
    network_count: 0,
    subscribers_count: 0,
  };
}

export function runJson(c: Repo, offer: Offer, attempt: boolean): object {
  const pull = offer.pr;
  const stub = (ref: string, sha: string, id: number) => ({
    ref,
    sha,
    repo: { id, url: `${API}/repos/${c.owner}/${c.repo}`, name: c.repo },
  });
  const url = `${API}/repos/${c.owner}/${c.repo}/actions/runs/${offer.run_id}`;
  return {
    id: offer.run_id,
    name: "ci",
    node_id: "node",
    head_branch: pull ? pull.head_ref : offer.branch,
    head_sha: pull ? pull.head_sha : offer.sha,
    path: ".github/workflows/ci.yml",
    display_title: "ci",
    run_number: 1,
    event: offer.event,
    status: "queued",
    conclusion: null,
    workflow_id: 7,
    check_suite_id: 1,
    check_suite_node_id: "node",
    url,
    html_url: url,
    pull_requests: pull
      ? [{ url: `${API}/repos/${c.owner}/${c.repo}/pulls/${pull.number}`, id: pull.number, number: pull.number, head: stub(pull.head_ref, pull.head_sha, offer.head_repository_id ?? c.repository_id), base: stub(pull.base_ref, pull.base_sha, c.repository_id) }]
      : [],
    created_at: "2026-11-27T10:00:00Z",
    updated_at: "2026-11-27T10:00:00Z",
    actor: account(c.owner),
    run_attempt: 1,
    referenced_workflows: [],
    run_started_at: "2026-11-27T10:00:00Z",
    triggering_actor: account(c.owner),
    jobs_url: `${url}/jobs`,
    logs_url: `${url}/logs`,
    check_suite_url: `${API}/repos/${c.owner}/${c.repo}/check-suites/1`,
    artifacts_url: `${url}/artifacts`,
    cancel_url: `${url}/cancel`,
    rerun_url: `${url}/rerun`,
    previous_attempt_url: null,
    workflow_url: `${API}/repos/${c.owner}/${c.repo}/actions/workflows/7`,
    head_commit: { id: pull ? pull.head_sha : offer.sha },
    repository: { ...minimalRepository(c), id: attempt ? (offer.run_repository_id ?? c.repository_id) : c.repository_id },
    head_repository: { ...minimalRepository(c), id: offer.head_repository_id ?? c.repository_id },
  };
}

export function pullJson(c: Repo, pull: Pull, offer: Offer): object {
  const side = (ref: string, sha: string, id: number) => ({ label: `${c.owner}:${ref}`, ref, repo: { ...repository(c), id }, sha, user: account(c.owner) });
  return {
    url: `${API}/repos/${c.owner}/${c.repo}/pulls/${pull.number}`,
    id: pull.number,
    node_id: "node",
    number: pull.number,
    state: "open",
    locked: false,
    title: "change",
    user: account(c.owner),
    created_at: "2026-11-27T10:00:00Z",
    updated_at: "2026-11-27T10:00:00Z",
    closed_at: null,
    merged_at: null,
    merge_commit_sha: offer.sha,
    head: side(pull.head_ref, pull.head_sha, offer.head_repository_id ?? c.repository_id),
    base: side(pull.base_ref, pull.base_sha, c.repository_id),
    draft: false,
    merged: false,
    mergeable: true,
    mergeable_state: "clean",
  };
}

function sha(text: string): string {
  return createHash("sha1").update(text).digest("hex");
}

export function treeId(offer: Offer, level: "root" | "gh" | "wf"): string {
  return sha(`${level}:${offer.sha}`);
}

export function trees(offer: Offer): Record<string, object> {
  const content = Buffer.from(offer.workflow_content);
  const blob = createHash("sha1").update(`blob ${content.length}\0`).update(content).digest("hex");
  const [top, dot, flows] = [treeId(offer, "root"), treeId(offer, "gh"), treeId(offer, "wf")];
  const entry = (path: string, mode: string, type: string, id: string, size?: number) => ({ path, mode, type, sha: id, size });
  return {
    [top]: { sha: top, truncated: false, tree: [entry(".github", "040000", "tree", dot)] },
    [dot]: { sha: dot, truncated: false, tree: [entry("workflows", "040000", "tree", flows)] },
    [flows]: { sha: flows, truncated: false, tree: [entry("ci.yml", "100644", "blob", blob, content.length)] },
    [blob]: { sha: blob, encoding: "base64", size: content.length, content: content.toString("base64") },
  };
}
