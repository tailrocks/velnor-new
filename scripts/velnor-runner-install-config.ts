// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { isAbsolute, join, resolve } from "node:path";
import { InstallError } from "./velnor-runner-install-lib";

export const REPOSITORY = resolve(import.meta.dir, "..");
const MACHINE_SOURCE = join(REPOSITORY, "crates/velnor-runner-host/src/rust_runner/mod.rs");
const MISE_DIGESTS = join(REPOSITORY, "crates/velnor-render/src/mise_digest.rs");
const MISE_PLATFORM = "Linux-ARM64";

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
    throw new InstallError(`${MACHINE_SOURCE} no longer defines const ${name}`);
  }
  return match[1];
}

const dirnameOf = (path: string): string => path.slice(0, path.lastIndexOf("/"));

export async function loadMachine(): Promise<Machine> {
  const source = await readFile(MACHINE_SOURCE, "utf8");
  const override = process.env.VELNOR_HOST_STORE;
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

export async function miseBinary(): Promise<MiseBinary> {
  const pinned = /^min_version = "([^"]+)"$/m.exec(await readFile(join(REPOSITORY, "mise.toml"), "utf8"));
  const version = pinned?.[1];
  if (version === undefined || !/^\d{4}\.\d{1,2}\.\d{1,3}$/.test(version)) {
    throw new InstallError("mise.toml does not pin a mise release in min_version");
  }
  const table = await readFile(MISE_DIGESTS, "utf8");
  const row = new RegExp(`version: "${version.replaceAll(".", "\\.")}",\\s*sha256: \\[([^\\]]*)\\]`).exec(table);
  const sha256 = new RegExp(`"${MISE_PLATFORM}",\\s*"([0-9a-f]{64})"`).exec(row?.[1] ?? "")?.[1];
  if (sha256 === undefined) {
    throw new InstallError(`${MISE_DIGESTS} has no ${MISE_PLATFORM} digest for mise ${version}`);
  }
  return { version, sha256 };
}

export const checkoutKey = (): string => createHash("sha256").update(REPOSITORY).digest("hex").slice(0, 16);
