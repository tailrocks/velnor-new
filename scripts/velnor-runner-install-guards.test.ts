// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { chmod, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { deflateSync } from "node:zlib";
import { expect, test } from "bun:test";
import { INPUT_FILES, parseMise } from "./velnor-runner-install-config";
import {
  binary,
  calls,
  checkout,
  entries,
  gateFile,
  git,
  gitEnvironment,
  install,
  installCleanup,
  lockFile,
  machines,
  MARKER,
  sandbox,
  settled,
  start,
  TEST_TIMEOUT_MS,
  waitFor,
  WAIT_LIMIT_MS,
  within,
  type Box,
  type Repo,
} from "./velnor-runner-install.harness";

installCleanup();

const fullArchive = (cs: Awaited<ReturnType<typeof calls>>): string | undefined => cs.find((call) => call.command === "tar" && call.user === "velnor-build")?.stdinSha256;
const expectedArchive = (box: Box, repo: Repo): string =>
  createHash("sha256").update(Bun.spawnSync(["git", "-C", repo.dir, "archive", "--format=tar", "HEAD"], { env: gitEnvironment(box) }).stdout).digest("hex");

test(
  "a modified tracked file is refused before any guest call, staged or not; an untracked file is not",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    await writeFile(join(repo.dir, "a.txt"), "changed\n");
    const unstaged = await install(box, repo, binary("0.1.82"));
    git(box, repo.dir, "add", "a.txt");
    const staged = await install(box, repo, binary("0.1.82"));
    for (const refused of [unstaged, staged]) {
      expect(refused.status).not.toBe(0);
      expect(refused.stderr).toContain("1 uncommitted change in tracked files");
      expect(refused.stdout).toBe("");
    }
    expect(await calls(box)).toEqual([]);
    expect(existsSync(box.store)).toBe(false);
    git(box, repo.dir, "reset", "-q", "--hard");
    await writeFile(join(repo.dir, "untracked.txt"), "x\n");
    const allowed = await install(box, repo, binary("0.1.82"));
    expect(allowed.status, allowed.stderr).toBe(0);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a tracked file marked assume-unchanged or skip-worktree is refused although git reports a clean tree",
  async () => {
    for (const flag of ["--assume-unchanged", "--skip-worktree"]) {
      const box = await sandbox();
      const repo = await checkout(box);
      await writeFile(join(repo.dir, "a.txt"), "changed\n");
      git(box, repo.dir, "update-index", flag, "a.txt");
      expect(git(box, repo.dir, "status", "--porcelain", "--untracked-files=no").trim(), flag).toBe("");
      const refused = await install(box, repo, binary("0.1.82"));
      expect(refused.status, flag).not.toBe(0);
      expect(refused.stderr, flag).toContain("assume-unchanged or skip-worktree");
      expect(await calls(box), flag).toEqual([]);
    }
  },
  TEST_TIMEOUT_MS,
);

test(
  "the build prints and streams the exact HEAD commit, without untracked files",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    await writeFile(join(repo.dir, "untracked.txt"), "x\n");
    const head = git(box, repo.dir, "rev-parse", "HEAD").trim();
    const outcome = await install(box, repo, binary("0.1.82"));
    expect(outcome.status, outcome.stderr).toBe(0);
    expect(outcome.stderr).toContain(`building ${head}`);
    expect(fullArchive(await calls(box))).toBe(expectedArchive(box, repo));
  },
  TEST_TIMEOUT_MS,
);

test(
  "hostile repository state cannot run code or change what is built",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    await writeFile(join(repo.dir, ".gitattributes"), "a.txt filter=repoevil\nb.txt filter=globalevil\n");
    git(box, repo.dir, "add", "-A");
    git(box, repo.dir, "commit", "-q", "-m", "attributes");
    const expected = expectedArchive(box, repo);
    const marker = (name: string): string => join(box.root, `marker-${name}`);
    const quoted = (value: string): string => `'${value}'`;
    const hook = (name: string, role: string): string => [process.execPath, MARKER, marker(name), role].map(quoted).join(" ");
    const filter = (name: string): string => hook(name, "filter");
    git(box, repo.dir, "config", "filter.repoevil.smudge", filter("repo-smudge"));
    git(box, repo.dir, "config", "filter.repoevil.clean", filter("repo-clean"));
    git(box, repo.dir, "config", "core.fsmonitor", hook("fsmonitor", "fsmonitor"));
    await writeFile(join(repo.dir, ".git/info/attributes"), "* filter=repoevil\n");
    await writeFile(join(box.home, ".gitconfig"), `[filter "globalevil"]\n\tsmudge = ${filter("global-smudge")}\n\tclean = ${filter("global-clean")}\n`);
    const head = git(box, repo.dir, "rev-parse", "HEAD").trim();
    const other = git(box, repo.dir, "commit-tree", "-m", "other", "4b825dc642cb6eb9a060e54bf8d69288fbee4904").trim();
    git(box, repo.dir, "replace", head, other);
    const outcome = await install(box, repo, binary("0.1.82"));
    expect(outcome.status, outcome.stderr).toBe(0);
    expect(outcome.stderr).toContain(`building ${head}`);
    expect(fullArchive(await calls(box))).toBe(expected);
    for (const name of ["repo-smudge", "repo-clean", "fsmonitor", "global-smudge", "global-clean"]) {
      expect(existsSync(marker(name)), name).toBe(false);
    }
  },
  TEST_TIMEOUT_MS,
);

