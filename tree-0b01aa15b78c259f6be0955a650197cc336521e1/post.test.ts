import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest'
import {createHash} from 'node:crypto'
import {chmod, mkdtemp, rm, writeFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import path from 'node:path'

const mocks = vi.hoisted(() => ({
  state: {} as Record<string, string>,
  exec: vi.fn(), save: vi.fn(), failed: vi.fn(), warning: vi.fn(), info: vi.fn()
}))
vi.mock('@actions/core', () => ({getState: (name: string) => mocks.state[name] || '', setFailed: mocks.failed, warning: mocks.warning, info: mocks.info}))
vi.mock('@actions/cache', () => ({saveCache: mocks.save, ValidationError: class ValidationError extends Error {}}))
vi.mock('@actions/exec', () => ({exec: mocks.exec}))
vi.mock('@actions/github', () => ({context: {}}))
vi.mock('@actions/tool-cache', () => ({}))

let directory = ''
const digest = 'b'.repeat(64)
let report = {version: 1, useful_delta: true, exported: true, semantic_digest: digest}
beforeEach(async () => {
  vi.resetModules()
  vi.clearAllMocks()
  directory = await mkdtemp(path.join(tmpdir(), 'mbx-post-test-'))
  const bin = path.join(directory, 'mbx')
  await writeFile(bin, 'verified-executable')
  await chmod(bin, 0o755)
  const baseline = path.join(directory, 'baseline')
  await writeFile(baseline, '{}')
  mocks.state = {
    'mbx-post': 'github-save', 'mbx-cache-key': 'primary', 'mbx-cache-hit': 'true',
    'mbx-bin': bin, 'mbx-expected-version': '1.12.0',
    'mbx-executable-sha256': createHash('sha256').update('verified-executable').digest('hex'),
    'mbx-comparison-state': baseline, 'mbx-cache-archive': path.join(directory, 'bundle'),
    'mbx-comparison-sha256': createHash('sha256').update('{}').digest('hex'),
    'mbx-cache-export-group': 'group', 'mbx-cache-paths': JSON.stringify([path.join(directory, 'bundle')]),
    'mbx-cache-bundle-form': 'directory'
  }
  report = {version: 1, useful_delta: true, exported: true, semantic_digest: digest}
  mocks.exec.mockImplementation(async (_bin, args, options) => {
    const output = args[0] === '--version' ? 'mbx 1.12.0' : args.includes('--verify') ? JSON.stringify({version: 1, valid: true}) : JSON.stringify(report)
    options.listeners.stdout(Buffer.from(output))
    return 0
  })
  mocks.save.mockResolvedValue(42)
})
afterEach(async () => { await rm(directory, {recursive: true, force: true}) })
async function invoke(): Promise<void> {
  await import('../src/index.js')
  await vi.waitFor(() => {
    expect(mocks.failed.mock.calls.length + mocks.warning.mock.calls.length + mocks.info.mock.calls.length).toBeGreaterThan(0)
  })
}

describe('strict owner comparison post', () => {
  it('saves useful state on exact hits under the owner semantic digest', async () => {
    await invoke()
    expect(mocks.save).toHaveBeenCalledWith([path.join(directory, 'bundle')], `primary-${digest}`)
    expect(mocks.exec.mock.calls[2]?.[1]).toEqual(['cache', 'export', '--compare', mocks.state['mbx-comparison-state'], '--json', '--group', 'group', '--format', 'directory', path.join(directory, 'bundle')])
    expect(mocks.failed).not.toHaveBeenCalled()
  })
  it('skips unchanged owner state', async () => {
    report.useful_delta = false
    report.exported = false
    await invoke()
    expect(mocks.save).not.toHaveBeenCalled()
    expect(mocks.info).toHaveBeenCalledWith(expect.stringContaining('No useful'))
  })
  it('fails malformed owner reports', async () => {
    report.semantic_digest = 'bad'
    await invoke()
    expect(mocks.failed).toHaveBeenCalledWith(expect.objectContaining({message: expect.stringContaining('Invalid mbx comparison')}))
    expect(mocks.save).not.toHaveBeenCalled()
  })
  it('fails missing comparison baselines', async () => {
    await rm(mocks.state['mbx-comparison-state'] as string)
    await invoke()
    expect(mocks.failed).toHaveBeenCalled()
    expect(mocks.save).not.toHaveBeenCalled()
  })
  it('fails modified executable bytes even with an unchanged version banner', async () => {
    await writeFile(mocks.state['mbx-bin'] as string, 'tampered-executable')
    await invoke()
    expect(mocks.failed).toHaveBeenCalledWith(expect.objectContaining({message: expect.stringContaining('executable changed')}))
    expect(mocks.exec).not.toHaveBeenCalled()
  })
  it('warns on export availability failure without falsifying the build outcome', async () => {
    mocks.exec.mockImplementation(async (_bin, args, options) => {
      if (args[0] === '--version') { options.listeners.stdout(Buffer.from('mbx 1.12.0')); return 0 }
      if (args.includes('--verify')) { options.listeners.stdout(Buffer.from(JSON.stringify({version: 1, valid: true}))); return 0 }
      return 1
    })
    await invoke()
    expect(mocks.warning).toHaveBeenCalled()
    expect(mocks.failed).not.toHaveBeenCalled()
    expect(mocks.save).not.toHaveBeenCalled()
  })
  it('warns on upload availability failure without falsifying the build outcome', async () => {
    mocks.save.mockRejectedValue(new Error('service unavailable'))
    await invoke()
    expect(mocks.warning).toHaveBeenCalled()
    expect(mocks.failed).not.toHaveBeenCalled()
  })
  it('fails owner baseline verification before optional export', async () => {
    mocks.exec.mockImplementation(async (_bin, args, options) => {
      options.listeners.stdout(Buffer.from(args[0] === '--version' ? 'mbx 1.12.0' : JSON.stringify({version: 1, valid: false})))
      return 0
    })
    await invoke()
    expect(mocks.failed).toHaveBeenCalledWith(expect.objectContaining({message: expect.stringContaining('baseline verification')}))
    expect(mocks.save).not.toHaveBeenCalled()
    expect(mocks.exec.mock.calls.some(call => call[1][1] === 'export')).toBe(false)
  })
  it('rejects a baseline changed by repository tasks', async () => {
    await writeFile(mocks.state['mbx-comparison-state'] as string, '{"version":1}')
    await invoke()
    expect(mocks.failed).toHaveBeenCalledWith(expect.objectContaining({message: expect.stringContaining('baseline changed')}))
    expect(mocks.save).not.toHaveBeenCalled()
  })
})
