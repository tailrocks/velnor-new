// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { appendFileSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { Guest, complain, list, real, say } from "./fake_orb_base.ts";

const ACCOUNT = "velnor-job";
const HOST = "velnor-fake";
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

function account(guest: Guest): string | undefined {
  return database(guest, "/etc/passwd").find((line) => line.startsWith(`${ACCOUNT}:`));
}

function database(guest: Guest, path: string): string[] {
  const file = real(guest, path);
  return present(file) ? readFileSync(file, "utf8").split("\n").filter((line) => line !== "") : [];
}

function getent(guest: Guest, rest: string[]): number {
  if (rest[1] !== "passwd" && rest[1] !== "group") return 2;
  const lines = database(guest, rest[1] === "group" ? "/etc/group" : "/etc/passwd");
  if (rest[2] === undefined) {
    for (const line of lines) say(`${line}\n`);
    return 0;
  }
  const numeric = /^[0-9]+$/.test(rest[2]);
  const found = lines.find((line) => {
    const fields = line.split(":");
    return numeric ? fields[2] === rest[2] : fields[0] === rest[2];
  });
  if (found === undefined) return 2;
  say(`${found}\n`);
  return 0;
}

function useradd(guest: Guest, rest: string[]): number {
  if (rest.slice(1).join(" ") !== CREATE_USER.join(" ")) return 2;
  if (account(guest) !== undefined) return 9;
  mkdirSync(real(guest, "/etc"), { recursive: true });
  appendFileSync(real(guest, "/etc/passwd"), `${ACCOUNT}:x:500:500::${HOME}:/usr/sbin/nologin\n`);
  appendFileSync(real(guest, "/etc/group"), `${ACCOUNT}:x:500:\n`);
  return 0;
}

type Membership = { name: string; gid: string };

function memberships(guest: Guest, name: string, primary: string): Membership[] {
  return database(guest, "/etc/group")
    .map((line) => line.split(":"))
    .filter((fields) => fields[2] === primary || (fields[3] ?? "").split(",").includes(name))
    .map((fields) => ({ name: fields[0], gid: fields[2] }));
}

function applies(spec: string, name: string, uid: string, groups: Membership[]): boolean {
  if (spec === "ALL" || spec === name || spec === `#${uid}`) return true;
  if (spec.startsWith("%#")) return groups.some((group) => group.gid === spec.slice(2));
  return spec.startsWith("%") && groups.some((group) => group.name === spec.slice(1));
}

function privileges(guest: Guest): string[] {
  return ["/etc/sudoers", "/etc/sudoers.d"]
    .flatMap((path) => files(real(guest, path)))
    .flatMap((file) => readFileSync(file, "utf8").split("\n"))
    .map((line) => line.trim())
    .filter((line) => line.includes("=") && (!line.startsWith("#") || /^#[0-9]/.test(line)));
}

function sudo(guest: Guest, rest: string[]): number {
  if (rest.slice(1).join(" ") !== "-n -l -U velnor-job") return 64;
  const found = account(guest)?.split(":");
  if (found === undefined) return 1;
  const [name, , uid, gid] = found;
  const groups = memberships(guest, name, gid);
  const granted = privileges(guest).some((line) =>
    line
      .split(/\s+/)[0]
      .split(",")
      .some((spec) => applies(spec, name, uid, groups)),
  );
  say(
    granted
      ? `User ${name} may run the following commands on ${HOST}:\n    (ALL) NOPASSWD: ALL\n`
      : `User ${name} is not allowed to run sudo on ${HOST}.\n`,
  );
  return guest.setting("FAKE_GUEST_SUDO_EXIT") === "" ? 0 : 1;
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
    if (guest.setting("FAKE_GUEST_NO_BTRFS_PACKAGE") === "") writeFileSync(real(guest, "/usr/sbin/btrfs"), "");
    return 0;
  }
  return 64;
}

function findmnt(guest: Guest): number {
  say("/ btrfs\n/proc proc\n");
  for (const mount of list(guest, "FAKE_GUEST_MOUNTS")) say(`${mount}\n`);
  return 0;
}

export function systemCommand(guest: Guest, rest: string[]): number | undefined {
  switch (rest[0]) {
    case "getent":
      return getent(guest, rest);
    case "sudo":
      return sudo(guest, rest);
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
