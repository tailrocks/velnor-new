// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { hostEnvironment, InstallError, run } from "./velnor-runner-install-lib";

const HARDENING = ["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null"];
const TOOLCHAIN_FILES = ["mise.toml", "mise.lock", "rust-toolchain.toml"];
const OBJECT_NAME = /^[0-9a-f]{40,64}$/;

export const isObjectName = (text: string): boolean => OBJECT_NAME.test(text);

const gitEnvironment = (extra: Readonly<Record<string, string>> = {}): Record<string, string> =>
  hostEnvironment({
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_CONFIG_GLOBAL: "/dev/null",
    GIT_NO_REPLACE_OBJECTS: "1",
    GIT_OPTIONAL_LOCKS: "0",
    GIT_TERMINAL_PROMPT: "0",
    ...extra,
  });

async function git(args: readonly string[], extra: Readonly<Record<string, string>> = {}): Promise<string> {
  const outcome = await run(["git", ...HARDENING, ...args], { env: gitEnvironment(extra), output: "capture" });
  if (outcome.status !== 0) {
    throw new InstallError(`step 'source': git ${args.find((word) => !word.startsWith("-") && !word.includes("=")) ?? ""} exited with ${outcome.status}`);
  }
  return outcome.stdout;
}

async function gitBytes(args: readonly string[]): Promise<Uint8Array> {
  const child = Bun.spawn(["git", ...HARDENING, ...args], { cwd: "/", env: gitEnvironment(), stdin: "ignore", stdout: "pipe", stderr: "inherit" });
  const [bytes, status] = await Promise.all([new Response(child.stdout).bytes(), child.exited]);
  if (status !== 0) {
    throw new InstallError(`step 'source': git archive exited with ${status}`);
  }
  return bytes;
}

async function repositoryFacts(repository: string): Promise<{ commit: string; objects: string; index: string }> {
  const at = ["-C", repository];
  const commit = (await git([...at, "rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"])).trim();
  const objects = (await git([...at, "rev-parse", "--path-format=absolute", "--git-path", "objects"])).trim();
  const index = (await git([...at, "rev-parse", "--path-format=absolute", "--git-path", "index"])).trim();
  if (!isObjectName(commit) || objects === "" || index === "") {
    throw new InstallError(`step 'source': ${repository} is not a usable git checkout`);
  }
  return { commit, objects, index };
}

async function pristine<T>(objects: string, commit: string, work: (gitDir: string) => Promise<T>): Promise<T> {
  const gitDir = await mkdtemp(join(tmpdir(), "velnor-runner-git-"));
  try {
    await git(["init", "--bare", "--quiet", "--template=", gitDir]);
    await writeFile(join(gitDir, "objects/info/alternates"), `${objects}\n`);
    await writeFile(join(gitDir, "HEAD"), `${commit}\n`);
    return await work(gitDir);
  } finally {
    await rm(gitDir, { recursive: true, force: true });
  }
}

export async function cleanCommit(repository: string): Promise<string> {
  const { commit, objects, index } = await repositoryFacts(repository);
  const changes = await pristine(objects, commit, (gitDir) =>
    git(
      ["--git-dir", gitDir, "-c", "core.bare=false", "--work-tree", repository, "status", "--porcelain=v1", "--untracked-files=no", "--ignore-submodules=all"],
      { GIT_INDEX_FILE: index },
    ),
  );
  if (changes.trim() !== "") {
    const count = changes.trim().split("\n").length;
    throw new InstallError(
      `${repository} has ${count} uncommitted change${count === 1 ? "" : "s"} in tracked files; velnor-runner is built from the exact HEAD commit ${commit}, so commit or stash them first`,
    );
  }
  return commit;
}

export type Source = { readonly archive: Uint8Array; readonly toolchain: Uint8Array };

export async function archiveCommit(repository: string, commit: string): Promise<Source> {
  const { objects } = await repositoryFacts(repository);
  return pristine(objects, commit, async (gitDir) => {
    const archive = ["--git-dir", gitDir, "archive", "--format=tar", commit];
    return { archive: await gitBytes(archive), toolchain: await gitBytes([...archive, "--", ...TOOLCHAIN_FILES]) };
  });
}
