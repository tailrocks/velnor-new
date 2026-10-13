import { createHash } from 'node:crypto';
import { lstatSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ArchiveAdmissionError } from './archive-admission-errors.mjs';
import { qualifiedFoundationPython } from './foundation-python-verifier.mjs';

export const METADATA_CONTAINER_SOURCE_SHA256 =
  '3bba7f21fc728bea94c1d8202611537a1f8791585313e126a7a00b0249823fb1';
export const METADATA_CONTAINER_MAX_PREFIX_BYTES = 1024 * 1024;
export const METADATA_CONTAINER_MAX_CANDIDATES = 256;
export const METADATA_CONTAINER_MAX_TOTAL_PREFIX_BYTES = 64 * 1024 * 1024;
const METADATA_CONTAINER_MAX_FILE_BYTES = 10 * 1024 ** 3;
const METADATA_CONTAINER_MAGIC = Buffer.from([0x00, 0x05, 0x16, 0x07]);
const SOURCE_MAX_BYTES = 1024 * 1024;
const PREDICATE_TIMEOUT_MS = 30000;
const PREDICATE_OUTPUT_BYTES = 1024;
const PYTHON_DRIVER = `
import sys
def read_exact(count):
    chunks = []
    while count:
        chunk = sys.stdin.buffer.read(count)
        if not chunk:
            raise RuntimeError('metadata frame')
        chunks.append(chunk)
        count -= len(chunk)
    return b''.join(chunks)
source_length = int.from_bytes(read_exact(8), 'big')
if source_length > 1048576:
    raise RuntimeError('metadata source bound')
source = read_exact(source_length)
size = int.from_bytes(read_exact(8), 'big')
if size > 10737418240:
    raise RuntimeError('metadata file bound')
prefix_length = int.from_bytes(read_exact(8), 'big')
if prefix_length > 1048576:
    raise RuntimeError('metadata prefix bound')
prefix = read_exact(prefix_length)
if sys.stdin.buffer.read(1):
    raise RuntimeError('metadata frame')
namespace = {}
exec(compile(source, '<metadata_container.py>', 'exec'), namespace)
result = namespace['is_appledouble'](prefix, size)
sys.stdout.buffer.write(b'1' if result else b'0')
`;

function unavailable(reason) {
  const allowed = new Set([
    'unqualified-foundation-image',
    'foundation-closure-mismatch',
    'foundation-closure-unavailable',
    'source-integrity',
    'execution'
  ]);
  const detail = allowed.has(reason) ? reason : 'foundation-unavailable';
  return new ArchiveAdmissionError(
    'ADMISSION_METADATA_PREDICATE_UNAVAILABLE',
    `canonical metadata predicate unavailable: ${detail}`
  );
}

function canonicalSourcePath() {
  return fileURLToPath(new URL('./metadata_container.py', import.meta.url));
}

function verifyCanonicalSource() {
  let stat;
  let source;
  try {
    const filename = canonicalSourcePath();
    stat = lstatSync(filename);
    if (!stat.isFile() || stat.size > SOURCE_MAX_BYTES) throw unavailable('source-integrity');
    source = readFileSync(filename);
    if (source.length !== stat.size ||
        createHash('sha256').update(source).digest('hex') !== METADATA_CONTAINER_SOURCE_SHA256) {
      throw unavailable('source-integrity');
    }
  }
  catch (error) {
    if (error instanceof ArchiveAdmissionError) throw error;
    throw unavailable('source-integrity');
  }
  return source;
}

function qualifiedRuntime() {
  let runtime;
  try { runtime = qualifiedFoundationPython(); }
  catch (_error) { throw unavailable('execution'); }
  if (!runtime?.available || typeof runtime.executable !== 'string' || !path.isAbsolute(runtime.executable) ||
      JSON.stringify(runtime.argv) !== JSON.stringify(['-I', '-S', '-B']) ||
      runtime.environment?.LANG !== 'C' || runtime.environment?.LC_ALL !== 'C' ||
      Object.keys(runtime.environment).sort().join(',') !== 'LANG,LC_ALL') {
    throw unavailable(runtime?.available === false ? runtime.reason : 'execution');
  }
  return runtime;
}

function frameInput(source, prefix, size) {
  const encodeLength = value => {
    const bytes = Buffer.alloc(8);
    bytes.writeBigUInt64BE(BigInt(value));
    return bytes;
  };
  return Buffer.concat([
    encodeLength(source.length), source,
    encodeLength(size), encodeLength(prefix.length), prefix
  ]);
}

function invokePredicate(prefix, size) {
  const source = verifyCanonicalSource();
  const runtime = qualifiedRuntime();
  let result;
  try {
    result = spawnSync(runtime.executable, [...runtime.argv, '-c', PYTHON_DRIVER], {
      cwd: '/',
      env: { LANG: 'C', LC_ALL: 'C' },
      input: frameInput(source, prefix, size),
      encoding: 'utf8',
      timeout: PREDICATE_TIMEOUT_MS,
      maxBuffer: PREDICATE_OUTPUT_BYTES,
      shell: false,
      windowsHide: true
    });
  }
  catch (_error) {
    throw unavailable('execution');
  }
  if (result.error || result.signal || result.status !== 0) throw unavailable('execution');
  const output = typeof result.stdout === 'string' ? result.stdout.trim() : '';
  if (output === '1') {
    throw new ArchiveAdmissionError(
      'ADMISSION_METADATA_APPLEDOUBLE',
      'literal AppleDouble metadata container is forbidden'
    );
  }
  if (output !== '0') throw unavailable('execution');
}

function accountCandidate(prefix, budget) {
  if (!budget) return;
  budget.candidateCount += 1;
  budget.prefixBytes += prefix.length;
  if (budget.candidateCount > METADATA_CONTAINER_MAX_CANDIDATES ||
      budget.prefixBytes > METADATA_CONTAINER_MAX_TOTAL_PREFIX_BYTES) {
    throw new ArchiveAdmissionError('ADMISSION_LIMIT', 'metadata predicate budget exceeded');
  }
}

export function inspectMetadataPrefix(prefix, size, budget) {
  if (!Buffer.isBuffer(prefix) && !(prefix instanceof Uint8Array)) {
    throw new ArchiveAdmissionError('ADMISSION_METADATA', 'metadata prefix is invalid');
  }
  if (!Number.isSafeInteger(size) || size < 0 || size > METADATA_CONTAINER_MAX_FILE_BYTES) {
    throw new ArchiveAdmissionError('ADMISSION_LIMIT', 'metadata container exceeds maximum size');
  }
  const bytes = Buffer.from(prefix);
  if (bytes.length > METADATA_CONTAINER_MAX_PREFIX_BYTES) {
    throw new ArchiveAdmissionError('ADMISSION_LIMIT', 'metadata prefix exceeds maximum size');
  }
  if (bytes.length < METADATA_CONTAINER_MAGIC.length ||
      !METADATA_CONTAINER_MAGIC.equals(bytes.subarray(0, METADATA_CONTAINER_MAGIC.length))) return;
  accountCandidate(bytes, budget);
  invokePredicate(bytes, size);
}
