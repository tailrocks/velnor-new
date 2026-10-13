import {
  closeSync,
  openSync,
  readSync,
  statSync
} from 'node:fs';
import path from 'node:path';
import { admissionFail } from './archive-admission-errors.mjs';

export const ADMISSION_SCHEMA = 'velnor-cache-quarantine-v1';
export const ADMISSION_LIMITS = Object.freeze({
  maxArchiveBytes: 10 * 1024 * 1024 * 1024,
  maxExpandedBytes: 10 * 1024 * 1024 * 1024,
  maxEntries: 1_000_000,
  maxPathBytes: 4096,
  maxComponentBytes: 255,
  maxLinkBytes: 4096,
  maxDepth: 256,
  maxDecompressionRatio: 1000
});

export const compressionMagic = Object.freeze({
  gzip: [0x1f, 0x8b],
  zstd: [0x28, 0xb5, 0x2f, 0xfd],
  'zstd-without-long': [0x28, 0xb5, 0x2f, 0xfd]
});

export const fileKinds = new Set(['File', 'OldFile', 'ContiguousFile']);
export const linkKinds = new Set(['SymbolicLink', 'Link']);

export function asAbsolutePath(value, name, baseDirectory = process.cwd()) {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0')) {
    admissionFail('ADMISSION_INPUT', `${name} must be a non-empty path`);
  }
  if (value.includes('\\')) admissionFail('ADMISSION_INPUT', `${name} cannot contain backslashes`);
  if (/[?*\[\]{}!]/.test(value)) admissionFail('ADMISSION_INPUT', `${name} must be an exact path`);
  return path.resolve(baseDirectory, value);
}

export function posixMemberPath(value, field) {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0')) {
    admissionFail('ADMISSION_MEMBER', `${field} is empty or contains NUL`);
  }
  if (value.includes('\\') || value.startsWith('/')) {
    admissionFail('ADMISSION_MEMBER', `${field} is not a relative POSIX path`);
  }
  const withoutSlash = value.endsWith('/') ? value.slice(0, -1) : value;
  if (withoutSlash.length === 0) admissionFail('ADMISSION_MEMBER', `${field} is empty`);
  const normalized = path.posix.normalize(withoutSlash);
  if (normalized !== withoutSlash || normalized.split('/').some(part => part === '')) {
    admissionFail('ADMISSION_MEMBER', `${field} is not canonical: ${JSON.stringify(value)}`);
  }
  const parts = normalized.split('/');
  if (parts.length > ADMISSION_LIMITS.maxDepth) admissionFail('ADMISSION_LIMIT', `${field} exceeds maximum depth`);
  if (Buffer.byteLength(normalized, 'utf8') > ADMISSION_LIMITS.maxPathBytes) admissionFail('ADMISSION_LIMIT', `${field} exceeds maximum length`);
  if (parts.some(part => Buffer.byteLength(part, 'utf8') > ADMISSION_LIMITS.maxComponentBytes)) admissionFail('ADMISSION_LIMIT', `${field} has an oversized component`);
  return normalized;
}

function archiveRootPath(workspace, value) {
  const canonical = asAbsolutePath(value, 'requested root', workspace);
  const relative = path.relative(workspace, canonical).replaceAll(path.sep, '/');
  return { canonical, archive: relative === '' ? '.' : posixMemberPath(relative, 'requested root') };
}

export function requestedRoots(paths, workspace) {
  if (!Array.isArray(paths) || paths.length === 0) admissionFail('ADMISSION_INPUT', 'at least one requested root is required');
  return paths.map((value, index) => {
    const root = archiveRootPath(workspace, value);
    return { index, original_root: root.canonical, archive_root: root.archive, quarantine_root: `roots/${index}` };
  });
}

export function readMagic(file, compression) {
  const expected = compressionMagic[compression];
  if (!expected) admissionFail('ADMISSION_INPUT', `unsupported compression method: ${compression}`);
  const fd = openSync(file, 'r');
  try {
    const header = Buffer.alloc(expected.length);
    const count = readSync(fd, header, 0, header.length, 0);
    if (count !== expected.length || !expected.every((byte, index) => header[index] === byte)) {
      admissionFail('ADMISSION_FORMAT', `archive header does not match ${compression}`);
    }
  }
  finally {
    closeSync(fd);
  }
}

