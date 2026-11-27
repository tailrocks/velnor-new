// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";
import { parseMachine, parseMise, REPOSITORY } from "./velnor-runner-install-config";
import { build, cleanup, layout, provision, step, type Context } from "./velnor-runner-install-guest";
import { ensureStore, prepareLock, requireLockHeld, swap, underLock } from "./velnor-runner-install-host";
import { InstallError, machineNames, orb, requireMacOS } from "./velnor-runner-install-lib";
import { prepareSource } from "./velnor-runner-install-source";

const UNDER_LOCK = "--run-under-install-lock";

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
  await requireLockHeld();
  const source = await prepareSource(REPOSITORY);
  process.stderr.write(`velnor-runner:install: building ${source.commit}\n`);
  const machine = parseMachine(source.inputs.machine, process.env.VELNOR_HOST_STORE);
  const context: Context = { machine, paths: layout(), installId: crypto.randomUUID() };
  await ensureStore(machine, false);
  await refuseRepoint(context);
  await ensureStore(machine, true);
  await ensureMachine(context);
  try {
    await provision(context, parseMise(source.inputs), source);
    await swap(context, await build(context));
    process.stdout.write((await step(context, "version", [machine.guestRunner, "--version"], { capture: true })).stdout);
  } finally {
    await cleanup(context);
  }
}

async function main(): Promise<void> {
  requireMacOS(process.platform);
  if (process.argv[2] === UNDER_LOCK) {
    return installUnderLock();
  }
  const forwarded = process.env.VELNOR_HOST_STORE === undefined ? {} : { VELNOR_HOST_STORE: process.env.VELNOR_HOST_STORE };
  process.exit(await underLock(await prepareLock(), [process.execPath, import.meta.path, UNDER_LOCK], forwarded));
}

main().catch((error: unknown) => {
  process.stderr.write(`velnor-runner:install failed: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
});
