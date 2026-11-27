// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { writeSync } from "node:fs";

export const SHARE = "/var/lib/velnor-host";

export type Guest = {
  root: string;
  share: string;
  setting(name: string): string;
};

export function real(guest: Guest, path: string): string {
  return path === SHARE || path.startsWith(`${SHARE}/`) ? `${guest.share}${path.slice(SHARE.length)}` : `${guest.root}${path}`;
}

export function say(text: string): void {
  writeSync(1, text);
}

export function complain(text: string): void {
  writeSync(2, text);
}

export function list(guest: Guest, name: string): string[] {
  return guest.setting(name).split(";").filter((entry) => entry !== "");
}
