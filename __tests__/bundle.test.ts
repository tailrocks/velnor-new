import {createHash} from 'node:crypto'
import {afterEach, describe, expect, it} from 'vitest'
import {execFile} from 'node:child_process'
import {promisify} from 'node:util'
import {chmod, mkdtemp, readFile, rm, writeFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import path from 'node:path'

const run = promisify(execFile)
const directories: string[] = []
afterEach(async () => {
  await Promise.all(directories.splice(0).map(directory => rm(directory, {recursive: true, force: true})))
})

describe.skipIf(process.platform === 'win32')('distributed strict executable mode', () => {
  async function invoke(banner: string, missing = false, wrongHash = false) {
    const directory = await mkdtemp(path.join(tmpdir(), 'mbx-action-bundle-'))
    directories.push(directory)
    const bin = path.join(directory, 'mbx')
    await writeFile(bin, `#!/bin/sh\nprintf invoked > '${directory}/executed'\nif [ "$1" = "--version" ]; then printf '%s\\n' '${banner}'; else printf '%s\\n' '${directory}/cache'; fi\n`)
    await chmod(bin, 0o755)
    const preload = path.join(directory, 'no-download.cjs')
    await writeFile(preload, "global.fetch = () => { throw new Error('NETWORK_FORBIDDEN') }\n")
    await Promise.all(['path', 'state', 'env', 'output', 'summary'].map(name => writeFile(path.join(directory, name), '')))
    return run(process.execPath, ['--require', preload, path.resolve('dist/index.js')], {
      env: {
        ...process.env,
        'INPUT_BACKEND': 'local',
        'INPUT_GITHUB-CACHE-MODE': 'objects',
        'INPUT_MBX-PATH': missing ? `${bin}-missing` : bin,
        'INPUT_EXPECTED-VERSION': '1.12.0',
        'INPUT_EXPECTED-BINARY-SHA256': wrongHash ? 'a'.repeat(64) : createHash('sha256').update(await readFile(bin)).digest('hex'),
        'INPUT_VERSION': '',
        'INPUT_CACHE-LINKS': 'false',
        STATE_mbx_post: '',
        GITHUB_PATH: path.join(directory, 'path'),
        GITHUB_STATE: path.join(directory, 'state'),
        GITHUB_ENV: path.join(directory, 'env'),
        GITHUB_OUTPUT: path.join(directory, 'output'),
        GITHUB_STEP_SUMMARY: path.join(directory, 'summary')
      }
    }).catch(async error => {
      error.executed = await readFile(path.join(directory, 'executed')).then(() => true, () => false)
      throw error
    })
  }
  it('runs the generated bundle with the exact executable and network forbidden', async () => {
    const result = await invoke('mbx 1.12.0')
    expect(result.stdout).toContain('Set up mbx 1.12.0')
    expect(result.stdout).not.toContain('NETWORK_FORBIDDEN')
  })
  it('fails mismatches without release lookup', async () => {
    await expect(invoke('mbx 1.12.1')).rejects.toMatchObject({code: 1, stdout: expect.stringContaining('expected mbx 1.12.0')})
  })
  it('fails missing executables without release lookup', async () => {
    await expect(invoke('mbx 1.12.0', true)).rejects.toMatchObject({code: 1, stdout: expect.stringContaining('ENOENT')})
  })
  it('rejects wrong caller SHA-256 before running the actual executable', async () => {
    await expect(invoke('mbx 1.12.0', false, true)).rejects.toMatchObject({code: 1, executed: false, stdout: expect.stringContaining('SHA-256 does not match')})
  })
})
