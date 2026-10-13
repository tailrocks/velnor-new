import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';

// Complete trees include bytecode: -B disables writes, not bytecode reads.
const MAX_FILES = 100000;
const MAX_BYTES = 32 * 1024 ** 3;
function sameStat(left, right) {
  return ['dev', 'ino', 'mode', 'uid', 'gid', 'size', 'mtimeMs', 'ctimeMs']
    .every(key => left[key] === right[key]);
}
function directoryChildren(filename, remaining) {
  const children = [];
  const directory = fs.opendirSync(filename);
  try {
    let entry;
    while ((entry = directory.readSync()) !== null) {
      if (children.length >= remaining) throw new Error('foundation directory bound');
      children.push(entry.name);
    }
  } finally { directory.closeSync(); }
  return children.sort();
}
function hashFile(filename, budget, expected) {
  const fd = fs.openSync(filename, fs.constants.O_RDONLY |
    fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  try {
    const before = fs.fstatSync(fd);
    if (!sameStat(expected, before) || !before.isFile() ||
        before.uid !== 0 || (before.mode & 0o022) !== 0 ||
        before.size > MAX_BYTES - budget.bytes)
      throw new Error('foundation closure bound');
    const digest = crypto.createHash('sha256');
    const buffer = Buffer.alloc(1024 * 1024);
    let count;
    let bytes = 0;
    while ((count = fs.readSync(fd, buffer)) !== 0) {
      bytes += count;
      if (bytes > before.size) throw new Error('foundation changed during read');
      digest.update(buffer.subarray(0, count));
    }
    const after = fs.fstatSync(fd);
    if (bytes !== before.size || !sameStat(before, after) ||
        !sameStat(after, fs.lstatSync(filename)))
      throw new Error('foundation changed during read');
    budget.bytes += bytes;
    return { size: bytes, sha256: digest.digest('hex') };
  } finally { fs.closeSync(fd); }
}

export function inventoryFoundation(roots) {
  const entries = new Map();
  const expanded = new Set();
  const budget = { bytes: 0 };
  function captureDirectory(filename) {
    if (entries.has(filename)) return;
    if (entries.size >= MAX_FILES) throw new Error('foundation entry bound');
    let stat;
    try { stat = fs.lstatSync(filename); }
    catch (error) {
      if (error.code !== 'ENOENT') throw error;
      entries.set(filename, { kind: 'ancestor-absent' });
      if (filename !== '/') captureDirectory(path.dirname(filename));
      return;
    }
    if (!stat.isDirectory() && !stat.isSymbolicLink())
      throw new Error('foundation ancestor is not a directory');
    if (stat.uid !== 0 || (!stat.isSymbolicLink() && (stat.mode & 0o022) !== 0))
      throw new Error('foundation ancestor is mutable');
    const identity = { mode: stat.mode & 0o7777, uid: stat.uid, gid: stat.gid };
    entries.set(filename, stat.isSymbolicLink() ?
      { ...identity, kind: 'ancestor-symlink', target: fs.readlinkSync(filename) } :
      { ...identity, kind: 'ancestor-directory' });
    if (filename !== '/') captureDirectory(path.dirname(filename));
    if (stat.isSymbolicLink())
      captureDirectory(path.resolve(path.dirname(filename), fs.readlinkSync(filename)));
  }
  function visit(filename) {
    if (!path.isAbsolute(filename) || path.normalize(filename) !== filename)
      throw new Error('foundation path');
    if (expanded.has(filename)) return;
    captureDirectory(path.dirname(filename));
    if (entries.size >= MAX_FILES) throw new Error('foundation entry bound');
    let stat;
    try { stat = fs.lstatSync(filename); }
    catch (error) {
      if (error.code !== 'ENOENT') throw error;
      entries.set(filename, { kind: 'absent' });
      expanded.add(filename);
      return;
    }
    if (stat.uid !== 0 || (!stat.isSymbolicLink() && (stat.mode & 0o022) !== 0))
      throw new Error('foundation is mutable by unprivileged users');
    expanded.add(filename);
    const identity = { mode: stat.mode & 0o7777, uid: stat.uid, gid: stat.gid };
    if (stat.isSymbolicLink()) {
      const target = fs.readlinkSync(filename);
      entries.set(filename, { ...identity, kind: 'symlink', target });
      visit(path.resolve(path.dirname(filename), target));
    } else if (stat.isDirectory()) {
      const children = directoryChildren(filename, MAX_FILES - entries.size);
      entries.set(filename, { ...identity, kind: 'directory', children });
      for (const child of children) visit(path.join(filename, child));
      if (!sameStat(stat, fs.lstatSync(filename)) ||
          JSON.stringify(children) !== JSON.stringify(directoryChildren(filename, MAX_FILES)))
        throw new Error('foundation directory changed during traversal');
    } else if (stat.isFile()) {
      entries.set(filename, { ...identity, kind: 'file', ...hashFile(filename, budget, stat) });
    } else throw new Error('foundation special file');
  }
  // Capture parent resolution for every entry, including followed link targets.
  for (const root of roots) visit(root);
  return [...entries].sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0)
    .map(([filename, record]) => ({ path: filename, ...record }));
}

export function canonicalInventory(value) { return JSON.stringify(value); }