export function rootMatch(member, roots) {
  return roots
    .filter(root => root.archive_root === '.' || member === root.archive_root || member.startsWith(`${root.archive_root}/`))
    .sort((left, right) => right.archive_root.length - left.archive_root.length || left.index - right.index)[0];
}

export function rootRelative(member, root) {
  if (root.archive_root === '.') return member === '.' ? '' : member;
  if (member === root.archive_root) return '';
  return member.slice(root.archive_root.length + 1);
}

export function validateMode(entry) {
  const mode = typeof entry.mode === 'number' && Number.isSafeInteger(entry.mode) ? entry.mode & 0o7777 : 0;
  if ((mode & 0o7000) !== 0) admissionFail('ADMISSION_MEMBER', `special mode bits are forbidden: ${entry.path}`);
  return mode;
}

export function validateLinkPath(value) {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\0') || value.includes('\\') || value.startsWith('/') || /^[A-Za-z]:/.test(value)) {
    admissionFail('ADMISSION_LINK', 'link target is invalid');
  }
  if (Buffer.byteLength(value, 'utf8') > ADMISSION_LIMITS.maxLinkBytes) admissionFail('ADMISSION_LIMIT', 'link target exceeds maximum length');
  return value;
}

function structuralDirectories(roots) {
  const result = new Set(['.']);
  for (const root of roots) {
    const components = root.archive_root.split('/');
    let prefix = '.';
    for (const component of components.slice(0, -1)) {
      prefix = path.posix.join(prefix, component);
      result.add(prefix);
    }
  }
  return result;
}

export function resolveSymlink(member, link, recordByPath, roots) {
  const structural = structuralDirectories(roots);
  const directory = value => {
    const record = recordByPath.get(value);
    if (record ? record.kind !== 'directory' : !structural.has(value)) {
      admissionFail('ADMISSION_LINK', 'raw symlink target traverses an unadmitted directory');
    }
  };
  let current = path.posix.dirname(member);
  let initial = '.';
  directory(initial);
  for (const component of current.split('/')) {
    if (component === '.') continue;
    initial = path.posix.join(initial, component);
    directory(initial);
  }
  const components = validateLinkPath(link).split('/');
  for (let index = 0; index < components.length; index++) {
    const component = components[index];
    if (component === '' || component === '.' || component === '..') {
      directory(current);
      if (component === '..') current = path.posix.join(current, '..');
    }
    else {
      current = path.posix.join(current, component);
    }
    if (index < components.length - 1) directory(current);
  }
  return posixMemberPath(current, 'symlink target');
}

export function checkArchiveFile(file) {
  const archive = asAbsolutePath(file, 'archive');
  let stat;
  try {
    stat = statSync(archive);
  }
  catch (error) {
    admissionFail('ADMISSION_IO', `cannot stat archive: ${error.message}`);
  }
  if (!stat.isFile()) admissionFail('ADMISSION_IO', 'archive is not a regular file');
  if (!Number.isSafeInteger(stat.size) || stat.size > ADMISSION_LIMITS.maxArchiveBytes) admissionFail('ADMISSION_LIMIT', 'compressed archive exceeds maximum size');
  return { archive, size: stat.size };
}

export const allowedPaxKeys = new Set([
  'atime', 'ctime', 'mtime', 'charset', 'comment', 'gid', 'uid', 'gname',
  'uname', 'linkpath', 'path', 'size', 'mode', 'dev', 'ino', 'nlink',
  'SCHILY.dev', 'SCHILY.ino', 'SCHILY.nlink'
]);

export function validatePaxMetadata(metadata) {
  for (const line of String(metadata).split('\n')) {
    if (line === '') continue;
    const separator = line.indexOf(' ');
    const equals = line.indexOf('=', separator + 1);
    if (separator < 1 || equals <= separator + 1) admissionFail('ADMISSION_METADATA', 'malformed PAX metadata');
    const key = line.slice(separator + 1, equals);
    if (!allowedPaxKeys.has(key)) admissionFail('ADMISSION_METADATA', `unsupported PAX metadata key: ${key}`);
  }
}
