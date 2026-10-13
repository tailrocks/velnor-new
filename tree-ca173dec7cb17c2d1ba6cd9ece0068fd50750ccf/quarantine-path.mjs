import {
  lstatSync,
  mkdirSync,
  writeFileSync
} from 'node:fs';
import path from 'node:path';

export class ArchiveConfigurationError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`);
    this.name = 'ArchiveConfigurationError';
    this.code = code;
  }
}

function inspectOwnedDirectory(directory, runnerTemp) {
  const root = path.resolve(runnerTemp);
  const target = path.resolve(directory);
  if (target === root || !target.startsWith(`${root}${path.sep}`)) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', 'quarantine path is outside RUNNER_TEMP');
  }
  const relative = target.slice(root.length + 1).split(path.sep);
  const uid = typeof process.getuid === 'function' ? process.getuid() : undefined;
  let rootStat;
  try {
    rootStat = lstatSync(root);
  }
  catch (error) {
    throw new ArchiveConfigurationError('QUARANTINE_IO', `cannot inspect RUNNER_TEMP: ${error.message}`);
  }
  checkDirectory(rootStat, uid, 'RUNNER_TEMP');
  let current = root;
  for (const component of ['', ...relative.slice(0, -1)]) {
    if (component) current = path.join(current, component);
    try {
      const stat = lstatSync(current);
      checkDirectory(stat, uid, 'quarantine ancestor');
    }
    catch (error) {
      if (error.code === 'ENOENT') continue;
      if (error instanceof ArchiveConfigurationError) throw error;
      throw new ArchiveConfigurationError('QUARANTINE_IO', `cannot inspect quarantine ancestor: ${error.message}`);
    }
  }
}

function checkDirectory(stat, uid, label) {
  if (stat.isSymbolicLink() || !stat.isDirectory()) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', `${label} is not a directory`);
  }
  if (uid !== undefined && stat.uid !== uid && stat.uid !== 0) {
    throw new ArchiveConfigurationError('QUARANTINE_OWNER', `${label} is not runner-owned`);
  }
  if ((stat.mode & 0o022) !== 0) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', `${label} is writable by another user`);
  }
}

export function validateQuarantinePath(quarantinePath, runnerTemp = process.env.RUNNER_TEMP) {
  if (typeof runnerTemp !== 'string' || runnerTemp.length === 0 || !path.isAbsolute(runnerTemp)) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'RUNNER_TEMP is required');
  }
  if (typeof quarantinePath !== 'string' || !path.isAbsolute(quarantinePath)) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'quarantine path must be absolute');
  }
  const root = path.resolve(runnerTemp);
  const supplied = path.resolve(quarantinePath);
  if (!/^[a-f0-9]{64}$/.test(path.basename(supplied))) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'quarantine leaf must be lowercase SHA-256 hex');
  }
  const expectedParent = path.join(root, 'velnor', 'cache-staging');
  if (path.dirname(supplied) !== expectedParent) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', 'quarantine path is outside the closed staging domain');
  }
  inspectOwnedDirectory(supplied, root);
  try {
    const stat = lstatSync(supplied);
    if (stat.isSymbolicLink() || !stat.isDirectory()) {
      throw new ArchiveConfigurationError('QUARANTINE_SCOPE', 'quarantine path is not a directory');
    }
    throw new ArchiveConfigurationError('QUARANTINE_EXISTS', 'quarantine path must be absent');
  }
  catch (error) {
    if (error instanceof ArchiveConfigurationError) throw error;
    if (error.code !== 'ENOENT') {
      throw new ArchiveConfigurationError('QUARANTINE_IO', `cannot inspect quarantine path: ${error.message}`);
    }
  }
  return supplied;
}

function ensureOwnedDirectory(directory, runnerTemp) {
  const root = path.resolve(runnerTemp);
  const relative = path.relative(root, directory);
  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', 'staging directory is outside RUNNER_TEMP');
  }
  let current = root;
  const uid = typeof process.getuid === 'function' ? process.getuid() : undefined;
  for (const component of relative.split(path.sep).filter(Boolean)) {
    current = path.join(current, component);
    try {
      const stat = lstatSync(current);
      checkDirectory(stat, uid, 'staging component');
    }
    catch (error) {
      if (error instanceof ArchiveConfigurationError) throw error;
      if (error.code !== 'ENOENT') {
        throw new ArchiveConfigurationError('QUARANTINE_IO', `cannot inspect staging component: ${error.message}`);
      }
      mkdirSync(current, { mode: 0o700 });
    }
  }
}

export function createQuarantinePath(quarantinePath, descriptorDigest, runnerTemp = process.env.RUNNER_TEMP) {
  if (typeof runnerTemp !== 'string' || runnerTemp.length === 0 || !path.isAbsolute(runnerTemp)) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'RUNNER_TEMP is required');
  }
  if (!/^[a-f0-9]{64}$/.test(descriptorDigest || '')) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'descriptor digest must be lowercase SHA-256 hex');
  }
  if (typeof quarantinePath !== 'string' || !path.isAbsolute(quarantinePath)) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'quarantine path must be absolute');
  }
  const expected = path.join(path.resolve(runnerTemp), 'velnor', 'cache-staging', descriptorDigest);
  const supplied = path.resolve(quarantinePath);
  if (supplied !== expected) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', 'quarantine path is not the closed staging path');
  }
  validateQuarantinePath(supplied, runnerTemp);
  ensureOwnedDirectory(path.dirname(supplied), runnerTemp);
  mkdirSync(supplied, { mode: 0o700 });
  const finalStat = lstatSync(supplied);
  if (!finalStat.isDirectory() || finalStat.isSymbolicLink() || (finalStat.mode & 0o077) !== 0) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', 'created quarantine directory is unsafe');
  }
  return supplied;
}

export function writeAdmissionManifest(quarantinePath, manifest) {
  if (!manifest || manifest.schema !== 'velnor-cache-quarantine-v1' || !Array.isArray(manifest.entries)) {
    throw new ArchiveConfigurationError('QUARANTINE_MANIFEST', 'invalid admission manifest');
  }
  const target = path.join(quarantinePath, 'archive-admission.json');
  if (path.dirname(target) !== path.resolve(quarantinePath)) {
    throw new ArchiveConfigurationError('QUARANTINE_SCOPE', 'manifest path escaped quarantine');
  }
  writeFileSync(target, `${JSON.stringify(manifest)}\n`, { encoding: 'utf8', flag: 'wx', mode: 0o600 });
  return target;
}
