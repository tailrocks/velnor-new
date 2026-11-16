// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { expect, test } from "bun:test";
import { TOML, YAML } from "bun";
import recorded from "../fixtures/tag-ruleset-drift/protect-tags.json";
function object(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error("Expected a mapping");
  return Object.fromEntries(Object.entries(value));
}
function array(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error("Expected a sequence");
  return value;
}
const workflow = object(YAML.parse(await Bun.file(new URL("../.github/workflows/release-rust-ci-tools.yml", import.meta.url)).text()));
const jobs = object(workflow.jobs);
const drift = object(jobs["tag-ruleset-drift"]);
const steps = array(drift.steps).map(object);
const step = steps.find(value => value.name === "Check tag immutability rulesets");
if (!step) throw new Error("Tag ruleset drift step is missing");
const inputs = object(step.with);
if (typeof inputs.script !== "string") throw new Error("Drift script is missing");
const execute = new Function("github", "context", "process", `return (async () => {${inputs.script}\n})();`);
type Ruleset = Omit<typeof recorded, "bypass_actors" | "conditions"> & { conditions: { ref_name: { include: string[]; exclude: string[] } }; bypass_actors?: unknown[] };
const baseline = (): Ruleset => structuredClone(recorded), actor = { actor_id: 5762496, actor_type: "Integration", bypass_mode: "always" };
const release = (): Ruleset => ({ ...baseline(), id: recorded.id + 1, name: "release-tags", rules: [{ type: "creation" }], bypass_actors: [{ ...actor }] });
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
  await execute(github, { repo: { owner: "tailrocks", repo: "velnor-new" } }, { env: { TAG_RULESET_ADMIN_READ_TOKEN: token } });
  return detailed;
}
test("the recorded protection and pinned release app prove the expected tag policy", async () => {
  expect(await run()).toEqual([recorded.id, recorded.id + 1]);
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
test("daily and manual checks isolate the credential from release and checkout code", () => {
  const events = object(workflow.on);
  expect(object(array(events.schedule)[0]).cron).toBe("23 4 * * *");
  expect(Object.hasOwn(events, "workflow_dispatch")).toBe(true);
  expect(drift.if).toBe("(github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)");
  expect(drift).toMatchObject({ permissions: {}, "timeout-minutes": 5 });
  expect(steps).toHaveLength(1);
  expect(step.uses).toBe("actions/github-script@3a2844b7e9c422d3c10d287c895573f7108da1b3");
  expect(inputs["github-token"]).toBe("${{ secrets.TAG_RULESET_ADMIN_READ_TOKEN }}");
  expect(object(step.env)).toEqual({ TAG_RULESET_ADMIN_READ_TOKEN: inputs["github-token"] });
  expect(object(jobs.gate).if).toBe("github.event_name == 'push' && startsWith(github.ref, 'refs/tags/rust-ci-v')");
  for (const [id, job] of Object.entries(jobs)) {
    if (id === "tag-ruleset-drift") continue;
    expect(JSON.stringify(job)).not.toContain("TAG_RULESET_ADMIN_READ_TOKEN");
    if (id !== "gate") {
      expect(object(job).needs).toBe(new Map(Object.entries({ build: "gate", checksums: "build", release: "checksums" })).get(id));
      expect(object(job).if).toBeUndefined();
    }
  }
});
test("the maintenance check provisions the project's pinned Bun in its isolated tool environment", async () => {
  const config = object(TOML.parse(await Bun.file(new URL("../.velnor/config.toml", import.meta.url)).text()));
  const mise = object(TOML.parse(await Bun.file(new URL("../mise.toml", import.meta.url)).text()));
  const maintenance = array(config.checks).map(object).find(check => check.id === "maintenance-helpers");
  if (!maintenance) throw new Error("maintenance-helpers check is missing");
  const selected = array(maintenance.tools);
  const bun = array(config.qualified_tools).map(object).find(tool => selected.includes(tool.id) && object(tool.backend).tool === "bun");
  if (!bun) throw new Error("maintenance-helpers must select a qualified Bun runtime");
  expect(bun.version).toBe(object(mise.tools).bun);
  const linux = array(bun.platforms).map(object).find(platform => platform.platform === "linux_x64");
  if (!linux) throw new Error("Bun must be qualified for the maintenance runner");
  expect(array(linux.executables).map(object)).toContainEqual(expect.objectContaining({ name: "bun", path: "bun", probe: { kind: "version", expected: object(mise.tools).bun } }));
});
