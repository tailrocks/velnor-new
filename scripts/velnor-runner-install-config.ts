// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { isAbsolute, join, resolve } from "node:path";
import { InstallError } from "./velnor-runner-install-lib";

export const REPOSITORY = resolve(import.meta.dir, "..");
export const INPUT_FILES = {
  machine: "crates/velnor-runner-host/src/rust_runner/mod.rs",
  digests: "crates/velnor-render/src/mise_digest.rs",
  mise: "mise.toml",
} as const;
const MISE_PLATFORM = "Linux-ARM64";

export type Inputs = { readonly [Name in keyof typeof INPUT_FILES]: string };

export type Machine = {
  readonly name: string;
  readonly arch: string;
  readonly distro: string;
  readonly guestStore: string;
  readonly guestRunner: string;
  readonly guestBin: string;
  readonly macMount: string;
  readonly hostStore: string;
  readonly hostBin: string;
  readonly link: string;
};

export type MiseBinary = { readonly version: string; readonly sha256: string };

function rustConst(source: string, name: string): string {
  const match = new RegExp(`^const ${name}: &str = "([^"]*)";$`, "m").exec(source);
  if (match?.[1] === undefined) {
    throw new InstallError(`${INPUT_FILES.machine} no longer defines const ${name}`);
  }
  return match[1];
}

const dirnameOf = (path: string): string => path.slice(0, path.lastIndexOf("/"));

export function parseMachine(source: string, override: string | undefined): Machine {
  if (override !== undefined && !isAbsolute(override)) {
    throw new InstallError(`VELNOR_HOST_STORE must be an absolute path, got '${override}'`);
  }
  const guestStore = rustConst(source, "GUEST_STORE");
  const guestRunner = rustConst(source, "GUEST_RUNNER");
  const binDirectory = rustConst(source, "BIN_DIRECTORY");
  if (dirnameOf(guestRunner) !== `${guestStore}/${binDirectory}`) {
    throw new InstallError(`GUEST_RUNNER ${guestRunner} is not under ${guestStore}/${binDirectory}`);
  }
  const hostStore = override ?? `/${rustConst(source, "DEFAULT_HOST_STORE_UNDER_ROOT")}`;
  const macMount = rustConst(source, "MAC_MOUNT");
  return {
    name: rustConst(source, "MACHINE_NAME"),
    arch: rustConst(source, "MACHINE_ARCH"),
    distro: rustConst(source, "MACHINE_DISTRO"),
    guestStore,
    guestRunner,
    guestBin: dirnameOf(guestRunner),
    macMount,
    hostStore,
    hostBin: join(hostStore, binDirectory),
    link: `${macMount}${hostStore}`,
  };
}

export function parseMise(inputs: Inputs): MiseBinary {
  const pinned = /^min_version = "([^"]+)"$/m.exec(inputs.mise);
  const version = pinned?.[1];
  if (version === undefined || !/^\d{4}\.\d{1,2}\.\d{1,3}$/.test(version)) {
    throw new InstallError(`${INPUT_FILES.mise} does not pin a mise release in min_version`);
  }
  const row = new RegExp(`version: "${version.replaceAll(".", "\\.")}",\\s*sha256: \\[([^\\]]*)\\]`).exec(inputs.digests);
  const sha256 = new RegExp(`"${MISE_PLATFORM}",\\s*"([0-9a-f]{64})"`).exec(row?.[1] ?? "")?.[1];
  if (sha256 === undefined) {
    throw new InstallError(`${INPUT_FILES.digests} has no ${MISE_PLATFORM} digest for mise ${version}`);
  }
  return { version, sha256 };
}
