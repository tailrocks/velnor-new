import {constants} from 'node:fs'
import {access, stat} from 'node:fs/promises'
import path from 'node:path'

export interface MbxInstallation {
  bin: string
  version: string
}

type Capture = (command: string, args: string[]) => Promise<string>

/** An explicit executable is authoritative: failure never falls back to installation. */
export function preinstalledInputs(bin: string, expected: string, release: string): boolean {
  if (!bin && !expected) return false
  if (!bin || !expected) throw new Error('mbx-path and expected-version must be supplied together')
  if (release) throw new Error('version cannot be combined with mbx-path and expected-version')
  if (!path.isAbsolute(bin)) throw new Error('mbx-path must be an absolute executable path')
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(expected)) {
    throw new Error('expected-version must be an exact mbx version, without a v prefix')
  }
  return true
}

export async function verifiedPreinstalledMbx(
  bin: string,
  expected: string,
  capture: Capture
): Promise<MbxInstallation> {
  preinstalledInputs(bin, expected, '')
  if (!(await stat(bin)).isFile()) throw new Error(`mbx-path is not a regular file: ${bin}`)
  await access(bin, constants.X_OK)
  const banner = await capture(bin, ['--version'])
  if (banner !== `mbx ${expected}`) {
    throw new Error(`mbx-path expected mbx ${expected}, received ${JSON.stringify(banner)}`)
  }
  return {bin, version: expected}
}

export async function compilerIdentity(
  toolchain: string,
  args: string[],
  capture: Capture
): Promise<string | null> {
  try {
    const identity = await capture('rustc', args)
    if (!identity.trim()) throw new Error('rustc returned an empty identity')
    return identity
  } catch (error) {
    if (toolchain.trim()) {
      throw new Error(`Could not probe named Rust toolchain ${JSON.stringify(toolchain)}: ${String(error)}`)
    }
    return null
  }
}
