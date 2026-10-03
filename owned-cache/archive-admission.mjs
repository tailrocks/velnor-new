import { createHash } from 'node:crypto';
import { createReadStream, createWriteStream, lstatSync, readdirSync, rmSync } from 'node:fs';
import { Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import path from 'node:path';
import { extract } from 'tar/extract';
import { Parser } from 'tar/parse';
import { decodeArchive } from './archive-codec.mjs';
import { ArchiveAdmissionError, admissionFail } from './archive-admission-errors.mjs';
import { ArchiveConfigurationError } from './quarantine-path.mjs';
import { inspectMetadataPrefix, METADATA_CONTAINER_MAX_PREFIX_BYTES } from './appledouble-admission.mjs';
import { copyHardlinkMetadata, materializeHardlinks, memberKind, memberSize, quarantineFilter, resolveHardlinks, validateMemberAncestors } from './archive-hardlinks.mjs';
import {
  ADMISSION_LIMITS,
  ADMISSION_SCHEMA,
  asAbsolutePath,
  checkArchiveFile,
  readMagic,
  requestedRoots,
  rootMatch,
  rootRelative,
  posixMemberPath,
  resolveSymlink,
  validateLinkPath,
  validateMode,
  validatePaxMetadata,
  linkKinds
} from './archive-admission-policy.mjs';

export { ADMISSION_LIMITS, ADMISSION_SCHEMA, ArchiveAdmissionError };

function recordArchiveEntry(entry, roots, records, recordByPath, recordByAlias, state, failOnce) {
  if (records.length >= ADMISSION_LIMITS.maxEntries) admissionFail('ADMISSION_LIMIT', 'archive has too many members');
  const member = posixMemberPath(entry.path, 'member path');
  const root = rootMatch(member, roots);
  if (!root) admissionFail('ADMISSION_FOREIGN', `member is outside requested roots: ${member}`);
  if (recordByPath.has(member)) admissionFail('ADMISSION_MEMBER', `duplicate member: ${member}`);
  const mode = validateMode(entry);
  const relative = rootRelative(member, root);
  validateRootRelative(relative, member);
  const kind = memberKind(entry, member);
  const size = memberSize(entry, member);
  if (kind === 'file') {
    state.expandedBytes += size;
    if (state.expandedBytes > ADMISSION_LIMITS.maxExpandedBytes) admissionFail('ADMISSION_LIMIT', 'expanded archive exceeds maximum size');
  }
  const record = {
    path: member,
    root_index: root.index,
    relative_path: relative,
    quarantine_path: relative === '' ? root.quarantine_root : `${root.quarantine_root}/${relative}`,
    kind,
    mode,
    size,
    content_sha256: null,
    link_target: null,
    resolved_link_path: null
  };
  if (linkKinds.has(entry.type)) record.link_target = validateLinkPath(entry.linkpath);
  const alias = quarantineAlias(record.quarantine_path);
  const previous = recordByAlias.get(alias);
  if (previous && previous.path !== member) admissionFail('ADMISSION_MEMBER', `path aliases another member: ${member}`);
  records.push(record);
  recordByPath.set(member, record);
  recordByAlias.set(alias, record);
  hashEntry(entry, record, state, failOnce);
  entry.resume();
}

function quarantineAlias(value) {
  try {
    return value.split('/').map(component => component.normalize('NFKD').toLocaleLowerCase('en-US')).join('/');
  }
  catch (error) {
    admissionFail('ADMISSION_MEMBER', `member path has invalid Unicode: ${error.message}`);
  }
}

function validateRootRelative(relative, member) {
  if (relative === '' || !relative.split('/').includes('..')) return;
  admissionFail('ADMISSION_FOREIGN', `member escapes requested root: ${member}`);
}

function hashEntry(entry, record, state, failOnce) {
  if (record.kind !== 'file') return;
  const contentHash = createHash('sha256');
  const prefixParts = [];
  let prefixBytes = 0;
  entry.on('data', chunk => {
    contentHash.update(chunk);
    if (prefixBytes >= METADATA_CONTAINER_MAX_PREFIX_BYTES) return;
    const count = Math.min(chunk.length, METADATA_CONTAINER_MAX_PREFIX_BYTES - prefixBytes);
    prefixParts.push(chunk.subarray(0, count));
    prefixBytes += count;
  });
  entry.on('end', () => {
    try {
      inspectMetadataPrefix(Buffer.concat(prefixParts, prefixBytes), record.size, state.metadata);
      record.content_sha256 = contentHash.digest('hex');
    }
    catch (error) {
      failOnce(error);
    }
  });
}

function parseArchive(file, compression, roots) {
  const archiveHash = createHash('sha256');
  const records = [];
  const recordByPath = new Map();
  const recordByAlias = new Map();
  const state = { expandedBytes: 0, metadata: { candidateCount: 0, prefixBytes: 0 } };
  let stream;
  let settled = false;
  return new Promise((resolve, reject) => {
    const failOnce = error => {
      if (settled) return;
      settled = true;
      stream?.destroy();
      const admissionError = error instanceof ArchiveAdmissionError
        ? error
        : new ArchiveAdmissionError(decodeErrorCode(error), error?.message || String(error));
      reject(admissionError);
    };
    const parser = new Parser({
      strict: true,
      maxDecompressionRatio: ADMISSION_LIMITS.maxDecompressionRatio,
      maxMetaEntrySize: 1024 * 1024,
      onReadEntry: entry => readEntry(entry, roots, records, recordByPath, recordByAlias, state, failOnce)
    });
    parser.on('meta', metadata => catchAdmission(() => validatePaxMetadata(metadata), failOnce));
    parser.on('ignoredEntry', entry => {
      if (entry?.meta) failOnce(new ArchiveAdmissionError('ADMISSION_METADATA', 'PAX metadata exceeds maximum size'));
    });
    parser.on('error', failOnce);
    parser.on('end', () => {
      if (settled) return;
      try {
        resolveHardlinks(records, recordByPath, state, ADMISSION_LIMITS.maxExpandedBytes);
        validateMemberAncestors(records, recordByPath);
        validateLinks(records, recordByPath, roots);
        settled = true;
        resolve({ records, expandedBytes: state.expandedBytes, archive_sha256: archiveHash.digest('hex') });
      }
      catch (error) {
        failOnce(error);
      }
    });
    try {
      const decoded = decodeArchive(file, compression, {
        maxDecodedBytes: ADMISSION_LIMITS.maxExpandedBytes,
        maxRatio: ADMISSION_LIMITS.maxDecompressionRatio
      });
      stream = decoded.stream;
      decoded.source.on('data', chunk => archiveHash.update(chunk));
      stream.on('error', failOnce);
      stream.pipe(parser);
    }
    catch (error) {
      failOnce(error);
    }
  });
}

function readEntry(entry, roots, records, recordByPath, recordByAlias, state, failOnce) {
  try {
    recordArchiveEntry(entry, roots, records, recordByPath, recordByAlias, state, failOnce);
  }
  catch (error) {
    failOnce(error);
    entry.resume();
  }
}

function catchAdmission(action, failOnce) {
  try {
    action();
  }
  catch (error) {
    failOnce(error);
  }
}

function decodeErrorCode(error) {
  if (error?.code === 'ARCHIVE_DECODE_LIMIT' || error?.code === 'ARCHIVE_DECODE_RATIO') return 'ADMISSION_LIMIT';
  return 'ADMISSION_FORMAT';
}

function validateLinks(records, recordByPath, roots) {
  for (const record of records) {
    if (record.kind !== 'symlink') continue;
    const targetPath = resolveSymlink(record.path, record.link_target, recordByPath, roots);
    const target = recordByPath.get(targetPath);
    if (!target || !['file', 'directory'].includes(target.kind)) {
      admissionFail('ADMISSION_LINK', `symlink target is outside admitted closure: ${record.path}`);
    }
    record.resolved_link_path = target.quarantine_path;
  }
}

async function extractDecodedArchive(archivePath, compression, target, entries) {
  let decoded;
  try {
    decoded = decodeArchive(archivePath, compression, {
      maxDecodedBytes: ADMISSION_LIMITS.maxExpandedBytes,
      maxRatio: ADMISSION_LIMITS.maxDecompressionRatio
    });
  }
  catch (error) {
    throw new ArchiveAdmissionError(decodeErrorCode(error), error.message || String(error));
  }
  const unpack = extract({
    cwd: target,
    strict: true,
    preservePaths: false,
    preserveOwner: false,
    noMtime: true,
    chmod: true,
    processUmask: 0o022,
    dmode: 0o700,
    fmode: 0o600,
    maxDepth: ADMISSION_LIMITS.maxDepth,
    maxDecompressionRatio: ADMISSION_LIMITS.maxDecompressionRatio,
    filter: quarantineFilter(target, entries)
  });
  await pipeDecoder(decoded, unpack);
}

async function snapshotArchive(sourcePath, quarantinePath, manifest) {
  const snapshot = path.join(quarantinePath, '.archive-input');
  const hash = createHash('sha256');
  let bytes = 0;
  const bounded = new Transform({
    transform(chunk, _encoding, callback) {
      bytes += chunk.length;
      if (bytes > ADMISSION_LIMITS.maxArchiveBytes) {
        callback(new ArchiveAdmissionError('ADMISSION_LIMIT', 'archive changed beyond size limit'));
        return;
      }
      hash.update(chunk);
      callback(null, chunk);
    }
  });
  try {
    await pipeline(createReadStream(sourcePath), bounded, createWriteStream(snapshot, { flags: 'wx', mode: 0o600 }));
  }
  catch (error) {
    rmSync(snapshot, { force: true });
    throw error instanceof ArchiveAdmissionError
      ? error
      : new ArchiveAdmissionError('ADMISSION_IO', error.message || String(error));
  }
  if (bytes !== manifest.archive_bytes || hash.digest('hex') !== manifest.archive_sha256) {
    rmSync(snapshot, { force: true });
    throw new ArchiveAdmissionError('ADMISSION_RACE', 'archive changed after admission');
  }
  return snapshot;
}

function pipeDecoder(decoded, unpack) {
  return new Promise((resolve, reject) => {
    let settled = false;
    const finish = error => {
      if (settled) return;
      settled = true;
      if (error) {
        decoded.stream.destroy();
        unpack.destroy(error);
        reject(error instanceof ArchiveAdmissionError ? error : new ArchiveAdmissionError(decodeErrorCode(error), error.message));
      }
      else resolve();
    };
    decoded.stream.on('error', finish);
    unpack.on('error', finish);
    unpack.on('close', () => finish());
    decoded.stream.pipe(unpack);
  });
}

export async function extractAdmittedArchive(archivePath, manifest, quarantinePath) {
  if (!manifest || manifest.schema !== ADMISSION_SCHEMA || !Array.isArray(manifest.entries)) {
    throw new ArchiveAdmissionError('ADMISSION_MANIFEST', 'invalid admission manifest');
  }
  if (typeof quarantinePath !== 'string' || !path.isAbsolute(quarantinePath)) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'quarantine path must be absolute');
  }
  const target = path.resolve(quarantinePath);
  let stat;
  try {
    stat = lstatSync(target);
  }
  catch (error) {
    throw new ArchiveAdmissionError('ADMISSION_QUARANTINE', `cannot inspect quarantine directory: ${error.message}`);
  }
  const uid = typeof process.getuid === 'function' ? process.getuid() : undefined;
  if (uid !== undefined && stat.uid !== uid && stat.uid !== 0) throw new ArchiveConfigurationError('QUARANTINE_OWNER', 'quarantine directory is not runner-owned');
  if (stat.isSymbolicLink() || !stat.isDirectory() || (stat.mode & 0o077) !== 0) {
    throw new ArchiveAdmissionError('ADMISSION_QUARANTINE', 'quarantine directory is unsafe');
  }
  try {
    if (readdirSync(target).length !== 0) throw new ArchiveAdmissionError('ADMISSION_QUARANTINE', 'quarantine directory must be empty');
  }
  catch (error) {
    if (error instanceof ArchiveAdmissionError) throw error;
    throw new ArchiveAdmissionError('ADMISSION_QUARANTINE', `cannot inspect quarantine directory: ${error.message}`);
  }
  const snapshot = await snapshotArchive(archivePath, target, manifest);
  try {
    const roots = manifest.ordered_roots.map(root => root.original_root);
    const current = await admitArchive(snapshot, manifest.compression, roots, { workspace: manifest.workspace });
    if (JSON.stringify(current) !== JSON.stringify(manifest)) throw new ArchiveAdmissionError('ADMISSION_RACE', 'admission manifest changed after parsing');
    await extractDecodedArchive(snapshot, manifest.compression, target, new Map(current.entries.map(entry => [entry.member, entry])));
    await materializeHardlinks(target, current.entries);
    return current;
  }
  finally {
    rmSync(snapshot, { force: true });
  }
}

