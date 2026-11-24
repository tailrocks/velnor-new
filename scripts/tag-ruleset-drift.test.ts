// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { expect, test } from "bun:test";
import { TOML, YAML } from "bun";
import { checkTagRulesets, tagPushFamilyPatterns } from "./tag-ruleset-drift.mjs";
import recorded from "../fixtures/tag-ruleset-drift/protect-tags.json";
function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Expected a mapping");
  return Object.fromEntries(Object.entries(value));
}
function array(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error("Expected a sequence");
  return value;
}
const workflow = object(YAML.parse(await Bun.file(new URL("../.github/workflows/release.yml", import.meta.url)).text()));
const jobs = object(workflow.jobs);
const driftWorkflow = object(YAML.parse(await Bun.file(new URL("../.github/workflows/tag-ruleset-drift.yml", import.meta.url)).text()));
const driftJobs = object(driftWorkflow.jobs);
const drift = object(driftJobs["tag-ruleset-drift"]);
const steps = array(drift.steps).map(object);
const checkout = steps[0], step = steps[1];
type Ruleset = Omit<typeof recorded, "bypass_actors" | "conditions"> & { conditions: { ref_name: { include: string[]; exclude: string[] } }; bypass_actors?: unknown[] };
const baseline = (): Ruleset => structuredClone(recorded), actor = { actor_id: 5762496, actor_type: "Integration", bypass_mode: "always" };
const releaseRefs = ["refs/tags/rust-ci-v*", "refs/tags/velnor-ci-v*", "refs/tags/velnor-gen-v*"] satisfies [string, string, string];
const toolsRef = "refs/tags/tools-v*";
const release = (): Ruleset => ({ ...baseline(), id: recorded.id + 1, name: "release-tags", conditions: { ref_name: { include: [...releaseRefs], exclude: [] } }, rules: [{ type: "creation" }], bypass_actors: [{ ...actor }] });
type Scenario = { rulesets?: Ruleset[]; token?: string; failure?: "" | "list" | "detail" };
async function run({ rulesets = [baseline(), release()], token = "admin-read-token", failure = "" }: Scenario = {}) {
  const detailed: number[] = [];
  const github = {
    async paginate(route: string, parameters: unknown) {
      expect(route).toBe("GET /repos/{owner}/{repo}/rulesets");
      expect(parameters).toEqual({ owner: "tailrocks", repo: "velnor-new", per_page: 100 });
      if (failure === "list") throw new Error("List unavailable");
      return rulesets.map(({ id, name, target }) => ({ id, name, target }));
    },
    async request(route: string, parameters: { owner: string; repo: string; ruleset_id: number }) {
      expect({ route, owner: parameters.owner, repo: parameters.repo }).toEqual({ route: "GET /repos/{owner}/{repo}/rulesets/{ruleset_id}", owner: "tailrocks", repo: "velnor-new" });
      if (failure === "detail") throw new Error("Detail unavailable");
      detailed.push(parameters.ruleset_id);
      const data = rulesets.find(rule => rule.id === parameters.ruleset_id);
      if (!data) throw new Error("Unknown ruleset");
      return { data };
    },
  };
  const getOctokit = (auth: string) => {
    expect(auth).toBe(token);
    return github;
  };
  await checkTagRulesets({ context: { repo: { owner: "tailrocks", repo: "velnor-new" } }, process: { env: { TAG_RULESET_ADMIN_READ_TOKEN: token } }, getOctokit });
  return detailed;
}
test("the recorded protection and pinned release app prove the expected tag policy", async () => {
  expect(await run()).toEqual([recorded.id, recorded.id + 1]);
  const transition = release();
  transition.conditions.ref_name.include.push(toolsRef);
  expect(await run({ rulesets: [baseline(), transition] })).toEqual([recorded.id, recorded.id + 1]);
  const reordered = release();
  reordered.conditions.ref_name.include.reverse();
  expect(await run({ rulesets: [baseline(), reordered] })).toEqual([recorded.id, recorded.id + 1]);
});
test.each([
  ["no creation rule", { rules: [] }],
  ["an update rule instead of creation", { rules: [{ type: "update" }] }],
  ["a narrowed release ref set", { conditions: { ref_name: { include: [releaseRefs[0]], exclude: [] } } }],
  ["all tags instead of release refs", { conditions: { ref_name: { include: ["~ALL"], exclude: [] } } }],
  ["tools-v alone instead of the release refs", { conditions: { ref_name: { include: [toolsRef], exclude: [] } } }],
  ["a bare v ref", { conditions: { ref_name: { include: [...releaseRefs, "refs/tags/v*"], exclude: [] } } }],
  ["tools-v with a missing legacy ref", { conditions: { ref_name: { include: [releaseRefs[0], releaseRefs[1], toolsRef], exclude: [] } } }],
  ["tools-v duplicated to hide an absent ref", { conditions: { ref_name: { include: [releaseRefs[0], releaseRefs[1], toolsRef, toolsRef], exclude: [] } } }],
  ["an extra release ref", { conditions: { ref_name: { include: [...releaseRefs, "refs/tags/extra-v*"], exclude: [] } } }],
  ["an excluded release ref", { conditions: { ref_name: { include: [...releaseRefs], exclude: [releaseRefs[0]] } } }],
  ["a duplicate ref hiding an absent ref", { conditions: { ref_name: { include: [releaseRefs[0], releaseRefs[0], releaseRefs[2]], exclude: [] } } }],
] satisfies [string, Partial<Ruleset>][])("release-tags rejects %s", async (_name, changed) => {
  await expect(run({ rulesets: [baseline(), { ...release(), ...changed }] })).rejects.toThrow("release-tags must protect creation of exactly the release tag refs");
});
test("an unreadable release ref condition cannot prove creation protection", async () => {
  const rule = release();
  Reflect.deleteProperty(rule, "conditions");
  await expect(run({ rulesets: [baseline(), rule] })).rejects.toThrow("release-tags must protect creation of exactly the release tag refs");
});
test("a bypass actor added to the recorded immutability ruleset fails", async () => {
  const rule = baseline();
  rule.bypass_actors = [actor];
  await expect(run({ rulesets: [rule, release()] })).rejects.toThrow("tag immutability bypass actors detected");
});
test("an unreadable bypass list cannot prove tag immutability", async () => {
  const rule = baseline();
  delete rule.bypass_actors;
  await expect(run({ rulesets: [rule, release()] })).rejects.toThrow("bypass_actors unreadable");
});
test.each([
  ["an extra owner", [actor, { actor_id: 139017, actor_type: "User", bypass_mode: "always" }]],
  ["a duplicate release app", [actor, actor]],
  ["a missing release app", []],
  ["a wrong app ID", [{ ...actor, actor_id: 139017 }]],
  ["a wrong actor type", [{ ...actor, actor_type: "User" }]],
  ["a wrong bypass mode", [{ ...actor, bypass_mode: "pull_request" }]],
])("release-tags rejects %s", async (_name, actors) => {
  await expect(run({ rulesets: [baseline(), { ...release(), bypass_actors: actors }] })).rejects.toThrow("release-tags: tag immutability bypass actors detected");
});
test("an unreadable release bypass list cannot prove the pinned release app", async () => {
  const rule = release();
  delete rule.bypass_actors;
  await expect(run({ rulesets: [baseline(), rule] })).rejects.toThrow("release-tags: bypass_actors unreadable");
});
test("missing, duplicated or inactive release protection fails", async () => {
  for (const rulesets of [
    [baseline()],
    [baseline(), release(), { ...release(), id: 1 }],
    [baseline(), release(), { ...release(), id: 1, target: "branch" }],
    [baseline(), { ...release(), target: "branch" }],
    [baseline(), { ...release(), enforcement: "disabled" }],
  ]) {
    await expect(run({ rulesets })).rejects.toThrow("release-tags must be exactly one active tag ruleset");
  }
});
test("all paginated tag details are checked while branch rulesets are excluded", async () => {
  const rulesets = Array.from({ length: 101 }, (_, id) => ({
    ...baseline(), id, name: `creation-${id}`, rules: [{ type: "creation" }], bypass_actors: [actor],
  }));
  const branch = { ...baseline(), id: 102, name: "branch", target: "branch", bypass_actors: [actor] };
  const all = [...rulesets, release(), baseline(), branch];
  expect(await run({ rulesets: all })).toEqual([...rulesets.map(rule => rule.id), recorded.id + 1, recorded.id]);
  for (const type of ["update", "deletion"]) {
    const additional = { ...baseline(), id: 103, name: "additional", enforcement: "disabled", rules: [{ type }], bypass_actors: [actor] };
    await expect(run({ rulesets: [...all, additional] })).rejects.toThrow("additional: tag immutability bypass actors detected");
  }
});
test("missing, duplicated or weakened baseline protection fails", async () => {
  await expect(run({ rulesets: [release()] })).rejects.toThrow("protect-tags must actively prevent");
  await expect(run({ rulesets: [baseline(), { ...baseline(), id: 1 }, release()] })).rejects.toThrow("protect-tags must actively prevent");
  await expect(run({ rulesets: [baseline(), { ...baseline(), id: 1, target: "branch" }, release()] })).rejects.toThrow("protect-tags must actively prevent");
  for (const rule of [
    { ...baseline(), enforcement: "disabled" },
    { ...baseline(), target: "branch" },
    { ...baseline(), rules: [{ type: "deletion" }] },
    { ...baseline(), rules: [{ type: "update" }] },
    { ...baseline(), conditions: { ref_name: { include: ["refs/tags/v*"], exclude: [] } } },
    { ...baseline(), conditions: { ref_name: { include: ["~ALL"], exclude: ["refs/tags/v*"] } } },
  ]) {
    await expect(run({ rulesets: [rule, release()] })).rejects.toThrow("protect-tags must actively prevent");
  }
});
test("API failures and absent owner credentials fail the check", async () => {
  await expect(run({ failure: "list" })).rejects.toThrow("List unavailable");
  await expect(run({ failure: "detail" })).rejects.toThrow("Detail unavailable");
  await expect(run({ token: "" })).rejects.toThrow("Provision TAG_RULESET_ADMIN_READ_TOKEN");
});
test("the manual check isolates the credential from release and checkout code", () => {
  expect(driftWorkflow.on).toEqual({ workflow_dispatch: {}, schedule: [{ cron: "23 4 * * *" }] });
  expect(driftWorkflow.permissions).toEqual({});
  expect(Object.keys(driftJobs)).toEqual(["tag-ruleset-drift"]);
  expect(drift.if).toBe("${{ github.ref == format('refs/heads/{0}', github.event.repository.default_branch) }}");
  expect(drift).toMatchObject({ permissions: { contents: "read" }, "timeout-minutes": 5 });
  expect(steps).toHaveLength(2);
  expect(object(checkout.with)["persist-credentials"]).toBe(false);
  expect(checkout.env).toBeUndefined();
  expect(step.run).toBe("node scripts/tag-ruleset-drift.mjs");
  expect(step.uses).toBeUndefined();
  expect(object(step.env)).toEqual({ TAG_RULESET_ADMIN_READ_TOKEN: "${{ secrets.TAG_RULESET_ADMIN_READ_TOKEN }}", GITHUB_REPOSITORY: "${{ github.repository }}" });
  expect(Object.hasOwn(object(workflow.on), "schedule")).toBe(false);
  expect(object(jobs.gate).if).toBe("${{ github.ref_type == 'tag' }}");
  expect(object(jobs.gate).needs).toBeUndefined();
  for (const [id, job] of Object.entries(jobs)) {
    expect(JSON.stringify(job)).not.toContain("TAG_RULESET_ADMIN_READ_TOKEN");
    if (id !== "gate") expect(JSON.stringify(object(job).needs)).toContain("gate");
  }
});
test("the maintenance task runs this test with the project's pinned Bun", async () => {
  const mise = object(TOML.parse(await Bun.file(new URL("../mise.toml", import.meta.url)).text()));
  expect(typeof object(mise.tools).bun).toBe("string");
  const task = object(object(mise.tasks)["test-maintenance-helpers"]);
  expect(String(task.run)).toContain("bun test ./scripts/tag-ruleset-drift.test.ts");
});
const environmentRoute = "GET /repos/{owner}/{repo}/environments/{environment_name}";
const policiesRoute = `${environmentRoute}/deployment-branch-policies`;
type Policy = { name: string; type: "branch" | "tag" };
type Environment = { deployment_branch_policy: null | { protected_branches: boolean; custom_branch_policies: boolean }; policies?: Policy[]; total?: number };
const custom = { protected_branches: false, custom_branch_policies: true };
async function runEnvironment(environment: Environment, releaseToml: string) {
  const calls: string[] = [];
  const rulesets = [baseline(), release()];
  const github = {
    async paginate() { return rulesets.map(({ id, name, target }) => ({ id, name, target })); },
    async request(route: string, parameters: { owner: string; repo: string; environment_name?: string; ruleset_id?: number }) {
      calls.push(route);
      if (route.endsWith("/rulesets/{ruleset_id}")) return { data: rulesets.find(rule => rule.id === parameters.ruleset_id) };
      if (route === environmentRoute) {
        expect(parameters).toEqual({ owner: "tailrocks", repo: "velnor-new", environment_name: "release" });
        return { data: { name: "release", deployment_branch_policy: environment.deployment_branch_policy } };
      }
      expect(route).toBe(policiesRoute);
      expect(parameters).toMatchObject({ owner: "tailrocks", repo: "velnor-new", environment_name: "release" });
      const branch_policies = environment.policies ?? [];
      return { data: { total_count: environment.total ?? branch_policies.length, branch_policies } };
    },
  };
  const process = { env: { TAG_RULESET_ADMIN_READ_TOKEN: "admin-read-token" } };
  const context = { repo: { owner: "tailrocks", repo: "velnor-new" } };
  await checkTagRulesets({ context, process, getOctokit: () => github, releaseToml }).catch(error => { throw Object.assign(error, { calls }); });
  return calls;
}
const tagPush = 'family_tag = "tools-v{{ version }}"\nfamily_trigger = "tag_push"\n\n[[binaries]]\npackage = "velnor-ci"\n';
const rulesetDetails = Array<string>(2).fill("GET /repos/{owner}/{repo}/rulesets/{ruleset_id}");
const basics: Policy[] = [{ name: "main", type: "branch" }];
test("the checked-in release.toml names the tools-v* family tag pattern", async () => {
  const toml = await Bun.file(new URL("../.velnor/release.toml", import.meta.url)).text();
  expect(tagPushFamilyPatterns(toml)).toEqual(["tools-v*"]);
  expect(tagPushFamilyPatterns('family_tag = "x-v{{ version }}"\n')).toEqual([]);
  expect(tagPushFamilyPatterns('# family_tag = "x-v"\nfamily_trigger = "tag_push"\n')).toEqual([]);
});
test("a release environment that allows the family tag pattern passes", async () => {
  const calls = await runEnvironment({ deployment_branch_policy: custom, policies: [...basics, { name: "tools-v*", type: "tag" }] }, tagPush);
  expect(calls).toEqual([environmentRoute, policiesRoute, ...rulesetDetails]);
});
test("a release environment without the family tag policy fails with a clear message", async () => {
  for (const policies of [basics, [...basics, { name: "tools-v*", type: "branch" } satisfies Policy], [...basics, { name: "other-v*", type: "tag" } satisfies Policy], []]) {
    await expect(runEnvironment({ deployment_branch_policy: custom, policies }, tagPush)).rejects.toThrow("deployment policies must allow the tag pattern tools-v*");
  }
});
test("an environment limited to protected branches fails and an unrestricted one passes", async () => {
  await expect(runEnvironment({ deployment_branch_policy: { protected_branches: true, custom_branch_policies: false } }, tagPush)).rejects.toThrow("allows only protected branches");
  expect(await runEnvironment({ deployment_branch_policy: null }, tagPush)).toEqual([environmentRoute, ...rulesetDetails]);
});
test("an incomplete policy page cannot prove the tag policy", async () => {
  await expect(runEnvironment({ deployment_branch_policy: custom, policies: basics, total: 120 }, tagPush)).rejects.toThrow("page through them");
});
test("a release.toml without a tag_push family never reads the environment", async () => {
  expect(await runEnvironment({ deployment_branch_policy: custom }, 'family_tag = "tools-v{{ version }}"\n')).toEqual(rulesetDetails);
});

function matchesTagFilter(pattern: string, tag: string): boolean {
  return new RegExp(`^${pattern.replace(/[.]/g, "\\.").replace(/\*/g, "[^/]*")}$`).test(tag);
}
test("the release workflow runs on tools-v tags and on no other release tag", () => {
  const tags = array(object(object(workflow.on).push).tags);
  expect(tags).toEqual(["tools-v*"]);
  for (const tag of ["tools-v0.1.85"]) {
    expect(tags.some(pattern => matchesTagFilter(String(pattern), tag))).toBe(true);
  }
  for (const tag of ["v0.1.85", "rust-ci-v0.1.84", "runner-v0.1.85", "actions-v0.1.85", "velnor-ci-v0.1.85", "velnor-gen-v0.1.85"]) {
    expect(tags.some(pattern => matchesTagFilter(String(pattern), tag))).toBe(false);
  }
});
