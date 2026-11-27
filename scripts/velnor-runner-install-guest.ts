// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import type { Machine, MiseBinary } from "./velnor-runner-install-config";
import { InstallError, messageOf, orb, type Outcome } from "./velnor-runner-install-lib";
import type { Source } from "./velnor-runner-install-source";

const BUILD_USER = "velnor-build";
const ROOT = "/var/lib/velnor-build";
const GUEST_PATH = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
const TRIPLE = "aarch64-unknown-linux-gnu";
const PACKAGES = ["build-essential", "pkg-config", "libssl-dev", "ca-certificates", "curl"];
const APT_LOCK = "DPkg::Lock::Timeout=120";
const USERNAME_IN_USE = 9;
const PASSWD_LOCKED = 1;
const USER_ATTEMPTS = 5;
const REAP_ATTEMPTS = 50;
const REAP_PAUSE_MS = 100;
const ABSENT = 1;
const NOT_A_MOUNT_POINT = 32;
const NAMESPACE_DIRECTORY = "/run/velnor-build-namespace";
const NAMESPACE = `${NAMESPACE_DIRECTORY}/mnt`;
const HOST_FILESYSTEM = /^(virtiofs|9p|fuse(\..+)?)$/;
const MACHINES_DIRECTORY = "/mnt/machines";
const MASK_OPTIONS = "ro,nosuid,nodev,noexec,size=4k";

export type Paths = ReturnType<typeof layout>;
export type Context = { readonly machine: Machine; readonly paths: Paths; readonly installId: string };
type StepOptions = {
  readonly as?: "root" | "build";
  readonly cwd?: string;
  readonly env?: Readonly<Record<string, string>>;
  readonly stdin?: Uint8Array;
  readonly capture?: true;
  readonly bytes?: true;
  readonly accept?: readonly number[];
};

export function layout() {
  const work = `${ROOT}/work`;
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

const rootEnvironment = (paths: Paths): Record<string, string> => ({ PATH: GUEST_PATH, HOME: paths.home, TMPDIR: paths.tmp, LANG: "C.UTF-8" });

function buildEnvironment(paths: Paths, extra: Readonly<Record<string, string>> = {}): Record<string, string> {
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
    ...extra,
  };
}

const clearedEnvironment = (environment: Readonly<Record<string, string>>, cwd?: string): string[] => [
  "env",
  "-i",
  ...(cwd === undefined ? [] : ["-C", cwd]),
  ...Object.entries(environment).map(([name, value]) => `${name}=${value}`),
];

const dropToBuildUser = [
  "nsenter",
  `--mount=${NAMESPACE}`,
  "--",
  "setpriv",
  `--reuid=${BUILD_USER}`,
  `--regid=${BUILD_USER}`,
  "--init-groups",
  "--no-new-privs",
  "--bounding-set=-all",
  "--inh-caps=-all",
  "--",
];

export function guestArgv(context: Context, command: readonly string[], options: StepOptions = {}): string[] {
  const { machine, paths } = context;
  const prefix = ["-m", machine.name, "-u", "root"];
  if (options.as !== "build") {
    return [...prefix, ...clearedEnvironment({ ...rootEnvironment(paths), ...options.env }, options.cwd), ...command];
  }
  return [...prefix, ...clearedEnvironment(rootEnvironment(paths)), ...dropToBuildUser, ...clearedEnvironment(buildEnvironment(paths, options.env), options.cwd), ...command];
}

export async function step(context: Context, label: string, command: readonly string[], options: StepOptions = {}): Promise<Outcome> {
  const output = options.bytes === undefined ? (options.capture === undefined ? undefined : ("capture" as const)) : ("bytes" as const);
  const outcome = await orb(guestArgv(context, command, options), {
    ...(options.stdin === undefined ? {} : { stdin: options.stdin }),
    ...(output === undefined ? {} : { output }),
  });
  if (outcome.status !== 0 && !(options.accept ?? []).includes(outcome.status)) {
    throw new InstallError(`step '${label}': ${command[0] ?? ""} exited with ${outcome.status}`);
  }
  return outcome;
}

const text = (value: string): Uint8Array => new TextEncoder().encode(value);
const unescapeMountInfo = (field: string): string => field.replace(/\\([0-7]{3})/g, (_, octal: string) => String.fromCharCode(Number.parseInt(octal, 8)));
const within = (path: string, parent: string): boolean => path === parent || path.startsWith(`${parent}/`);

