import path from 'node:path'
import {access, lstat, realpath} from 'node:fs/promises'
import {constants} from 'node:fs'

export async function prepareComparisonPath(file: string, runnerTemp: string): Promise<void> {
  if (!runnerTemp || !path.isAbsolute(runnerTemp)) throw new Error('comparison-state requires an absolute RUNNER_TEMP')
  const root = await realpath(runnerTemp)
  const parent = await realpath(path.dirname(file))
  const relative = path.relative(root, parent)
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error('comparison-state must stay inside RUNNER_TEMP')
  }
  await access(parent, constants.W_OK)
  try {
    await lstat(file)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return
    throw error
  }
  throw new Error('comparison-state must be a fresh path; preexisting files and symlinks are forbidden')
}

export async function requireComparisonFile(file: string): Promise<void> {
  if (!(await lstat(file)).isFile()) throw new Error('comparison-state must be a regular file, never a symlink')
}

export function cacheTransportUnavailable(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false
  const failure = error as {code?: string, statusCode?: number, name?: string}
  return ['ECONNRESET', 'ECONNREFUSED', 'ETIMEDOUT', 'ENOTFOUND', 'EAI_AGAIN'].includes(failure.code || '') ||
    failure.statusCode === 429 || (typeof failure.statusCode === 'number' && failure.statusCode >= 500) ||
    failure.name === 'CacheReadDeniedError'
}

export function validateComparisonMode(file: string, strict: boolean, backend: string, mode: string): void {
  if (!file) return
  if (!strict || backend !== 'github' || mode !== 'objects') {
    throw new Error('comparison-state requires mbx-path, expected-version, backend github, and github-cache-mode objects')
  }
  if (!path.isAbsolute(file)) throw new Error('comparison-state must be an absolute path')
}

export function comparisonExportResult(output: string): {useful: boolean, digest: string} {
  const result: unknown = JSON.parse(output)
  if (!result || typeof result !== 'object') throw new Error('Invalid mbx comparison export report')
  const report = result as Record<string, unknown>
  if (report.version !== 1 || typeof report.useful_delta !== 'boolean' ||
      typeof report.exported !== 'boolean' || report.exported !== report.useful_delta ||
      typeof report.semantic_digest !== 'string' || !/^[0-9a-f]{64}$/.test(report.semantic_digest)) {
    throw new Error('Invalid mbx comparison export report: require version 1, useful_delta, matching exported, and semantic_digest')
  }
  return {useful: report.useful_delta, digest: report.semantic_digest}
}
