// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { expect, test } from "bun:test";
import { readdir } from "node:fs/promises";
import {
  binary,
  calls,
  checkout,
  install,
  installCleanup,
  MASKED_MOUNTS,
  MOUNT_INFO,
  sandbox,
  TEMP_PREFIX,
  TEST_TIMEOUT_MS,
  type Call,
} from "./velnor-runner-install.harness";

installCleanup();

const BUILD_ROOT = "/var/lib/velnor-build";
const WORK = `${BUILD_ROOT}/work`;
const NAMESPACE_HOLDER = "/run/velnor-build-namespace";
const guestCalls = (all: Call[]): Call[] => all.filter((call) => call.command !== undefined);
const isCargoBuild = (call: Call): boolean => call.operands?.includes("cargo") === true && call.operands.includes("build");
const indexOf = (all: Call[], match: (call: Call) => boolean): number => all.findIndex(match);

test(
  "a failing step names itself in the error",
  async () => {
    const steps = [
      [{ command: "/mise", operand: "install" }, "toolchain"],
      [{ command: "/mise", operand: "build" }, "build"],
      [{ command: "sha256sum", operand: `${WORK}/out/velnor-runner` }, "hash"],
      [{ command: "cp" }, "copy"],
    ] as const;
    const outcomes = await Promise.all(
      steps.map(async ([failOn]) => {
        const box = await sandbox();
        return install(box, await checkout(box), binary("0.1.82"), { failOn });
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
  "every guest call is root under env -i with a private TMPDIR and no agent or orb variables; cargo runs as the build user inside the namespace",
  async () => {
    const box = await sandbox();
    const repo = await checkout(box);
    const outcome = await install(box, repo, binary("0.1.82"));
    expect(outcome.status, outcome.stderr).toBe(0);
    const all = await calls(box);
    const guest = guestCalls(all);
    for (const call of all) {
      expect(call.env).not.toContain("SSH_AUTH_SOCK");
      expect(call.env).not.toContain("ORBENV");
      expect(call.args.join(" ")).not.toContain("SSH_AUTH_SOCK");
    }
    for (const call of guest) {
      expect(call.args.slice(0, 6), call.command).toEqual(["-m", "velnor", "-u", "root", "env", "-i"]);
      expect(call.outer?.TMPDIR?.startsWith(BUILD_ROOT), call.command).toBe(true);
      expect(Object.keys(call.assignments ?? {})).not.toContain("SSH_AUTH_SOCK");
    }
    const apt = guest.filter((call) => call.command === "apt-get");
    expect(apt.length).toBeGreaterThan(0);
    for (const call of apt) {
      expect(call.operands).toContain("DPkg::Lock::Timeout=120");
    }
    const toolchain = guest.filter((call) => call.operands?.[0] === "install" && call.operands.includes("--locked"));
    expect(toolchain.map((call) => [call.user, call.operands])).toEqual([["root", ["install", "--locked", "rust", "mr-boxington"]]]);
    const cargo = guest.find(isCargoBuild);
    expect(cargo?.user).toBe("velnor-build");
    expect(cargo?.namespaced).toBe(true);
    expect(cargo?.assignments?.CARGO_HOME).toBe(`${WORK}/cargo`);
    expect(cargo?.assignments?.TMPDIR).toBe(`${WORK}/tmp`);
    expect(cargo?.assignments?.HOME).toBe(`${WORK}/home`);
    expect(guest.filter((call) => call.operands?.includes("cargo") && call.user !== "velnor-build").map((call) => call.operands?.join(" "))).toEqual([]);
    const hashAt = indexOf(guest, (call) => call.command === "sha256sum" && !call.operands?.includes("--check"));
    const copyAt = indexOf(guest, (call) => call.command === "cp");
    expect(hashAt).toBeGreaterThan(-1);
    expect(copyAt).toBeGreaterThan(hashAt);
    expect(guest[copyAt]?.operands?.[0]).toBe(`${WORK}/out/velnor-runner`);
    expect(guest[copyAt]?.operands?.[1]?.startsWith(`/var/lib/velnor/bin/${TEMP_PREFIX}`)).toBe(true);
  },
  TEST_TIMEOUT_MS,
);

test(
  "the build user only runs inside a private mount namespace that masks every host file mount, and nothing runs as it before the masks are checked",
  async () => {
    const box = await sandbox();
    const outcome = await install(box, await checkout(box), binary("0.1.82"));
    expect(outcome.status, outcome.stderr).toBe(0);
    const guest = guestCalls(await calls(box));
    const masks = guest.filter((call) => call.command === "mount" && call.namespaced === true && call.operands?.includes("tmpfs"));
    expect(masks.map((call) => call.operands?.at(-1)).sort()).toEqual([...MASKED_MOUNTS].sort());
    for (const call of masks) {
      expect(call.operands?.join(" ")).toContain("ro,nosuid,nodev,noexec");
    }
    const unshare = guest.find((call) => call.command === "unshare");
    expect(unshare?.operands).toEqual([`--mount=${NAMESPACE_HOLDER}/mnt`, "--propagation", "private", "--", "true"]);
    const firstBuildUser = indexOf(guest, (call) => call.user === "velnor-build");
    const checkAt = indexOf(guest, (call) => call.command === "find" && call.operands?.includes("-mindepth") === true);
    expect(guest[checkAt]?.user).toBe("velnor-build");
    expect(checkAt).toBe(firstBuildUser);
    expect(indexOf(guest, (call) => call.command === "unshare")).toBeLessThan(indexOf(guest, (call) => call.command === "mount" && call.namespaced === true));
    expect(guest.filter((call) => call.user === "velnor-build").every((call) => call.namespaced === true)).toBe(true);
    expect(guest.filter((call) => call.user === "velnor-build").length).toBeGreaterThan(4);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a mask that does not hide the mount, or a machine that lists no host mounts, stops the install before anything runs as the build user",
  async () => {
    const leaky = await sandbox();
    const leaked = await install(leaky, await checkout(leaky), binary("0.1.82"), { maskIneffective: true });
    const bare = await sandbox();
    const noMounts = MOUNT_INFO.split("\n").filter((line) => !line.includes("virtiofs") && !line.includes("/mnt/machines")).join("\n");
    const none = await install(bare, await checkout(bare), binary("0.1.82"), { mountInfo: noMounts });
    expect(leaked.status).not.toBe(0);
    expect(leaked.stderr).toContain("step 'namespace-check'");
    expect(none.status).not.toBe(0);
    expect(none.stderr).toContain("step 'namespace-mounts'");
    for (const box of [leaky, bare]) {
      const guest = guestCalls(await calls(box));
      expect(guest.some(isCargoBuild)).toBe(false);
      expect(guest.some((call) => call.command === "tar" && call.user === "velnor-build")).toBe(false);
      expect(guest.some((call) => call.command === "cp")).toBe(false);
    }
  },
  TEST_TIMEOUT_MS,
);

test(
  "a cargo that resolves outside the mise wrapper stops the build before cargo runs",
  async () => {
    const box = await sandbox();
    const outcome = await install(box, await checkout(box), binary("0.1.82"), { cargoOutsideWrapper: true });
    expect(outcome.status).not.toBe(0);
    expect(outcome.stderr).toContain("step 'cargo-wrapper'");
    expect(guestCalls(await calls(box)).some(isCargoBuild)).toBe(false);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a build output that is not a regular file of the build user is neither collected nor copied",
  async () => {
    const box = await sandbox();
    const outcome = await install(box, await checkout(box), binary("0.1.82"), { collectRejected: true });
    expect(outcome.status).not.toBe(0);
    expect(outcome.stderr).toContain("step 'collect-check'");
    const guest = guestCalls(await calls(box));
    expect(guest.some((call) => call.command === "install" && call.operands?.at(-1) === `${WORK}/out/velnor-runner`)).toBe(false);
    expect(guest.some((call) => call.command === "cp")).toBe(false);
    expect(await readdir(box.bin)).not.toContain("velnor-runner");
  },
  TEST_TIMEOUT_MS,
);

test(
  "the build user's processes are killed before the output is collected, and the namespace and work directory are removed after a success and after a failure",
  async () => {
    const box = await sandbox();
    expect((await install(box, await checkout(box), binary("0.1.82"))).status).toBe(0);
    const guest = guestCalls(await calls(box));
    const reaps = guest.flatMap((call, index) => (call.command === "pkill" ? [index] : []));
    expect(reaps).toHaveLength(2);
    expect(reaps[0]).toBeGreaterThan(indexOf(guest, isCargoBuild));
    expect(reaps[0]).toBeLessThan(indexOf(guest, (call) => call.command === "find" && call.operands?.includes("-maxdepth") === true));
    const tail = guest.slice(reaps[1]);
    expect(tail.map((call) => [call.command, call.operands?.at(-1)])).toEqual([
      ["pkill", "velnor-build"],
      ["mountpoint", NAMESPACE_HOLDER],
      ["umount", NAMESPACE_HOLDER],
      ["rmdir", NAMESPACE_HOLDER],
      ["rm", WORK],
    ]);
    const failed = await sandbox();
    const outcome = await install(failed, await checkout(failed), binary("0.1.82"), { failOn: { command: "/mise", operand: "build" } });
    expect(outcome.status).not.toBe(0);
    const afterFailure = guestCalls(await calls(failed));
    expect(afterFailure.slice(-5).map((call) => [call.command, call.operands?.at(-1)])).toEqual(tail.map((call) => [call.command, call.operands?.at(-1)]));
    expect(afterFailure.some((call) => call.command === "cp")).toBe(false);
  },
  TEST_TIMEOUT_MS,
);

test(
  "a failed useradd because the password database is locked is retried; a user that never appears fails the install",
  async () => {
    const racing = await sandbox();
    const retried = await install(racing, await checkout(racing), binary("0.1.82"), { useraddFailures: 2 });
    expect(retried.status, retried.stderr).toBe(0);
    expect(guestCalls(await calls(racing)).filter((call) => call.command === "useradd")).toHaveLength(3);
    const broken = await sandbox();
    const failed = await install(broken, await checkout(broken), binary("0.1.82"), { useraddFailures: 99 });
    expect(failed.status).not.toBe(0);
    expect(failed.stderr).toContain("step 'user'");
    expect(guestCalls(await calls(broken)).filter((call) => call.command === "useradd")).toHaveLength(5);
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
    const goodCalls = guestCalls(await calls(good));
    expect(goodCalls.filter((call) => call.command === "curl")).toHaveLength(1);
    expect(goodCalls.filter((call) => call.command === "mv" && call.operands?.[2]?.endsWith("/bin/mise"))).toHaveLength(1);
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
    expect(guestCalls(await calls(box)).some(isCargoBuild)).toBe(false);
    expect(await readdir(box.bin)).not.toContain("velnor-runner");
  },
  TEST_TIMEOUT_MS,
);
