import { createHash } from 'node:crypto';
import {
  closeSync, constants, fchmodSync, fstatSync, lstatSync,
  openSync, readSync, writeSync
} from 'node:fs';
import { ArchiveAdmissionError } from './archive-admission-errors.mjs';

function fail(message) {
  throw new ArchiveAdmissionError('ADMISSION_RACE', message);
}

function regular(fd, expected, label) {
  const stat = fstatSync(fd, { bigint: true });
  if (!stat.isFile() || (stat.mode & 0o7000n) !== 0n || stat.nlink !== 1n) fail(`${label} is not an independent regular file`);
  if (stat.size !== BigInt(expected.size) || Number(stat.mode & 0o7777n) !== expected.mode) fail(`${label} metadata differs`);
  return stat;
}

function unchanged(before, after, label) {
  for (const field of ['dev', 'ino', 'mode', 'nlink', 'size', 'mtimeNs', 'ctimeNs']) {
    if (before[field] !== after[field]) fail(`${label} changed during copy`);
  }
}

function samePath(filename, stat, label) {
  const current = lstatSync(filename, { bigint: true });
  if (!current.isFile() || current.dev !== stat.dev || current.ino !== stat.ino) fail(`${label} pathname changed`);
}

function closeDescriptor(fd) {
  if (fd === undefined) return;
  try { closeSync(fd); }
  catch { fail('hardlink descriptor close failed'); }
}

function digest(fd, expected) {
  const hash = createHash('sha256');
  const buffer = Buffer.alloc(1024 * 1024);
  let position = 0;
  while (position < expected.size) {
    const count = readSync(fd, buffer, 0, Math.min(buffer.length, expected.size - position), position);
    if (count === 0) fail('hardlink destination ended early');
    hash.update(buffer.subarray(0, count));
    position += count;
  }
  if (hash.digest('hex') !== expected.content_sha256) fail('hardlink destination bytes differ');
}

function copyBytes(source, destination, expected) {
  const hash = createHash('sha256');
  const buffer = Buffer.alloc(1024 * 1024);
  let position = 0;
  while (position < expected.size) {
    const count = readSync(source, buffer, 0, Math.min(buffer.length, expected.size - position), position);
    if (count === 0) fail('hardlink source ended early');
    hash.update(buffer.subarray(0, count));
    let written = 0;
    while (written < count) {
      const amount = writeSync(destination, buffer, written, count - written, position + written);
      if (amount <= 0) fail('hardlink destination write stopped');
      written += amount;
    }
    position += count;
  }
  if (hash.digest('hex') !== expected.content_sha256) fail('hardlink source bytes differ');
}

// Caller validates all ancestors inside a fresh quiescent owned job namespace.
// Node has no openat API: this does not claim safety against arbitrary same-UID
// concurrent ancestor mutation. Final components are held no-follow descriptors.
export function copyPrivateRegular(sourcePath, destinationPath, expected) {
  let source;
  let destination;
  try {
    if (![constants.O_NOFOLLOW, constants.O_NONBLOCK].every(value => Number.isInteger(value) && value > 0)) fail('required descriptor flags unavailable');
    source = openSync(sourcePath, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    const before = regular(source, expected, 'hardlink source');
    samePath(sourcePath, before, 'hardlink source');
    destination = openSync(destinationPath, constants.O_RDWR | constants.O_CREAT |
      constants.O_EXCL | constants.O_NOFOLLOW | constants.O_NONBLOCK, 0o600);
    copyBytes(source, destination, expected);
    unchanged(before, regular(source, expected, 'hardlink source'), 'hardlink source');
    samePath(sourcePath, before, 'hardlink source');
    fchmodSync(destination, expected.mode);
    const completed = regular(destination, expected, 'hardlink destination');
    digest(destination, expected);
    unchanged(completed, regular(destination, expected, 'hardlink destination'), 'hardlink destination');
    samePath(destinationPath, completed, 'hardlink destination');
  }
  catch (error) {
    if (error instanceof ArchiveAdmissionError) throw error;
    throw new ArchiveAdmissionError('ADMISSION_RACE', 'hardlink descriptor copy failed');
  }
  finally {
    try { closeDescriptor(destination); }
    finally { closeDescriptor(source); }
  }
}
