// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { parseMachine, parseMise, REPOSITORY } from "./velnor-runner-install-config";
import { build, cleanup, layout, provision, step, type Context } from "./velnor-runner-install-guest";
import { ensureStore, prepareLock, requireLockHeld, swap, underLock } from "./velnor-runner-install-host";
import { InstallError, machineNames, messageOf, orb, requireMacOS } from "./velnor-runner-install-lib";
import { prepareSource } from "./velnor-runner-install-source";

const FORWARDED_ENVIRONMENT = ["VELNOR_HOST_STORE", "TMPDIR"] as const;
const UNDER_LOCK = [process.execPath, import.meta.path, "--run-under-install-lock"];

async function refuseRepoint(context: Context): Promise<void> {
  const { machine } = context;
  if (!(await machineNames()).includes(machine.name)) {
    return;
  }
  const current = await step(context, "store-link", ["readlink", machine.guestStore], { capture: true, accept: [1] });
  const target = current.stdout.trim();
  if (current.status === 0 && target !== machine.link) {
    throw new InstallError(`step 'store-link': ${machine.name} links ${machine.guestStore} to ${target}; refusing to repoint it to ${machine.link} (VELNOR_HOST_STORE)`);
  }
}

async function ensureMachine(context: Context): Promise<void> {
  const { machine } = context;
  if (!(await machineNames()).includes(machine.name)) {
    const created = await orb(["create", "-a", machine.arch, machine.distro, machine.name], { output: "capture" });
    if (created.status !== 0 && !created.stderr.includes("already exists")) {
      throw new InstallError(`step 'machine-create': orb create exited with ${created.status}`);
    }
  }
  await step(context, "store-link", ["ln", "-sfn", machine.link, machine.guestStore]);
}

async function installUnderLock(): Promise<void> {
  await requireLockHeld(UNDER_LOCK);
  const source = await prepareSource(REPOSITORY);
  process.stderr.write(`velnor-runner:install: building ${source.commit}\n`);
  const machine = parseMachine(source.inputs.machine, process.env.VELNOR_HOST_STORE);
  const context: Context = { machine, paths: layout(), installId: crypto.randomUUID() };
  await ensureStore(machine, false);
  await refuseRepoint(context);
  await ensureStore(machine, true);
  await ensureMachine(context);
  const failures: string[] = [];
  try {
    await provision(context, parseMise(source.inputs), source);
    await swap(context, await build(context));
    process.stdout.write((await step(context, "version", [machine.guestRunner, "--version"], { capture: true })).stdout);
  } catch (error) {
    failures.push(messageOf(error));
  }
  failures.push(...(await cleanup(context)));
  if (failures.length > 0) {
    throw new InstallError(failures.join("; "));
  }
}

async function main(): Promise<void> {
  requireMacOS(process.platform);
  if (process.argv[2] === UNDER_LOCK[2]) {
    return installUnderLock();
  }
  const forwarded: Record<string, string> = {};
  for (const name of FORWARDED_ENVIRONMENT) {
    const value = process.env[name];
    if (value !== undefined) {
      forwarded[name] = value;
    }
  }
  process.exit(await underLock(await prepareLock(), UNDER_LOCK, forwarded));
}

main().catch((error: unknown) => {
  process.stderr.write(`velnor-runner:install failed: ${messageOf(error)}\n`);
  process.exit(1);
});
