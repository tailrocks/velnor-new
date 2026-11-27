// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { chmod, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { expect, test } from "bun:test";
import {
  binary,
  calls,
  checkout,
  checkoutLock,
  entries,
  gateFile,
  git,
  gitEnvironment,
  install,
  installCleanup,
  machines,
  sandbox,
  settled,
  start,
  TEMP_PREFIX,
  TEST_TIMEOUT_MS,
  waitFor,
  WAIT_LIMIT_MS,
  within,
  type Box,
  type Repo,
} from "./velnor-runner-install.harness";

installCleanup();

const BUILD_ROOT = "/var/lib/velnor-build";
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
    const filter = (name: string): string => `touch ${marker(name)}; cat`;
    const hook = join(box.root, "fsmonitor.sh");
    await writeFile(hook, `#!/bin/sh\ntouch ${marker("fsmonitor")}\n`);
    await chmod(hook, 0o755);
    git(box, repo.dir, "config", "filter.repoevil.smudge", filter("repo-smudge"));
    git(box, repo.dir, "config", "filter.repoevil.clean", filter("repo-clean"));
    git(box, repo.dir, "config", "core.fsmonitor", hook);
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
  "a copy whose sha256 differs from the machine's digest is not installed",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    expect((await install(box, repo, binary("0.1.81"))).status).toBe(0);
    const failed = await install(box, repo, binary("0.1.82"), { guestSha: "0".repeat(64) });
    expect(failed.status).not.toBe(0);
    expect(failed.stderr).toContain("step 'integrity'");
    expect(failed.stdout).toBe("");
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.81"));
    expect(await entries(box)).toEqual(settled(repo));
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
  "a failing step names itself in the error",
  async () => {
    const steps = [
      ["install --locked rust mr-boxington", "toolchain"],
      ["cargo build", "build"],
      ["sha256sum /var/lib", "hash"],
      ["cp /var/lib", "copy"],
    ] as const;
    const outcomes = await Promise.all(
      steps.map(async ([failStep]) => {
        const box = await sandbox();
        return install(box, await checkout(box), binary("0.1.82"), { failStep });
      }),
    );
    steps.forEach(([, label], index) => {
      expect(outcomes[index]?.status).not.toBe(0);
      expect(outcomes[index]?.stderr).toContain(`step '${label}'`);
    });
  },
  TEST_TIMEOUT_MS,
);

test(
  "a different host store is refused when the machine already links another one",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    expect((await install(box, repo, binary("0.1.82"))).status).toBe(0);
    const other = { ...box, store: join(box.root, "store-b"), bin: join(box.root, "store-b/bin") };
    const refused = await (await start(other, repo, binary("0.1.82"))).done;
    expect(refused.status).not.toBe(0);
    expect(refused.stderr).toContain(`refusing to repoint it to /mnt/mac${other.store}`);
    expect(existsSync(other.store)).toBe(false);
    const state = JSON.parse(await Bun.file(box.state).text()) as { link: string };
    expect(state.link).toBe(`/mnt/mac${box.store}`);
  },
  TEST_TIMEOUT_MS,
);

