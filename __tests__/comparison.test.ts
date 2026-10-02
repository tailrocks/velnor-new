import {afterEach, describe, expect, it} from 'vitest'
import {mkdtemp, rm, symlink, writeFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import path from 'node:path'
import {cacheTransportUnavailable, comparisonExportResult, prepareComparisonPath, requireComparisonFile, validateComparisonMode} from '../src/comparison.js'

const directories: string[] = []
afterEach(async () => { await Promise.all(directories.splice(0).map(directory => rm(directory, {recursive: true, force: true}))) })

describe('owner comparison report', () => {
  it('accepts only strict objects transport with an absolute baseline', () => {
    expect(() => validateComparisonMode('/tmp/baseline', true, 'github', 'objects')).not.toThrow()
    expect(() => validateComparisonMode('', false, 'local', 'target')).not.toThrow()
    for (const [strict, backend, mode] of [[false, 'github', 'objects'], [true, 'local', 'objects'], [true, 'github', 'target']] as const) {
      expect(() => validateComparisonMode('/tmp/baseline', strict, backend, mode)).toThrow(/requires/)
    }
    expect(() => validateComparisonMode('baseline', true, 'github', 'objects')).toThrow(/absolute/)
  })
  it('permits only a fresh path inside the runner temporary directory', async () => {
    const root = await mkdtemp(path.join(tmpdir(), 'mbx-path-'))
    directories.push(root)
    const file = path.join(root, 'baseline')
    await expect(prepareComparisonPath(file, root)).resolves.toBeUndefined()
    await expect(prepareComparisonPath(file, '')).rejects.toThrow(/RUNNER_TEMP/)
    await expect(prepareComparisonPath(path.join(path.dirname(root), 'outside'), root)).rejects.toThrow(/inside RUNNER_TEMP/)
    await writeFile(file, '{}')
    await expect(prepareComparisonPath(file, root)).rejects.toThrow(/fresh path/)
    await expect(requireComparisonFile(file)).resolves.toBeUndefined()
    const link = path.join(root, 'symlink')
    await symlink(file, link)
    await expect(prepareComparisonPath(link, root)).rejects.toThrow(/fresh path/)
    await expect(requireComparisonFile(link)).rejects.toThrow(/never a symlink/)
  })
  it('rejects symlink parent escapes', async () => {
    const root = await mkdtemp(path.join(tmpdir(), 'mbx-root-'))
    const outside = await mkdtemp(path.join(tmpdir(), 'mbx-outside-'))
    directories.push(root, outside)
    await symlink(outside, path.join(root, 'escape'))
    await expect(prepareComparisonPath(path.join(root, 'escape', 'baseline'), root)).rejects.toThrow(/inside RUNNER_TEMP/)
  })
  it('classifies only known transport failures as optional', () => {
    for (const error of [{code: 'ECONNRESET'}, {code: 'ENOTFOUND'}, {statusCode: 503}, {statusCode: 429}, {name: 'CacheReadDeniedError'}]) {
      expect(cacheTransportUnavailable(error)).toBe(true)
    }
    for (const error of [new Error('bug'), {code: 'EACCES'}, {statusCode: 400}, {name: 'ValidationError'}, null]) {
      expect(cacheTransportUnavailable(error)).toBe(false)
    }
  })
  it.each([true, false])('preserves owner useful_delta=%s and semantic digest', useful => {
    expect(comparisonExportResult(JSON.stringify({version: 1, useful_delta: useful, exported: useful, semantic_digest: 'a'.repeat(64)}))).toEqual({useful, digest: 'a'.repeat(64)})
  })
  it.each([{}, {version: 2}, {version: 1, useful_delta: 'false'}, {version: 1, useful_delta: true, exported: false, semantic_digest: 'a'.repeat(64)}, {version: 1, useful_delta: true, semantic_digest: 'bad'}, null, []])('fails closed on malformed report %j', report => {
    expect(() => comparisonExportResult(JSON.stringify(report))).toThrow()
  })
})
