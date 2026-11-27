// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import {
  chmodSync,
  readdirSync,
  copyFileSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import { createHash } from "node:crypto";

const SHARE = "/var/lib/velnor-host";

export type Guest = {
  root: string;
  share: string;
  setting(name: string): string;
};

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

function real(guest: Guest, path: string): string {
  return path === SHARE || path.startsWith(`${SHARE}/`) ? `${guest.share}${path.slice(SHARE.length)}` : `${guest.root}${path}`;
}

function say(text: string): void {
  writeSync(1, text);
}

function complain(text: string): void {
  writeSync(2, text);
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
  const mode = parseInt(args[args.indexOf("-m") + 1], 8);
  if (args.includes("-d")) {
    for (const path of args.slice(args.indexOf("-m") + 2)) {
      mkdirSync(real(guest, path), { recursive: true });
      chmodSync(real(guest, path), mode);
    }
    return 0;
  }
  const [source, target] = args.slice(args.indexOf("-m") + 2);
  try {
    copyFileSync(real(guest, source), real(guest, target));
  } catch {
    return 1;
  }
  chmodSync(real(guest, target), mode);
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

function fallocate(guest: Guest, size: number, path: string): number {
  const cap = Number(guest.setting("FAKE_GUEST_CAP_BYTES") || "0");
  if (guest.setting("FAKE_GUEST_NO_CAP") === "" && size > cap) {
    complain("fallocate: fallocate failed: No space left on device\n");
    return 1;
  }
  writeFileSync(real(guest, path), "");
  return 0;
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
      renameSync(real(guest, rest[2]), real(guest, rest[3]));
      return 0;
    case "rm":
      return remove(guest, rest.slice(1));
    case "find":
      return find(guest, rest.slice(1));
    case "cat":
      try {
        say(readFileSync(real(guest, rest[1]), "utf8"));
        return 0;
      } catch {
        return 1;
      }
    case "fallocate":
      return fallocate(guest, Number(rest[2]), rest[3]);
    case "sh": {
      if (rest.length === 3) {
        say(`${(guest.setting("FAKE_GUEST_USER") || "1000,1000,/srv/sandbox").split(",").join("\n")}\n`);
        return 0;
      }
      const target = real(guest, rest[5]);
      writeFileSync(`${target}.new`, `${rest[4]}\n`);
      renameSync(`${target}.new`, target);
      return 0;
    }
    default:
      return undefined;
  }
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
  const paths = args.includes("--") ? args.slice(args.indexOf("--") + 1) : args.slice(1);
  if (args[0] === "-rf") for (const path of paths) purge(real(guest, path));
  else for (const path of paths) rmSync(real(guest, path), { force: true });
  return 0;
}

function find(guest: Guest, args: string[]): number {
  const directory = real(guest, args[0]);
  if (args.includes("-exec")) {
    try {
      for (const name of readdirSync(directory)) purge(`${directory}/${name}`);
    } catch {
      return 1;
    }
    return 0;
  }
  try {
    lstatSync(directory);
    say(`${args[0]}\n`);
    return 0;
  } catch {
    return 1;
  }
}

function statFilesystem(guest: Guest, paths: string[]): number {
  for (const _ of paths) say(`${guest.setting("FAKE_GUEST_FS") || "btrfs"}\n`);
  return 0;
}

export function listing(args: string[]): string {
  const flag = (name: string): boolean => args.includes(name);
  const value = (name: string): string => args[args.indexOf(name) + 1] ?? "";
  const mount = value("--mount");
  const split = mount.lastIndexOf(":");
  const disk = value("--disk");
  const config: Record<string, unknown> = {
    isolated: flag("--isolated"),
    forward_ssh_agent: !flag("--isolated"),
    isolate_network: flag("--isolate-network"),
  };
  if (mount !== "") config.mounts = [{ source: mount.slice(0, split), destination: mount.slice(split + 1) }];
  if (disk.endsWith("G")) config.disk_limit_bytes = Number(disk.slice(0, -1)) * 2 ** 30;
  return `${JSON.stringify([{ name: "velnor", image: { distro: "ubuntu", arch: "arm64" }, config }])}\n`;
}
