import {createHash} from 'node:crypto'
import {afterEach, describe, expect, it, vi} from 'vitest'
import {chmod, mkdtemp, rm, writeFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import path from 'node:path'
import {compilerIdentity, preinstalledInputs, verifiedPreinstalledMbx} from '../src/preinstalled.js'

const digest = createHash('sha256').update('fixture').digest('hex')
const directories: string[] = []
async function executable(): Promise<string> {
  const directory = await mkdtemp(path.join(tmpdir(), 'mbx-action-test-'))
  directories.push(directory)
  const bin = path.join(directory, process.platform === 'win32' ? 'mbx.exe' : 'mbx')
  await writeFile(bin, 'fixture')
  await chmod(bin, 0o755)
  return bin
}
afterEach(async () => {
  await Promise.all(directories.splice(0).map(directory => rm(directory, {recursive: true, force: true})))
})

describe('strict preinstalled executable', () => {
  it('requires both explicit inputs and excludes release installation', () => {
    expect(preinstalledInputs('', '', '')).toBe(false)
    expect(() => preinstalledInputs('/mbx', '', '')).toThrow(/together/)
    expect(() => preinstalledInputs('', '1.12.0', '')).toThrow(/together/)
    expect(() => preinstalledInputs('/mbx', '1.12.0', '')).toThrow(/together/)
    expect(() => preinstalledInputs('', '', '', digest)).toThrow(/together/)
    expect(() => preinstalledInputs('/mbx', '1.12.0', '', 'bad')).toThrow(/64 lowercase/)
    expect(() => preinstalledInputs('/mbx', '1.12.0', 'latest', digest)).toThrow(/cannot be combined/)
    expect(() => preinstalledInputs('mbx', '1.12.0', '', digest)).toThrow(/absolute/)
    for (const version of ['latest', 'v1.12.0', '1.12', '01.12.0']) {
      expect(() => preinstalledInputs('/mbx', version, '', digest)).toThrow(/exact/)
    }
  })

  it('uses only the selected executable and accepts the exact banner', async () => {
    const bin = await executable()
    const capture = vi.fn().mockResolvedValue('mbx 1.12.0')
    expect(await verifiedPreinstalledMbx(bin, '1.12.0', digest, capture)).toEqual({bin, version: '1.12.0'})
    expect(capture.mock.calls).toEqual([[bin, ['--version']]])
  })

  it.each(['mbx 1.12.1', 'other 1.12.0', 'mbx 1.12.0\nmbx 1.12.0', 'mbx 1.12.0 extra'])('rejects %s', async banner => {
    const bin = await executable()
    await expect(verifiedPreinstalledMbx(bin, '1.12.0', digest, vi.fn().mockResolvedValue(banner))).rejects.toThrow(/expected/)
  })

  it('fails before execution when the path is missing or a directory', async () => {
    const bin = await executable()
    const capture = vi.fn()
    await expect(verifiedPreinstalledMbx(`${bin}-absent`, '1.12.0', digest, capture)).rejects.toThrow()
    await expect(verifiedPreinstalledMbx(path.dirname(bin), '1.12.0', digest, capture)).rejects.toThrow(/regular file/)
    expect(capture).not.toHaveBeenCalled()
  })

  it.skipIf(process.platform === 'win32')('rejects non-executable files before execution', async () => {
    const bin = await executable()
    await chmod(bin, 0o644)
    const capture = vi.fn()
    await expect(verifiedPreinstalledMbx(bin, '1.12.0', digest, capture)).rejects.toThrow()
    expect(capture).not.toHaveBeenCalled()
  })

  it('propagates execution failure without fallback', async () => {
    const bin = await executable()
    await expect(verifiedPreinstalledMbx(bin, '1.12.0', digest, vi.fn().mockRejectedValue(new Error('exit 1')))).rejects.toThrow(/exit 1/)
  })
  it('rejects a wrong caller hash before invoking the matching-banner executable', async () => {
    const bin = await executable()
    const capture = vi.fn().mockResolvedValue('mbx 1.12.0')
    await expect(verifiedPreinstalledMbx(bin, '1.12.0', 'a'.repeat(64), capture)).rejects.toThrow(/SHA-256/)
    expect(capture).not.toHaveBeenCalled()
  })
  it('accepts an exact source build identity banner with matching bytes', async () => {
    const bin = await executable()
    const version = '1.13.0-velnor.abcdef+source.123'
    expect(await verifiedPreinstalledMbx(bin, version, digest, vi.fn().mockResolvedValue(`mbx ${version}`))).toEqual({bin, version})
  })
})

describe('named compiler identity', () => {
  it('probes the named compiler and preserves its complete identity', async () => {
    const identity = 'rustc 1.98.0\nhost: x86_64-unknown-linux-gnu'
    const capture = vi.fn().mockResolvedValue(identity)
    expect(await compilerIdentity('1.98.0', ['+1.98.0', '-vV'], capture)).toBe(identity)
    expect(capture.mock.calls).toEqual([['rustc', ['+1.98.0', '-vV']]])
  })
  it.each(['', ' '])('retains optional default probe fallback for %j', async name => {
    expect(await compilerIdentity(name, ['-vV'], vi.fn().mockRejectedValue(new Error('absent')))).toBeNull()
  })
  it.each([new Error('missing toolchain'), new Error('exit 1')])('fails closed on a named toolchain error', async error => {
    await expect(compilerIdentity('1.98.0', ['+1.98.0', '-vV'], vi.fn().mockRejectedValue(error))).rejects.toThrow(/named Rust toolchain/)
  })
  it('rejects an empty named compiler identity', async () => {
    await expect(compilerIdentity('1.98.0', ['+1.98.0', '-vV'], vi.fn().mockResolvedValue(''))).rejects.toThrow(/named Rust toolchain/)
  })
})
