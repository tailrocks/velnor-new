import {createHash} from 'node:crypto'
import {afterEach, beforeEach, describe, expect, it, vi} from 'vitest'
import {chmod, mkdtemp, rm, writeFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import path from 'node:path'

const mocks = vi.hoisted(() => ({inputs: {} as Record<string, string>, state: {} as Record<string, string>, exec: vi.fn(), restore: vi.fn(), failed: vi.fn(), warning: vi.fn(), info: vi.fn(), context: {runId: 1, runAttempt: 1, eventName: 'push', ref: 'refs/heads/main', sha: 'abc', payload: {repository: {default_branch: 'main'}}}}))
vi.mock('@actions/core', () => ({
  getState: () => '', getInput: (name: string) => mocks.inputs[name] || '',
  getMultilineInput: () => ['linux-x64-mbx-qualified-'], getBooleanInput: (name: string) => mocks.inputs[name] === 'true',
  saveState: (name: string, value: string) => { mocks.state[name] = value },
  setFailed: mocks.failed, warning: mocks.warning, info: mocks.info,
  addPath: vi.fn(), exportVariable: vi.fn(), setOutput: vi.fn(), setSecret: vi.fn(), debug: vi.fn(),
  summary: {addDetails: () => ({write: async () => {}})}
}))
vi.mock('@actions/cache', () => ({restoreCache: mocks.restore, ValidationError: class ValidationError extends Error {}}))
vi.mock('@actions/exec', () => ({exec: mocks.exec}))
vi.mock('@actions/github', () => ({context: mocks.context}))
vi.mock('@actions/tool-cache', () => ({}))

let directory = ''
let importFails = false
beforeEach(async () => {
  vi.resetModules()
  vi.clearAllMocks()
  vi.stubEnv('ACTIONS_RUNTIME_TOKEN', 'fixture-token')
  vi.stubEnv('ACTIONS_RESULTS_URL', 'https://fixture.invalid/')
  directory = await mkdtemp(path.join(tmpdir(), 'mbx-main-test-'))
  vi.stubEnv('RUNNER_TEMP', directory)
  const bin = path.join(directory, 'mbx')
  await writeFile(bin, 'fixture-executable')
  await chmod(bin, 0o755)
  mocks.inputs = {'mbx-path': bin, 'expected-version': '1.12.0', 'expected-binary-sha256': createHash('sha256').update('fixture-executable').digest('hex'), backend: 'github', 'github-cache-mode': 'objects', 'comparison-state': path.join(directory, 'baseline'), toolchain: '1.98.0', 'cache-links': 'false', 'cache-generation': 'qualified'}
  mocks.state = {}
  mocks.context.eventName = 'push'
  importFails = false
  mocks.restore.mockResolvedValue('restored-qualified-key')
  mocks.exec.mockImplementation(async (_bin, args, options) => {
    let output = ''
    if (args[0] === '--version') output = 'mbx 1.12.0'
    else if (args[0] === '+1.98.0') output = 'rustc 1.98.0\nhost: fixture'
    else if (args[1] === 'dir') output = path.join(directory, 'cache')
    else if (args[1] === 'comparison-state') {
      await writeFile(args[2], JSON.stringify({version: 1, cold: true}))
      output = JSON.stringify({version: 1, empty: true})
    } else if (args[1] === 'import') {
      if (importFails) return 1
      await writeFile(args[3], JSON.stringify({version: 1, restored: true}))
    }
    options?.listeners?.stdout?.(Buffer.from(output))
    return 0
  })
})
afterEach(async () => { vi.unstubAllEnvs(); await rm(directory, {recursive: true, force: true}) })
async function invoke() {
  await import('../src/index.js')
  await vi.waitFor(() => expect(mocks.failed.mock.calls.length > 0 || mocks.state['mbx-post'] === 'github-save').toBe(true))
}
describe('strict object transport main', () => {
  it('records imported baseline digest and never installs an executable', async () => {
    await invoke()
    expect(mocks.failed).not.toHaveBeenCalled()
    expect(mocks.state['mbx-comparison-sha256']).toMatch(/^[0-9a-f]{64}$/)
    expect(mocks.state['mbx-bin']).toBe(mocks.inputs['mbx-path'])
    expect(mocks.exec.mock.calls.some(call => call[1][1] === 'import')).toBe(true)
  })
  it('falls back from failed import to an explicit cold owner baseline', async () => {
    importFails = true
    await invoke()
    expect(mocks.failed).not.toHaveBeenCalled()
    expect(mocks.warning).toHaveBeenCalledWith(expect.stringContaining('cold comparison baseline'))
    expect(mocks.exec.mock.calls.filter(call => call[1][1] === 'comparison-state')).toHaveLength(2)
    expect(mocks.state['mbx-cache-hit']).toBe('false')
  })
  it('continues cold on a classified cache transport outage', async () => {
    mocks.restore.mockRejectedValue(Object.assign(new Error('offline'), {code: 'ECONNRESET'}))
    await invoke()
    expect(mocks.failed).not.toHaveBeenCalled()
    expect(mocks.warning).toHaveBeenCalledWith(expect.stringContaining('restore unavailable'))
  })
  it('fails unexpected restore errors instead of suppressing bugs', async () => {
    mocks.restore.mockRejectedValue(new Error('unexpected bug'))
    await invoke()
    expect(mocks.failed).toHaveBeenCalledWith(expect.objectContaining({message: 'unexpected bug'}))
  })
  it('fails unsupported owner baseline capability before cache restore', async () => {
    const implementation = mocks.exec.getMockImplementation()
    mocks.exec.mockImplementation(async (bin, args, options) => {
      if (args[1] === 'comparison-state') { options.listeners.stdout(Buffer.from('{}')); return 0 }
      return implementation?.(bin, args, options)
    })
    await invoke()
    expect(mocks.failed).toHaveBeenCalled()
    expect(mocks.restore).not.toHaveBeenCalled()
  })
  it('keeps comparison dispatch keys semantic without a run identifier', async () => {
    mocks.context.eventName = 'workflow_dispatch'
    mocks.inputs['save-on-workflow-dispatch'] = 'true'
    await invoke()
    expect(mocks.failed).not.toHaveBeenCalled()
    expect(mocks.state['mbx-cache-key']).toMatch(/-abc$/)
    expect(mocks.state['mbx-cache-key']).not.toContain('-run-')
  })
  it('rejects a wrong published binary hash before any executable or cache operation', async () => {
    mocks.inputs['expected-binary-sha256'] = 'a'.repeat(64)
    await invoke()
    expect(mocks.failed).toHaveBeenCalledWith(expect.objectContaining({message: expect.stringContaining('SHA-256')}))
    expect(mocks.exec).not.toHaveBeenCalled()
    expect(mocks.restore).not.toHaveBeenCalled()
  })
})