export async function admitArchive(archivePath, compressionMethod, paths, options = {}) {
  const { archive, size } = checkArchiveFile(archivePath);
  const workspace = asAbsolutePath(options.workspace || process.env.GITHUB_WORKSPACE || process.cwd(), 'workspace');
  const roots = requestedRoots(paths, workspace);
  readMagic(archive, compressionMethod);
  const parsed = await parseArchive(archive, compressionMethod, roots);
  const entries = parsed.records.map(entry => {
    const output = {
      member: entry.path,
      original_root_index: entry.root_index,
      relative_path: entry.relative_path,
      quarantine_path: entry.quarantine_path,
      kind: entry.kind,
      mode: entry.mode,
      size: entry.size,
      content_sha256: entry.content_sha256,
      link_target: entry.link_target,
      resolved_link_path: entry.resolved_link_path
    };
    copyHardlinkMetadata(output, entry);
    return output;
  });
  return {
    schema: ADMISSION_SCHEMA,
    archive_sha256: parsed.archive_sha256,
    archive_bytes: size,
    compression: compressionMethod,
    workspace,
    ordered_roots: roots,
    entries,
    totals: { entry_count: parsed.records.length, expanded_bytes: parsed.expandedBytes }
  };
}

export {
  ArchiveConfigurationError,
  createQuarantinePath,
  validateQuarantinePath,
  writeAdmissionManifest
} from './quarantine-path.mjs';
