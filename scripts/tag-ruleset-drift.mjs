// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const RELEASE_ENVIRONMENT = "release";

// The tag patterns of every tag_push family: the family tag prefix plus `*`.
export function tagPushFamilyPatterns(releaseToml) {
  const text = releaseToml.replace(/^\s*#.*$/gm, "");
  const top = text.split(/^\s*\[/m)[0];
  const value = key => new RegExp(`^\\s*${key}\\s*=\\s*"([^"]*)"`, "m").exec(top)?.[1];
  const tag = value("family_tag");
  if (value("family_trigger") !== "tag_push" || tag === undefined) return [];
  const prefix = tag.split("{{")[0];
  if (prefix === "") throw new Error("family_tag must start with a literal prefix to derive its tag pattern");
  return [`${prefix}*`];
}

export async function checkReleaseEnvironmentTagPolicy({ context, admin, patterns }) {
  if (patterns.length === 0) return;
  const { owner, repo } = context.repo;
  const parameters = { owner, repo, environment_name: RELEASE_ENVIRONMENT };
  const { data: environment } = await admin.request('GET /repos/{owner}/{repo}/environments/{environment_name}', parameters);
  const policy = environment?.deployment_branch_policy;
  if (policy === null) return;
  if (!policy?.custom_branch_policies) {
    throw new Error(`the ${RELEASE_ENVIRONMENT} environment allows only protected branches, so a tag push of a tag_push family never starts the family job; select "Selected branches and tags" and allow ${patterns.join(', ')}`);
  }
  const listed = await admin.request('GET /repos/{owner}/{repo}/environments/{environment_name}/deployment-branch-policies', { ...parameters, per_page: 100 });
  const policies = listed.data?.branch_policies ?? [];
  if (listed.data?.total_count > policies.length) {
    throw new Error(`the ${RELEASE_ENVIRONMENT} environment has more than ${policies.length} deployment policies; page through them before checking the tag policy`);
  }
  const allowedTags = policies.filter(entry => entry.type === 'tag').map(entry => entry.name);
  const missing = patterns.filter(pattern => !allowedTags.includes(pattern));
  if (missing.length > 0) {
    throw new Error(`the ${RELEASE_ENVIRONMENT} environment deployment policies must allow the tag pattern ${missing.join(', ')} of a tag_push family in .velnor/release.toml, or the family job never starts; add a "tag" deployment policy (Settings > Environments > ${RELEASE_ENVIRONMENT} > Deployment branches and tags)`);
  }
}

export async function checkTagRulesets({ context, process, getOctokit, releaseToml = "" }) {
  if (!process.env.TAG_RULESET_ADMIN_READ_TOKEN) {
    throw new Error('Provision TAG_RULESET_ADMIN_READ_TOKEN with Administration: read (#949)');
  }
  const admin = getOctokit(process.env.TAG_RULESET_ADMIN_READ_TOKEN);
  const { owner, repo } = context.repo;
  await checkReleaseEnvironmentTagPolicy({ context, admin, patterns: tagPushFamilyPatterns(releaseToml) });
  const expectedBypassActors = new Map([
    ['protect-tags', []],
    ['release-tags', [{ actor_id: 5762496, actor_type: 'Integration', bypass_mode: 'always' }]]
  ]);
  const listed = await admin.paginate('GET /repos/{owner}/{repo}/rulesets', {
    owner, repo, per_page: 100
  });
  const rulesets = [];
  for (const listedRuleset of listed.filter(rule => rule.target === 'tag' || expectedBypassActors.has(rule.name))) {
    const { data } = await admin.request('GET /repos/{owner}/{repo}/rulesets/{ruleset_id}', {
      owner, repo, ruleset_id: listedRuleset.id
    });
    rulesets.push(data);
  }
  const baseline = rulesets.filter(rule => rule.name === 'protect-tags');
  const [protection] = baseline;
  if (baseline.length !== 1 || protection.target !== 'tag' || protection.enforcement !== 'active'
      || !protection.conditions?.ref_name?.include?.includes('~ALL')
      || protection.conditions?.ref_name?.exclude?.length !== 0
      || !['update', 'deletion'].every(type => protection.rules?.some(rule => rule.type === type))) {
    throw new Error('protect-tags must actively prevent all tag updates and deletion without exclusions');
  }
  const releases = rulesets.filter(rule => rule.name === 'release-tags');
  if (releases.length !== 1 || releases[0].target !== 'tag' || releases[0].enforcement !== 'active') {
    throw new Error('release-tags must be exactly one active tag ruleset');
  }
  const legacyReleaseRefs = ['refs/tags/rust-ci-v*', 'refs/tags/velnor-ci-v*', 'refs/tags/velnor-gen-v*'];
  const acceptedReleaseRefs = [legacyReleaseRefs, [...legacyReleaseRefs, 'refs/tags/tools-v*']];
  const releaseRefs = releases[0].conditions?.ref_name;
  if (!Array.isArray(releaseRefs?.include)
      || !acceptedReleaseRefs.some(expected => releaseRefs.include.length === expected.length
        && expected.every(ref => releaseRefs.include.includes(ref)))
      || !Array.isArray(releaseRefs.exclude) || releaseRefs.exclude.length !== 0
      || !Array.isArray(releases[0].rules) || !releases[0].rules.some(rule => rule.type === 'creation')) {
    throw new Error('release-tags must protect creation of exactly the release tag refs');
  }
  for (const ruleset of rulesets) {
    if (expectedBypassActors.has(ruleset.name) || ruleset.rules?.some(rule => ['update', 'deletion'].includes(rule.type))) {
      if (!Array.isArray(ruleset.bypass_actors)) {
        throw new Error(`${ruleset.name}: bypass_actors unreadable; require Administration: read`);
      }
      const expected = expectedBypassActors.get(ruleset.name) ?? [];
      if (ruleset.bypass_actors.length !== expected.length
          || !ruleset.bypass_actors.every(actor => expected.some(value =>
            actor.actor_id === value.actor_id && actor.actor_type === value.actor_type
            && actor.bypass_mode === value.bypass_mode))) {
        throw new Error(`${ruleset.name}: tag immutability bypass actors detected`);
      }
    }
  }
}

function restClient(token) {
  const headers = { Authorization: `Bearer ${token}`, Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" };
  const fetchJson = async url => {
    const response = await fetch(url, { headers });
    if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
    return response;
  };
  const expand = (route, parameters) => {
    const path = route.replace(/^GET /, "").replace(/\{(\w+)\}/g, (_, key) => parameters[key]);
    return `https://api.github.com${path}`;
  };
  return {
    async paginate(route, parameters) {
      const { per_page } = parameters;
      let url = `${expand(route, parameters)}?per_page=${per_page}`;
      const items = [];
      while (url) {
        const response = await fetchJson(url);
        items.push(...await response.json());
        url = /<([^>]+)>;\s*rel="next"/.exec(response.headers.get("link") ?? "")?.[1];
      }
      return items;
    },
    async request(route, parameters) {
      const query = parameters.per_page === undefined ? "" : `?per_page=${parameters.per_page}`;
      return { data: await (await fetchJson(`${expand(route, parameters)}${query}`)).json() };
    },
  };
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [owner, repo] = (process.env.GITHUB_REPOSITORY ?? "").split("/");
  const releaseToml = await readFile(new URL("../.velnor/release.toml", import.meta.url), "utf8");
  await checkTagRulesets({ context: { repo: { owner, repo } }, process, getOctokit: restClient, releaseToml });
}