test(
  "every guest call is root under env -i with a private TMPDIR and no agent or orb variables; cargo runs as the build user",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const outcome = await install(box, repo, binary("0.1.82"));
    expect(outcome.status, outcome.stderr).toBe(0);
    const all = await calls(box);
    const guest = all.filter((call) => call.command !== undefined);
    const work = `${BUILD_ROOT}/work/${repo.key}`;
    for (const call of all) {
      expect(call.env).not.toContain("SSH_AUTH_SOCK");
      expect(call.env).not.toContain("ORBENV");
      expect(call.args.join(" ")).not.toContain("SSH_AUTH_SOCK");
    }
    for (const call of guest) {
      expect(call.args.slice(0, 5), call.command).toEqual(["-m", "velnor", "-u", "root", "env"]);
      expect(call.args[5]).toBe("-i");
      expect(call.assignments?.TMPDIR?.startsWith(BUILD_ROOT), call.command).toBe(true);
      expect(Object.keys(call.assignments ?? {})).not.toContain("SSH_AUTH_SOCK");
    }
    const apt = guest.filter((call) => call.command === "apt-get");
    expect(apt.length).toBeGreaterThan(0);
    for (const call of apt) {
      expect(call.operands).toContain("DPkg::Lock::Timeout=120");
    }
    const toolchain = guest.filter((call) => call.operands?.[0] === "install" && call.operands.includes("--locked"));
    expect(toolchain.map((call) => [call.user, call.operands])).toEqual([["root", ["install", "--locked", "rust", "mr-boxington"]]]);
    const cargo = guest.find((call) => call.operands?.includes("cargo") && call.operands.includes("build"));
    expect(cargo?.user).toBe("velnor-build");
    expect(cargo?.assignments?.CARGO_HOME).toBe(`${work}/cargo`);
    expect(cargo?.assignments?.TMPDIR).toBe(`${work}/tmp`);
    expect(guest.filter((call) => call.operands?.includes("cargo") && call.user !== "velnor-build").map((call) => call.operands?.join(" "))).toEqual([]);
    const hashAt = guest.findIndex((call) => call.command === "sha256sum" && !call.operands?.includes("--check"));
    const copyAt = guest.findIndex((call) => call.command === "cp");
    expect(hashAt).toBeGreaterThan(-1);
    expect(copyAt).toBeGreaterThan(hashAt);
    expect(guest[copyAt]?.operands?.[0]).toBe(`${work}/out/velnor-runner`);
    expect(guest[copyAt]?.operands?.[1]?.startsWith(`/var/lib/velnor/bin/${TEMP_PREFIX}`)).toBe(true);
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

test(
  "a mise download that fails the pinned digest is deleted and fails the install; a verified one is installed once",
  async () => {
    const bad = await sandbox();
    const failed = await install(bad, await checkout(bad), binary("0.1.82"), { miseBadDownload: true });
    expect(failed.status).not.toBe(0);
    expect(failed.stderr).toContain("does not match the pinned sha256");
    const badCalls = await calls(bad);
    expect(badCalls.some((call) => call.command === "rm" && call.operands?.some((operand) => operand.includes("/bin/mise.download-")))).toBe(true);
    expect(badCalls.some((call) => call.command === "mv")).toBe(false);
    const good = await sandbox();
    const repo = await checkout(good);
    expect((await install(good, repo, binary("0.1.82"))).status).toBe(0);
    expect((await install(good, repo, binary("0.1.82"))).status).toBe(0);
    const goodCalls = (await calls(good)).filter((call) => call.command !== undefined);
    expect(goodCalls.filter((call) => call.command === "curl")).toHaveLength(1);
    expect(goodCalls.filter((call) => call.command === "mv" && call.operands?.[2]?.endsWith("/bin/mise"))).toHaveLength(1);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a second install of the same checkout waits for the first to finish all guest work",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const started = join(box.root, "a-started");
    const gate = gateFile(box, "a-gate");
    const a = await start(box, repo, binary("0.1.81"), { catStarted: started, catGate: gate }, "A");
    await waitFor("A's copy to start", () => Bun.file(started).exists());
    const b = await start(box, repo, binary("0.1.82"), {}, "B");
    const lock = join(box.bin, checkoutLock(repo));
    await waitFor("B to report that it waits for the checkout lock", () => {
      if (b.finished()) {
        throw new Error(`B exited while A held the lock: ${b.stderr()}`);
      }
      return b.stderr().includes(`waiting for ${lock}`);
    });
    const early = (await calls(box)).filter((call) => call.who === "B" && call.command !== undefined && call.command !== "readlink");
    expect(early.map((call) => call.command)).toEqual([]);
    await writeFile(gate, "");
    const [first, second] = await within("A and B to finish", WAIT_LIMIT_MS, Promise.all([a.done, b.done]));
    expect(first.status, first.stderr).toBe(0);
    expect(second.status, second.stderr).toBe(0);
    expect(await Bun.file(join(box.bin, "velnor-runner")).text()).toBe(binary("0.1.82"));
  },
  TEST_TIMEOUT_MS,
);

test(
  "a toolchain tree that is writable or not owned by root stops the build before cargo runs",
  async () => {
    const box = await sandbox();
    const outcome = await install(box, await checkout(box), binary("0.1.82"), { tainted: `${BUILD_ROOT}/toolchain/data/evil` });
    expect(outcome.status).not.toBe(0);
    expect(outcome.stderr).toContain("step 'toolchain-permissions'");
    expect((await calls(box)).some((call) => call.operands?.includes("cargo"))).toBe(false);
    expect(await readdir(box.bin)).not.toContain("velnor-runner");
  },
  TEST_TIMEOUT_MS,
);