export function hostMounts(mountInfo: string): string[] {
  const found = new Set<string>();
  for (const line of mountInfo.split("\n")) {
    const split = line.indexOf(" - ");
    const mountPoint = unescapeMountInfo(line.slice(0, Math.max(split, 0)).split(" ")[4] ?? "");
    const filesystem = line.slice(split + 3).split(" ")[0] ?? "";
    if (split < 0 || mountPoint === "") {
      continue;
    }
    if (HOST_FILESYSTEM.test(filesystem)) {
      if (mountPoint === "/") {
        throw new InstallError("step 'namespace-mounts': the machine mounts a host filesystem at /");
      }
      found.add(mountPoint);
    } else if (within(mountPoint, MACHINES_DIRECTORY)) {
      found.add(MACHINES_DIRECTORY);
    }
  }
  const kept: string[] = [];
  for (const mountPoint of [...found].sort((a, b) => a.length - b.length || Number(a > b) - Number(a < b))) {
    if (!kept.some((parent) => within(mountPoint, parent))) {
      kept.push(mountPoint);
    }
  }
  return kept;
}

async function unmountHolder(context: Context, label: string): Promise<void> {
  const mounted = await step(context, label, ["mountpoint", "--quiet", NAMESPACE_DIRECTORY], { accept: [ABSENT, NOT_A_MOUNT_POINT] });
  if (mounted.status === 0) {
    await step(context, label, ["umount", "--recursive", "--lazy", NAMESPACE_DIRECTORY]);
  }
}

async function enterNamespace(context: Context): Promise<void> {
  const inNamespace = (command: readonly string[]): string[] => ["nsenter", `--mount=${NAMESPACE}`, "--", ...command];
  await step(context, "namespace-holder", ["install", "-d", "-m", "0700", "-o", "root", "-g", "root", NAMESPACE_DIRECTORY]);
  await unmountHolder(context, "namespace-reset");
  await step(context, "namespace-holder", ["mount", "-t", "tmpfs", "-o", "size=64k,mode=0700,nosuid,nodev,noexec", "tmpfs", NAMESPACE_DIRECTORY]);
  await step(context, "namespace-holder", ["mount", "--make-private", NAMESPACE_DIRECTORY]);
  await step(context, "namespace-create", ["touch", NAMESPACE]);
  await step(context, "namespace-create", ["unshare", `--mount=${NAMESPACE}`, "--propagation", "private", "--", "true"]);
  const listed = await step(context, "namespace-mounts", inNamespace(["cat", "/proc/self/mountinfo"]), { capture: true });
  const masks = hostMounts(listed.stdout);
  if (masks.length === 0) {
    throw new InstallError("step 'namespace-mounts': the machine lists no host file mounts, so the isolation of the build user cannot be checked");
  }
  for (const mask of masks) {
    await step(context, "namespace-mask", inNamespace(["mount", "-t", "tmpfs", "-o", MASK_OPTIONS, "tmpfs", mask]));
  }
  const visible = await step(context, "namespace-check", ["find", ...masks, "-mindepth", "1", "-print", "-quit"], { as: "build", capture: true });
  if (visible.stdout.trim() !== "") {
    throw new InstallError(`step 'namespace-check': ${visible.stdout.trim()} is visible to ${BUILD_USER} inside the build namespace`);
  }
}

