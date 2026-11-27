// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { INPUT_FILES, type Inputs } from "./velnor-runner-install-config";
import { hostEnvironment, InstallError, run } from "./velnor-runner-install-lib";

const HARDENING = ["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null"];
const TOOLCHAIN_FILES = ["mise.toml", "mise.lock", "rust-toolchain.toml"];
const OBJECT_NAME = /^[0-9a-f]{40,64}$/;
const HIDDEN_FROM_STATUS = /^[a-zS] /;

export type Source = { readonly commit: string; readonly archive: Uint8Array; readonly toolchain: Uint8Array; readonly inputs: Inputs };

const gitEnvironment = (extra: Readonly<Record<string, string>> = {}): Record<string, string> =>
  hostEnvironment({
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_CONFIG_GLOBAL: "/dev/null",
    GIT_NO_REPLACE_OBJECTS: "1",
    GIT_OPTIONAL_LOCKS: "0",
    GIT_TERMINAL_PROMPT: "0",
    ...extra,
  });

async function git(label: string, args: readonly string[], extra: Readonly<Record<string, string>> = {}): Promise<string> {
  const outcome = await run(["git", ...HARDENING, ...args], { env: gitEnvironment(extra), output: "capture" });
  if (outcome.status !== 0) {
    throw new InstallError(`step 'source': git ${label} exited with ${outcome.status}`);
  }
  return outcome.stdout;
}

async function gitBytes(label: string, args: readonly string[]): Promise<Uint8Array> {
  const child = Bun.spawn(["git", ...HARDENING, ...args], { cwd: "/", env: gitEnvironment(), stdin: "ignore", stdout: "pipe", stderr: "inherit" });
  const [bytes, status] = await Promise.all([new Response(child.stdout).bytes(), child.exited]);
  if (status !== 0) {
    throw new InstallError(`step 'source': git ${label} exited with ${status}`);
  }
  return bytes;
}

async function repositoryFacts(repository: string): Promise<{ commit: string; index: string }> {
  const at = ["-C", repository];
  const commit = (await git("rev-parse", [...at, "rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"])).trim();
  const index = (await git("rev-parse", [...at, "rev-parse", "--path-format=absolute", "--git-path", "index"])).trim();
  if (!OBJECT_NAME.test(commit) || index === "") {
    throw new InstallError(`step 'source': ${repository} is not a usable git checkout`);
  }
  return { commit, index };
}

async function verified<T>(repository: string, commit: string, work: (gitDir: string) => Promise<T>): Promise<T> {
  const gitDir = await mkdtemp(join(tmpdir(), "velnor-runner-git-"));
  try {
    await git("init", ["init", "--bare", "--quiet", "--template=", gitDir]);
    await git("fetch", ["--git-dir", gitDir, "fetch", "--quiet", "--no-tags", "--depth=1", pathToFileURL(repository).href, commit]);
    await writeFile(join(gitDir, "HEAD"), `${commit}\n`);
    return await work(gitDir);
  } finally {
    await rm(gitDir, { recursive: true, force: true });
  }
}

async function refuseUnclean(repository: string, gitDir: string, index: string, commit: string): Promise<void> {
  const tree = ["--git-dir", gitDir, "-c", "core.bare=false", "--work-tree", repository];
  const env = { GIT_INDEX_FILE: index };
  const changes = (await git("status", [...tree, "status", "--porcelain=v1", "--untracked-files=no", "--ignore-submodules=all"], env)).trim();
  if (changes !== "") {
    const count = changes.split("\n").length;
    throw new InstallError(
      `${repository} has ${count} uncommitted change${count === 1 ? "" : "s"} in tracked files; velnor-runner is built from the exact HEAD commit ${commit}, so commit or stash them first`,
    );
  }
  const hidden = (await git("ls-files", [...tree, "ls-files", "-v", "-z"], env)).split("\0").filter((entry) => HIDDEN_FROM_STATUS.test(entry));
  if (hidden.length > 0) {
    throw new InstallError(
      `${repository} marks ${hidden.length} tracked file${hidden.length === 1 ? "" : "s"} assume-unchanged or skip-worktree (${hidden[0]?.slice(2) ?? ""}), so git cannot show whether they match HEAD ${commit}; clear the flags first`,
    );
  }
}

const blob = async (gitDir: string, commit: string, path: string): Promise<string> => git("cat-file", ["--git-dir", gitDir, "cat-file", "blob", `${commit}:${path}`]);

export async function prepareSource(repository: string): Promise<Source> {
  const { commit, index } = await repositoryFacts(repository);
  return verified(repository, commit, async (gitDir) => {
    await refuseUnclean(repository, gitDir, index, commit);
    const archive = ["--git-dir", gitDir, "archive", "--format=tar", commit];
    return {
      commit,
      archive: await gitBytes("archive", archive),
      toolchain: await gitBytes("archive", [...archive, "--", ...TOOLCHAIN_FILES]),
      inputs: {
        machine: await blob(gitDir, commit, INPUT_FILES.machine),
        digests: await blob(gitDir, commit, INPUT_FILES.digests),
        mise: await blob(gitDir, commit, INPUT_FILES.mise),
      },
    };
  });
}
