// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { appendFileSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { Guest, complain, list, real, say } from "./fake_orb_base.ts";

const ACCOUNT = "velnor-job";
const HOME = "/home" + "/velnor-job";
const CREATE_USER = [
  "--system",
  "--user-group",
  "--no-create-home",
  "--home-dir",
  HOME,
  "--shell",
  "/usr/sbin/nologin",
  ACCOUNT,
];

function present(path: string): boolean {
  try {
    lstatSync(path);
    return true;
  } catch {
    return false;
  }
}

function accounts(guest: Guest): string[] {
  const file = real(guest, "/etc/passwd");
  return present(file) ? readFileSync(file, "utf8").split("\n").filter((line) => line !== "") : [];
}

function account(guest: Guest): string | undefined {
  return accounts(guest).find((line) => line.startsWith(`${ACCOUNT}:`));
}

function getent(guest: Guest, rest: string[]): number {
  const found = account(guest);
  if (rest[1] !== "passwd" || found === undefined) return 2;
  say(`${found}\n`);
  return 0;
}

function useradd(guest: Guest, rest: string[]): number {
  if (rest.slice(1).join(" ") !== CREATE_USER.join(" ")) return 2;
  if (account(guest) !== undefined) return 9;
  mkdirSync(real(guest, "/etc"), { recursive: true });
  appendFileSync(real(guest, "/etc/passwd"), `${ACCOUNT}:x:500:500::${HOME}:/usr/sbin/nologin\n`);
  return 0;
}

function id(guest: Guest, rest: string[]): number {
  if (account(guest) === undefined) return 1;
  const gid = account(guest)?.split(":")[3] ?? "";
  say(`${rest[1] === "-G" ? guest.setting("FAKE_GUEST_GROUPS") || gid : guest.setting("FAKE_GUEST_GROUP_NAMES") || ACCOUNT}\n`);
  return 0;
}

function files(path: string): string[] {
  try {
    if (lstatSync(path).isDirectory()) return readdirSync(path).flatMap((name) => files(`${path}/${name}`));
    return [path];
  } catch {
    return [];
  }
}

function grep(guest: Guest, rest: string[]): number {
  const [word, ...paths] = rest.slice(rest.indexOf("--") + 1);
  const matched = paths
    .flatMap((path) => files(real(guest, path)))
    .some((file) => new RegExp(`\\b${word}\\b`).test(readFileSync(file, "utf8")));
  return matched ? 0 : 1;
}

function apt(guest: Guest, rest: string[]): number {
  if (rest.includes("apt-get")) {
    if (guest.setting("FAKE_GUEST_NO_NFT_PACKAGE") !== "") return 100;
    writeFileSync(real(guest, "/usr/sbin/nft"), "");
    return 0;
  }
  return 64;
}

function findmnt(guest: Guest): number {
  say("/ btrfs\n/var/lib/velnor-host virtiofs\n/proc proc\n");
  for (const mount of list(guest, "FAKE_GUEST_MOUNTS")) say(`${mount}\n`);
  return 0;
}

export function systemCommand(guest: Guest, rest: string[]): number | undefined {
  switch (rest[0]) {
    case "getent":
      return getent(guest, rest);
    case "useradd":
      return useradd(guest, rest);
    case "id":
      return id(guest, rest);
    case "grep":
      return grep(guest, rest);
    case "env":
      return apt(guest, rest);
    case "findmnt":
      return findmnt(guest);
    case "/opt/orbstack-guest/bin/mac":
      complain("dial: no such file or directory\n");
      return guest.setting("FAKE_GUEST_MAC_WORKS") === "" ? 1 : 0;
    default:
      return undefined;
  }
}

export function shapeScript(guest: Guest, script: string, args: string[]): number | undefined {
  if (script.includes("command -v")) {
    return args.every((tool) => present(real(guest, `/usr/sbin/${tool}`))) ? 0 : 1;
  }
  if (script.includes("test ! -e")) return present(real(guest, args[0])) ? 1 : 0;
  if (script.includes("max_referenced")) {
    say(`${guest.setting("FAKE_GUEST_QGROUP") || guest.setting("FAKE_GUEST_CAP_BYTES")}\n`);
    return 0;
  }
  return undefined;
}
