// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import type { Machine, MiseBinary } from "./velnor-runner-install-config";
import { InstallError, orb, type Outcome } from "./velnor-runner-install-lib";
import type { Source } from "./velnor-runner-install-source";

export const BUILD_USER = "velnor-build";
const ROOT = "/var/lib/velnor-build";
const GUEST_PATH = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const TRIPLE = "aarch64-unknown-linux-gnu";
const PACKAGES = ["build-essential", "pkg-config", "libssl-dev", "ca-certificates", "curl"];
const APT_LOCK = "DPkg::Lock::Timeout=120";
const USERNAME_IN_USE = 9;
const LOCK_SECONDS = "900";

export type Paths = ReturnType<typeof layout>;
export type Context = { readonly machine: Machine; readonly paths: Paths; readonly installId: string };
type StepOptions = {
  readonly as?: "root" | "build";
  readonly cwd?: string;
  readonly env?: Readonly<Record<string, string>>;
  readonly stdin?: Uint8Array;
  readonly capture?: true;
  readonly accept?: readonly number[];
};

export function layout(key: string) {
  const work = `${ROOT}/work/${key}`;
  const toolchain = `${ROOT}/toolchain`;
  return {
    root: ROOT,
    home: `${ROOT}/home`,
    tmp: `${ROOT}/tmp`,
    bin: `${ROOT}/bin`,
    mise: `${ROOT}/bin/mise`,
    toolchain,
    toolchainDirectories: ["data", "cache", "state", "config", "cargo", "rustup"].map((name) => `${toolchain}/${name}`),
    work,
    cfg: `${work}/cfg`,
    out: `${work}/out`,
    output: `${work}/out/velnor-runner`,
    src: `${work}/src`,
    privateDirectories: ["src", "home", "tmp", "data", "cache", "state", "config", "cargo", "mbx"].map((name) => `${work}/${name}`),
    built: `${work}/src/target/${TRIPLE}/release/velnor-runner`,
  };
}

function environment(paths: Paths, options: StepOptions): Record<string, string> {
  if (options.as !== "build") {
    return { PATH: GUEST_PATH, HOME: paths.home, TMPDIR: paths.tmp, LANG: "C.UTF-8", ...options.env };
  }
  const work = paths.work;
  return {
    PATH: GUEST_PATH,
    HOME: `${work}/home`,
    TMPDIR: `${work}/tmp`,
    LANG: "C.UTF-8",
    MISE_DATA_DIR: `${work}/data`,
    MISE_CACHE_DIR: `${work}/cache`,
    MISE_STATE_DIR: `${work}/state`,
    MISE_CONFIG_DIR: `${work}/config`,
    MISE_TRUSTED_CONFIG_PATHS: paths.src,
    MISE_ENABLE_TOOLS: "rust,mr-boxington",
    MISE_AUTO_INSTALL: "0",
    MISE_EXEC_AUTO_INSTALL: "0",
    CARGO_HOME: `${work}/cargo`,
    RUSTUP_HOME: `${paths.toolchain}/rustup`,
    RUSTUP_AUTO_INSTALL: "0",
    MBX_CACHE_DIR: `${work}/mbx`,
    ...options.env,
  };
}

export function guestArgv(context: Context, command: readonly string[], options: StepOptions = {}): string[] {
  const assignments = Object.entries(environment(context.paths, options)).map(([name, value]) => `${name}=${value}`);
  return [
    "-m",
    context.machine.name,
    "-u",
    "root",
    "env",
    "-i",
    ...(options.cwd === undefined ? [] : ["-C", options.cwd]),
    ...assignments,
    ...(options.as === "build" ? ["runuser", "-u", BUILD_USER, "--"] : []),
    ...command,
  ];
}

export async function step(context: Context, label: string, command: readonly string[], options: StepOptions = {}): Promise<Outcome> {
  const outcome = await orb(guestArgv(context, command, options), {
    ...(options.stdin === undefined ? {} : { stdin: options.stdin }),
    ...(options.capture === undefined ? {} : { output: "capture" as const }),
  });
  if (outcome.status !== 0 && !(options.accept ?? []).includes(outcome.status)) {
    throw new InstallError(`step '${label}': ${command[0] ?? ""} exited with ${outcome.status}`);
  }
  return outcome;
}

const text = (value: string): Uint8Array => new TextEncoder().encode(value);

async function ensureMise(context: Context, mise: MiseBinary): Promise<void> {
  const { paths } = context;
  const verified = async (path: string): Promise<boolean> =>
    (await step(context, "mise-verify", ["sha256sum", "--check", "--strict", "--status", "-"], { stdin: text(`${mise.sha256}  ${path}\n`), accept: [1] })).status === 0;
  if (await verified(paths.mise)) {
    return;
  }
  const download = `${paths.bin}/mise.download-${context.installId}`;
  try {
    const url = `https://github.com/jdx/mise/releases/download/v${mise.version}/mise-v${mise.version}-linux-arm64`;
    await step(context, "mise-download", ["curl", "--proto", "=https", "--tlsv1.2", "-fsSL", "-o", download, url]);
    if (!(await verified(download))) {
      throw new InstallError(`step 'mise-verify': the mise ${mise.version} download does not match the pinned sha256 ${mise.sha256}`);
    }
    await step(context, "mise-install", ["chmod", "0755", download]);
    await step(context, "mise-install", ["mv", "-f", download, paths.mise]);
  } catch (error) {
    await step(context, "mise-discard", ["rm", "-f", download], { accept: [1] });
    throw error;
  }
}

