// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
"use strict";

export class InstallError extends Error {}

export type Outcome = { readonly status: number; readonly stdout: string; readonly stderr: string };
export type RunOptions = {
  readonly env: Readonly<Record<string, string>>;
  readonly stdin?: Uint8Array;
  readonly cwd?: string;
  readonly output?: "stderr" | "stdout" | "capture";
};

export const EX_TEMPFAIL = 75;
const PRESERVED_ENVIRONMENT = ["PATH", "HOME"] as const;

export function hostEnvironment(extra: Readonly<Record<string, string>> = {}): Record<string, string> {
  const environment: Record<string, string> = {};
  for (const name of PRESERVED_ENVIRONMENT) {
    const value = process.env[name];
    if (value !== undefined) {
      environment[name] = value;
    }
  }
  return { ...environment, ...extra };
}

async function pump(stream: ReadableStream<Uint8Array>, forward: NodeJS.WriteStream | undefined): Promise<string> {
  const decoder = new TextDecoder();
  let text = "";
  for await (const chunk of stream) {
    const piece = decoder.decode(chunk, { stream: true });
    text += piece;
    forward?.write(piece);
  }
  return text + decoder.decode();
}

export async function run(argv: readonly string[], options: RunOptions): Promise<Outcome> {
  const child = Bun.spawn([...argv], {
    cwd: options.cwd ?? "/",
    env: { ...options.env },
    stdin: options.stdin ?? "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, stderr, status] = await Promise.all([
    pump(child.stdout, { stderr: process.stderr, stdout: process.stdout, capture: undefined }[options.output ?? "stderr"]),
    pump(child.stderr, process.stderr),
    child.exited,
  ]);
  return { status, stdout, stderr };
}

export const orb = (args: readonly string[], options: Omit<RunOptions, "env"> = {}): Promise<Outcome> =>
  run(["orb", ...args], { ...options, env: hostEnvironment() });

export function parseMachineNames(text: string): string[] {
  let listed: unknown;
  try {
    listed = JSON.parse(text);
  } catch {
    throw new InstallError("step 'machine-list': orb list did not print JSON");
  }
  if (!Array.isArray(listed)) {
    throw new InstallError("step 'machine-list': orb list did not print a list");
  }
  const names: string[] = [];
  for (const entry of listed as unknown[]) {
    const name = typeof entry === "object" && entry !== null ? (entry as { name?: unknown }).name : undefined;
    if (typeof name === "string") {
      names.push(name);
    }
  }
  return names;
}

export async function machineNames(): Promise<string[]> {
  const listed = await orb(["list", "-f", "json"], { output: "capture" });
  if (listed.status !== 0) {
    throw new InstallError(`step 'machine-list': orb list exited with ${listed.status}`);
  }
  return parseMachineNames(listed.stdout);
}