test(
  "a loose object of the checkout that was changed after the commit is rejected, not archived",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const blob = git(box, repo.dir, "rev-parse", "HEAD:a.txt").trim();
    const object = join(repo.dir, ".git/objects", blob.slice(0, 2), blob.slice(2));
    await chmod(object, 0o644);
    await writeFile(object, deflateSync(Buffer.from("blob 5\0evil\n")));
    const outcome = await install(box, repo, binary("0.1.82"));
    expect(outcome.status).not.toBe(0);
    expect(outcome.stderr).toContain("step 'source': git fetch exited");
    expect(await calls(box)).toEqual([]);
  },
  TEST_TIMEOUT_MS,
);

test(
  "files edited in the work tree right after the clean check change neither the machine, nor the pinned mise release, nor its digest that are read",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const show = (path: string): string => git(box, repo.dir, "show", `HEAD:${path}`);
    const expected = parseMise({ machine: "", digests: show(INPUT_FILES.digests), mise: show(INPUT_FILES.mise) });
    const gitTamper = [
      { path: join(repo.dir, INPUT_FILES.machine), content: show(INPUT_FILES.machine).replace('MACHINE_NAME: &str = "velnor"', 'MACHINE_NAME: &str = "evil"') },
      { path: join(repo.dir, INPUT_FILES.digests), content: show(INPUT_FILES.digests).replace(/[0-9a-f]{64}/g, "0".repeat(64)) },
      { path: join(repo.dir, INPUT_FILES.mise), content: show(INPUT_FILES.mise).replace(/^min_version = ".*"$/m, 'min_version = "2026.1.1"') },
    ];
    const outcome = await install(box, repo, binary("0.1.82"), { gitTamper });
    expect(outcome.status, outcome.stderr).toBe(0);
    expect(await Bun.file(gitTamper[0]?.path ?? "").text()).toContain('"evil"');
    expect(await machines(box)).toEqual(["velnor"]);
    const checked = (await calls(box)).filter((call) => call.checkedSha !== undefined);
    expect(checked.length).toBeGreaterThan(0);
    expect(checked.map((call) => call.checkedSha)).toEqual(checked.map(() => expected.sha256));
  },
  TEST_TIMEOUT_MS,
);

test(
  "a copy whose sha256 differs from the bytes that were verified is not installed",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    expect((await install(box, repo, binary("0.1.81"))).status).toBe(0);
    const failed = await install(box, repo, binary("0.1.82"), { copyCorrupt: true });
    expect(failed.status).not.toBe(0);
    expect(failed.stderr).toContain("step 'integrity'");
    expect(failed.stdout).toBe("");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.81"));
    expect(await entries(box)).toEqual(settled());
  },
  TEST_TIMEOUT_MS,
);

test(
  "orb list is parsed at the boundary: zero machines and junk entries create the machine, a wrong shape fails",
  async () => {
    const accepted = ['[]', '[null, 7, {"id": 1}, {"name": "other"}]'];
    for (const list of accepted) {
      const box = await sandbox();
      const outcome = await install(box, await checkout(box), binary("0.1.82"), { list });
      expect(outcome.status, `${list}: ${outcome.stderr}`).toBe(0);
      expect(await machines(box)).toEqual(["velnor"]);
    }
    for (const [list, message] of [['{"name": "velnor"}', "did not print a list"], ["not json", "did not print JSON"]] as const) {
      const box = await sandbox();
      const outcome = await install(box, await checkout(box), binary("0.1.82"), { list });
      expect(outcome.status).not.toBe(0);
      expect(outcome.stderr).toContain(`step 'machine-list': orb list ${message}`);
      expect(existsSync(box.state)).toBe(false);
    }
  },
  TEST_TIMEOUT_MS,
);

test(
  "a different host store is refused when the machine already links another one, after the run that holds the lock finishes",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    expect((await install(box, repo, binary("0.1.82"))).status).toBe(0);
    const other = { ...box, store: join(box.root, "store-b"), bin: join(box.root, "store-b/bin") };
    const started = join(box.root, "a-started");
    const gate = gateFile(box, "a-gate");
    const holder = await start(box, repo, binary("0.1.82"), { catStarted: started, catGate: gate }, "A");
    await waitFor("A's copy to start", () => Bun.file(started).exists());
    const refused = await start(other, repo, binary("0.1.82"), {}, "B");
    await waitFor("B to report that it waits for the install lock", () => refused.stderr().includes(`waiting for ${lockFile(box)}`));
    expect((await calls(box)).filter((call) => call.who === "B")).toEqual([]);
    await writeFile(gate, "");
    const [first, second] = await within("A and B to finish", WAIT_LIMIT_MS, Promise.all([holder.done, refused.done]));
    expect(first.status, first.stderr).toBe(0);
    expect(second.status).not.toBe(0);
    expect(second.stderr).toContain(`refusing to repoint it to /mnt/mac${other.store}`);
    expect(existsSync(other.store)).toBe(false);
    const state = JSON.parse(await Bun.file(box.state).text()) as { link: string };
    expect(state.link).toBe(`/mnt/mac${box.store}`);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a create that loses a race ('already exists') is tolerated",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    expect((await install(box, repo, binary("0.1.82"))).status).toBe(0);
    const raced = await install(box, repo, binary("0.1.82"), { hideMachines: true });
    expect(raced.status, raced.stderr).toBe(0);
    expect(raced.stdout).toBe("velnor-runner 0.1.82\n");
  },
  TEST_TIMEOUT_MS,
);
