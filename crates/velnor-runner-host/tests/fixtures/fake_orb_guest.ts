// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import {
  chmodSync,
  readdirSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename } from "node:path";
import { createHash } from "node:crypto";
import { Guest, complain, list, real, say } from "./fake_orb_base.ts";
import { shapeScript, systemCommand } from "./fake_orb_system.ts";

export type { Guest } from "./fake_orb_base.ts";

function owners(guest: Guest): Map<string, number> {
  const table = new Map<string, number>();
  for (const pair of guest.setting("FAKE_GUEST_OWNERS").split(",")) {
    const at = pair.indexOf("=");
    if (at > 0) table.set(pair.slice(0, at), Number(pair.slice(at + 1)));
  }
  return table;
}

function modes(guest: Guest): Map<string, number> {
  const table = new Map<string, number>();
  for (const pair of guest.setting("FAKE_GUEST_MODES").split(",")) {
    const at = pair.indexOf("=");
    if (at > 0) table.set(pair.slice(0, at), parseInt(pair.slice(at + 1), 8));
  }
  return table;
}

function kind(mode: number): string {
  if ((mode & 0o170000) === 0o040000) return "directory";
  if ((mode & 0o170000) === 0o120000) return "symbolic link";
  return "regular file";
}

function stat(guest: Guest, paths: string[]): number {
  const owned = owners(guest);
  const moded = modes(guest);
  let code = 0;
  for (const path of paths) {
    try {
      const found = lstatSync(real(guest, path));
      const mode = moded.get(path) ?? found.mode & 0o7777;
      say(`${path} ${owned.get(path) ?? 0} 0 ${mode.toString(8)} ${kind(found.mode)}\n`);
    } catch {
      complain(`stat: cannot statx '${path}': No such file or directory\n`);
      code = 1;
    }
  }
  return code;
}

function install(guest: Guest, args: string[]): number {
  if (!args.includes("-d")) return 64;
  const mode = parseInt(args[args.indexOf("-m") + 1], 8);
  for (const path of args.slice(args.indexOf("-m") + 2)) {
    mkdirSync(real(guest, path), { recursive: true });
    chmodSync(real(guest, path), mode);
  }
  return 0;
}

function sha256(guest: Guest, path: string): number {
  try {
    const sum = createHash("sha256").update(readFileSync(real(guest, path))).digest("hex");
    say(`${sum}  ${path}\n`);
    return 0;
  } catch {
    complain(`sha256sum: ${path}: No such file or directory\n`);
    return 1;
  }
}

function fallocate(guest: Guest, body: string, size: number, path: string): number {
  const cap = Number(guest.setting("FAKE_GUEST_CAP_BYTES") || "0");
  const refused = guest.setting("FAKE_GUEST_NO_CAP") === "" && size > cap;
  writeFileSync(real(guest, path), "");
  if (refused) {
    complain(`fallocate: fallocate failed: ${guest.setting("FAKE_GUEST_CAP_ERROR") || "No space left on device"}\n`);
  }
  if (body.includes("rm -f --")) rmSync(real(guest, path), { force: true });
  return refused ? 1 : 0;
}

function stage(guest: Guest, path: string): number {
  const target = real(guest, path);
  const bytes = readFileSync(0);
  if (guest.setting("FAKE_GUEST_STREAM_FAIL") !== "") {
    writeFileSync(target, bytes.subarray(0, 1));
    return 1;
  }
  writeFileSync(target, guest.setting("FAKE_GUEST_CORRUPT_STREAM") === "" ? bytes : Buffer.concat([bytes, Buffer.from("!")]));
  chmodSync(target, 0o700);
  return 0;
}

function cleanup(guest: Guest, paths: string[]): number {
  if (guest.setting("FAKE_GUEST_WEDGE") !== "") return 70;
  for (const path of paths) purge(real(guest, path));
  return paths.some((path) => existsSync(real(guest, path))) ? 71 : 0;
}

function sweep(guest: Guest, body: string, dirs: string[]): number {
  const wedged = guest.setting("FAKE_GUEST_WEDGE") !== "";
  const readonly = list(guest, "FAKE_GUEST_READONLY");
  const cleared = body.includes("btrfs property set -ts {} ro false");
  const left = new Set<string>();
  for (const dir of dirs) {
    for (const name of readdirSync(real(guest, dir))) {
      if (wedged || (readonly.includes(name) && !cleared)) left.add(name);
      else purge(`${real(guest, dir)}/${name}`);
    }
  }
  for (const name of left) say(`${name}\n`);
  return 0;
}