async function ensureUser(context: Context): Promise<void> {
  const create = ["useradd", "--system", "--user-group", "--no-create-home", "--shell", "/usr/sbin/nologin", BUILD_USER];
  const exists = async (): Promise<boolean> => (await step(context, "user", ["id", "-u", BUILD_USER], { capture: true, accept: [1] })).status === 0;
  for (let attempt = 1; attempt <= USER_ATTEMPTS; attempt += 1) {
    if (await exists()) {
      return;
    }
    await step(context, "user", create, { accept: [USERNAME_IN_USE, PASSWD_LOCKED] });
    if (await exists()) {
      return;
    }
    await Bun.sleep(attempt * 200);
  }
  throw new InstallError(`step 'user': ${BUILD_USER} does not exist after ${USER_ATTEMPTS} attempts to create it`);
}

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
  await step(context, "toolchain", [paths.mise, "install", "--locked", "rust", "mr-boxington"], {
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
  await step(context, "layout", ["install", "-d", "-m", "0755", ...root, paths.root, paths.bin, paths.toolchain, ...paths.toolchainDirectories]);
  await step(context, "layout-private", ["install", "-d", "-m", "0700", ...root, paths.home, paths.tmp]);
  await ensureUser(context);
  await reap(context, "reap-start");
  await ensurePackages(context);
  await ensureMise(context, mise);
  await step(context, "wipe", ["rm", "-rf", paths.work]);
  await step(context, "workspace", ["install", "-d", "-m", "0755", ...root, paths.work, paths.cfg, paths.out]);
  await step(context, "workspace-private", ["install", "-d", "-m", "0700", "-o", BUILD_USER, "-g", BUILD_USER, ...paths.privateDirectories]);
  await enterNamespace(context);
  await step(context, "extract-config", ["tar", "-x", "-f", "-", "-C", paths.cfg, "--no-same-owner"], { stdin: source.toolchain });
  await step(context, "extract-source", ["tar", "-x", "-f", "-", "-C", paths.src, "--no-same-owner"], { as: "build", stdin: source.archive });
  await installToolchain(context);
  await step(context, "link-installs", ["ln", "-s", `${paths.toolchain}/data/installs`, `${paths.work}/data/installs`], { as: "build" });
  await step(context, "link-cargo", ["ln", "-s", `${paths.toolchain}/cargo/bin`, `${paths.work}/cargo/bin`], { as: "build" });
}

const survivors = async (context: Context, label: string): Promise<boolean> =>
  (await step(context, label, ["pgrep", "-u", BUILD_USER], { capture: true, accept: [ABSENT] })).status === 0;

export async function reap(context: Context, label: string): Promise<void> {
  for (let attempt = 1; attempt <= REAP_ATTEMPTS; attempt += 1) {
    await step(context, label, ["pkill", "-KILL", "-u", BUILD_USER], { accept: [ABSENT] });
    if (!(await survivors(context, label))) {
      return;
    }
    await Bun.sleep(REAP_PAUSE_MS);
  }
  throw new InstallError(`step '${label}': processes of ${BUILD_USER} survive ${REAP_ATTEMPTS} kill rounds`);
}

const digestOf = (outcome: Outcome, label: string): string => {
  const sha256 = /^([0-9a-f]{64}) /m.exec(outcome.stdout)?.[1];
  if (sha256 === undefined) {
    throw new InstallError(`step '${label}': sha256sum printed ${JSON.stringify(outcome.stdout.slice(0, 200))}, stderr ${JSON.stringify(outcome.stderr.slice(-200))}`);
  }
  return sha256;
};

export async function collect(context: Context): Promise<string> {
  const { paths } = context;
  const regular = await step(context, "collect-check", ["find", paths.built, "-maxdepth", "0", "-type", "f", "-user", BUILD_USER, "-print"], { capture: true });
  if (regular.stdout.trim() !== paths.built) {
    throw new InstallError(`step 'collect-check': ${paths.built} is not a regular file built by ${BUILD_USER}`);
  }
  if (await survivors(context, "collect-reaped")) {
    throw new InstallError(`step 'collect-reaped': a process of ${BUILD_USER} is running, so the build output could change while it is read`);
  }
  const read = await step(context, "collect-read", ["dd", "iflag=nofollow", `if=${paths.built}`, "bs=1M", "status=none"], { as: "build", bytes: true });
  const sha256 = new Bun.CryptoHasher("sha256").update(read.bytes).digest("hex");
  const reference = digestOf(await step(context, "collect-hash", ["sha256sum", paths.built], { as: "build", capture: true }), "collect-hash");
  if (sha256 !== reference) {
    throw new InstallError(`step 'collect-verify': ${read.bytes.length} bytes were read with sha256 ${sha256}, but ${BUILD_USER} hashes the file as ${reference}`);
  }
  await step(context, "collect-write", ["dd", `of=${paths.output}`, "bs=1M", "status=none"], { stdin: read.bytes });
  return sha256;
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
  await reap(context, "reap");
  return collect(context);
}

export async function cleanup(context: Context): Promise<string[]> {
  const attempts = [
    () => reap(context, "cleanup-reap"),
    () => unmountHolder(context, "cleanup-namespace"),
    () => step(context, "cleanup-namespace", ["rmdir", NAMESPACE_DIRECTORY], { accept: [1] }),
    () => step(context, "cleanup-work", ["rm", "-rf", context.paths.work]),
  ];
  const failures: string[] = [];
  for (const attempt of attempts) {
    await attempt().catch((error: unknown) => {
      failures.push(messageOf(error));
    });
  }
  return failures;
}