async function ensurePackages(context: Context): Promise<void> {
  if ((await step(context, "packages-check", ["dpkg", "-s", ...PACKAGES], { capture: true, accept: [1] })).status === 0) {
    return;
  }
  await step(context, "packages-update", ["apt-get", "-o", APT_LOCK, "update", "-qq"]);
  await step(context, "packages-install", ["apt-get", "-o", APT_LOCK, "install", "-y", "-qq", ...PACKAGES], { env: { DEBIAN_FRONTEND: "noninteractive" } });
}

async function installToolchain(context: Context): Promise<void> {
  const { paths } = context;
  const toolchain = (name: string): string => `${paths.toolchain}/${name}`;
  await step(context, "toolchain", ["flock", "-w", LOCK_SECONDS, toolchain(".lock"), paths.mise, "install", "--locked", "rust", "mr-boxington"], {
    cwd: paths.cfg,
    env: {
      MISE_DATA_DIR: toolchain("data"),
      MISE_CACHE_DIR: toolchain("cache"),
      MISE_STATE_DIR: toolchain("state"),
      MISE_CONFIG_DIR: toolchain("config"),
      MISE_TRUSTED_CONFIG_PATHS: paths.cfg,
      CARGO_HOME: toolchain("cargo"),
      RUSTUP_HOME: toolchain("rustup"),
    },
  });
  const loose = await step(context, "toolchain-permissions", ["find", paths.toolchain, paths.bin, "!", "-type", "l", "(", "-perm", "/022", "-o", "!", "-user", "root", ")", "-print", "-quit"], {
    capture: true,
  });
  if (loose.stdout.trim() !== "") {
    throw new InstallError(`step 'toolchain-permissions': ${loose.stdout.trim()} is not root-owned and read-only for other users`);
  }
}

export async function provision(context: Context, mise: MiseBinary, source: Source): Promise<void> {
  const { paths } = context;
  const root = ["-o", "root", "-g", "root"];
  await step(context, "layout", ["install", "-d", "-m", "0755", ...root, paths.root, paths.bin, paths.toolchain, ...paths.toolchainDirectories, `${paths.root}/work`]);
  await step(context, "layout-private", ["install", "-d", "-m", "0700", ...root, paths.home, paths.tmp]);
  await step(context, "user", ["useradd", "--system", "--user-group", "--no-create-home", "--shell", "/usr/sbin/nologin", BUILD_USER], { accept: [USERNAME_IN_USE] });
  await ensurePackages(context);
  await ensureMise(context, mise);
  await step(context, "wipe", ["rm", "-rf", paths.work]);
  await step(context, "workspace", ["install", "-d", "-m", "0755", ...root, paths.work, paths.cfg, paths.out]);
  await step(context, "workspace-private", ["install", "-d", "-m", "0700", "-o", BUILD_USER, "-g", BUILD_USER, ...paths.privateDirectories]);
  await step(context, "extract-config", ["tar", "-x", "-f", "-", "-C", paths.cfg, "--no-same-owner"], { stdin: source.toolchain });
  await step(context, "extract-source", ["tar", "-x", "-f", "-", "-C", paths.src, "--no-same-owner"], { as: "build", stdin: source.archive });
  await installToolchain(context);
  await step(context, "link-installs", ["ln", "-s", `${paths.toolchain}/data/installs`, `${paths.work}/data/installs`], { as: "build" });
  await step(context, "link-cargo", ["ln", "-s", `${paths.toolchain}/cargo/bin`, `${paths.work}/cargo/bin`], { as: "build" });
}

export async function build(context: Context): Promise<string> {
  const { paths } = context;
  const inSource = { as: "build", cwd: paths.src } as const;
  const resolved = await step(context, "cargo-wrapper", [paths.mise, "exec", "--", "which", "-a", "cargo"], { ...inSource, capture: true });
  const expected = `${paths.work}/data/command-wrappers/bin/cargo`;
  const first = resolved.stdout.split("\n")[0];
  if (first !== expected) {
    throw new InstallError(`step 'cargo-wrapper': cargo resolves to '${first ?? ""}', not to the mise wrapper ${expected}`);
  }
  await step(context, "mbx-doctor", [paths.mise, "exec", "--", "mbx", "doctor"], inSource);
  await step(context, "build", [paths.mise, "exec", "--", "cargo", "build", "--release", "--locked", "-p", "velnor-runner", "--target", TRIPLE], inSource);
  await step(context, "reap", ["pkill", "-KILL", "-u", BUILD_USER], { accept: [1] });
  const regular = await step(context, "collect-check", ["find", paths.built, "-maxdepth", "0", "-type", "f", "-user", BUILD_USER, "-print"], { capture: true });
  if (regular.stdout.trim() !== paths.built) {
    throw new InstallError(`step 'collect-check': ${paths.built} is not a regular file built by ${BUILD_USER}`);
  }
  await step(context, "collect", ["install", "-m", "0755", "-o", "root", "-g", "root", paths.built, paths.output]);
  const hashed = await step(context, "hash", ["sha256sum", paths.output], { capture: true });
  const sha256 = /^([0-9a-f]{64}) /m.exec(hashed.stdout)?.[1];
  if (sha256 === undefined) {
    throw new InstallError(`step 'hash': sha256sum printed ${JSON.stringify(hashed.stdout.slice(0, 200))}, stderr ${JSON.stringify(hashed.stderr.slice(-200))}`);
  }
  return sha256;
}