function script(guest: Guest, rest: string[]): number | undefined {
  const [, , body, , ...args] = rest;
  if (body.includes("fallocate -l")) return fallocate(guest, body, Number(args[0]), args[1]);
  if (body.includes("set -e; umask 077")) return stage(guest, args[0]);
  if (body.includes("rm -rf -- \"$@\" || exit")) return cleanup(guest, args);
  if (body.includes("-mindepth")) return sweep(guest, body, args);
  if (body.includes("printf")) {
    const target = real(guest, args[1]);
    writeFileSync(`${target}.new`, `${args[0]}\n`);
    renameSync(`${target}.new`, target);
    return 0;
  }
  return shapeScript(guest, body, args);
}

export function guestCommand(guest: Guest, rest: string[]): number | undefined {
  switch (rest[0]) {
    case "stat":
      return rest[1] === "-f" ? statFilesystem(guest, rest.slice(4)) : stat(guest, rest.slice(3));
    case "sysctl":
      say(`${(guest.setting("FAKE_GUEST_SYSCTL") || "1,1").split(",").join("\n")}\n`);
      return 0;
    case "install":
      return install(guest, rest.slice(1));
    case "sha256sum":
      return sha256(guest, rest[1]);
    case "mv":
      return move(guest, rest.slice(1));
    case "chmod":
      chmodSync(real(guest, rest[3]), parseInt(rest[1], 8));
      return 0;
    case "rm":
      return remove(guest, rest.slice(1));
    case "find":
      return findSockets(guest, rest.slice(1));
    case "cat":
      try {
        say(readFileSync(real(guest, rest[1]), "utf8"));
        return 0;
      } catch {
        return 1;
      }
    case "sh":
      return script(guest, rest);
    default:
      return systemCommand(guest, rest);
  }
}

function move(guest: Guest, args: string[]): number {
  const split = args.indexOf("--");
  const [source, destination] = args.slice(split + 1).map((path) => real(guest, path));
  const noTarget = args.slice(0, split).some((flag) => /^-[a-zA-Z]*T/.test(flag));
  let target = destination;
  const entry = noTarget ? lstatSync(destination, { throwIfNoEntry: false }) : statSync(destination, { throwIfNoEntry: false });
  if (entry?.isDirectory()) {
    if (noTarget) {
      complain(`mv: cannot overwrite directory '${destination}' with non-directory\n`);
      return 1;
    }
    target = `${destination}/${basename(source)}`;
  }
  renameSync(source, target);
  if (guest.setting("FAKE_GUEST_SWAP_STAGE") !== "") writeFileSync(target, "swapped-in-flight");
  return 0;
}

function purge(path: string): void {
  let found;
  try {
    found = lstatSync(path);
  } catch {
    return;
  }
  if ((found.mode & 0o170000) === 0o040000) {
    chmodSync(path, 0o700);
    for (const name of readdirSync(path)) purge(`${path}/${name}`);
  }
  rmSync(path, { recursive: true, force: true });
}

function remove(guest: Guest, args: string[]): number {
  const paths = args.slice(args.indexOf("--") + 1);
  for (const path of paths) rmSync(real(guest, path), { force: true });
  return 0;
}

function findSockets(guest: Guest, args: string[]): number {
  if (args[1] !== "-type") return 64;
  for (const socket of list(guest, "FAKE_GUEST_SOCKETS")) say(`${socket}\n`);
  return 0;
}

function statFilesystem(guest: Guest, paths: string[]): number {
  for (const _ of paths) say(`${guest.setting("FAKE_GUEST_FS") || "btrfs"}\n`);
  return 0;
}

export function listing(args: string[]): string {
  const flag = (name: string): boolean => args.includes(name);
  const value = (name: string): string => args[args.indexOf(name) + 1] ?? "";
  const disk = value("--disk");
  const config: Record<string, unknown> = {
    isolated: flag("--isolated"),
    forward_ssh_agent: !flag("--isolated"),
    isolate_network: flag("--isolate-network"),
  };
  if (flag("--mount")) config.mounts = [{ source: value("--mount").split(":")[0], destination: value("--mount").split(":")[1] }];
  if (disk.endsWith("G")) config.disk_limit_bytes = Number(disk.slice(0, -1)) * 2 ** 30;
  return `${JSON.stringify([{ name: "velnor", image: { distro: "ubuntu", arch: "arm64" }, config }])}\n`;
}
