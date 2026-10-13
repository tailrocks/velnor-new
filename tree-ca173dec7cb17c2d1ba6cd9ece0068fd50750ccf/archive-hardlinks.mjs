import { lstatSync, realpathSync } from "node:fs";
import path from 'node:path';
import { copyPrivateRegular } from './archive-private-copy.mjs';
import { ArchiveAdmissionError, admissionFail } from './archive-admission-errors.mjs';
import {
  fileKinds,
  posixMemberPath,
  validateMode
} from './archive-admission-policy.mjs';

export function memberKind(entry, member) {
  if (entry.type === 'Directory') return 'directory';
  if (fileKinds.has(entry.type)) return 'file';
  if (entry.type === 'SymbolicLink') return 'symlink';
  if (entry.type === 'Link') return 'hardlink';
  admissionFail('ADMISSION_MEMBER', `unsupported member type: ${entry.type}`);
}

export function memberSize(entry, member) {
  if (typeof entry.size !== 'number' || !Number.isSafeInteger(entry.size) || entry.size < 0) {
    admissionFail('ADMISSION_MEMBER', `invalid member size: ${member}`);
  }
  return entry.size;
}

export function resolveHardlinks(records, recordByPath, state, maxExpandedBytes) {
  const hardlinkPaths = new Set(records.filter(record => record.kind === 'hardlink').map(record => record.path));
  for (const record of records) {
    if (record.kind !== 'hardlink') continue;
    if (record.size !== 0) admissionFail('ADMISSION_LINK', `hardlink has nonzero size: ${record.path}`);
    const targetPath = posixMemberPath(path.posix.normalize(record.link_target), 'hardlink target');
    const target = recordByPath.get(targetPath);
    if (!target || target.kind !== 'file' || hardlinkPaths.has(target.path)) {
      admissionFail('ADMISSION_LINK', `hardlink target is not a direct regular file: ${record.path}`);
    }
    if (target.mode !== record.mode || !target.content_sha256) {
      admissionFail('ADMISSION_LINK', `hardlink target metadata differs: ${record.path}`);
    }
    state.expandedBytes += target.size;
    if (state.expandedBytes > maxExpandedBytes) {
      admissionFail('ADMISSION_LIMIT', 'expanded archive exceeds maximum size');
    }
    record.hardlink_wire_target = record.link_target;
    record.hardlink_target = target.path;
    record.hardlink_source_quarantine_path = target.quarantine_path;
    record.kind = 'file';
    record.size = target.size;
    record.content_sha256 = target.content_sha256;
    record.link_target = null;
  }
}

export function validateMemberAncestors(records, recordByPath) {
  for (const record of records) {
    const parts = record.path.split('/');
    let ancestorPath = '';
    for (let index = 0; index < parts.length - 1; index += 1) {
      ancestorPath = ancestorPath ? `${ancestorPath}/${parts[index]}` : parts[index];
      const ancestor = recordByPath.get(ancestorPath);
      if (ancestor && ancestor.kind !== 'directory') {
        admissionFail('ADMISSION_MEMBER', `member ancestor is not a directory: ${record.path}`);
      }
    }
  }
}

export function copyHardlinkMetadata(output, record) {
  if (!record.hardlink_target) return;
  Object.defineProperties(output, {
    hardlink_target: { value: record.hardlink_target, enumerable: false },
    hardlink_wire_target: { value: record.hardlink_wire_target, enumerable: false },
    hardlink_source_quarantine_path: {
      value: record.hardlink_source_quarantine_path, enumerable: false
    }
  });
}

function normalizedRecordPath(value) {
  return posixMemberPath(String(value), 'member path');
}

export function quarantineFilter(quarantinePath, entries) {
  return (_entryPath, entry) => {
    const record = entries.get(normalizedRecordPath(entry.path));
    if (!record) admissionFail('ADMISSION_FOREIGN', `member changed during quarantine extraction: ${entry.path}`);
    if (record.hardlink_target) {
      assertHardlinkEntry(entry, record);
      return false;
    }
    assertEntryMatches(entry, record);
    entry.path = record.quarantine_path;
    if (record.kind === 'symlink') {
      const target = path.resolve(quarantinePath, record.resolved_link_path);
      const linkParent = path.dirname(path.resolve(quarantinePath, record.quarantine_path));
      entry.linkpath = path.relative(linkParent, target).replaceAll(path.sep, '/');
    }
    return true;
  };
}

function assertHardlinkEntry(entry, record) {
  if (entry.type !== 'Link' || memberSize(entry, record.path) !== 0 ||
      validateMode(entry) !== record.mode || entry.linkpath !== record.hardlink_wire_target) {
    admissionFail('ADMISSION_RACE', `archive hardlink changed after admission: ${recordName(record)}`);
  }
}

function recordName(record) {
  return record.member ?? record.path;
}

function assertEntryMatches(entry, record) {
  if (memberKind(entry, record.path) !== record.kind || memberSize(entry, record.path) !== record.size) {
    admissionFail('ADMISSION_RACE', `archive member changed after admission: ${recordName(record)}`);
  }
  if (validateMode(entry) !== record.mode) admissionFail('ADMISSION_RACE', `archive mode changed after admission: ${recordName(record)}`);
  if (record.kind === 'symlink' && entry.linkpath !== record.link_target) {
    admissionFail('ADMISSION_RACE', `archive link changed after admission: ${recordName(record)}`);
  }
}

function privatePath(root, relative, label) {
  const target = path.resolve(root, relative);
  if (target !== root && !target.startsWith(`${root}${path.sep}`)) {
    throw new ArchiveAdmissionError('ADMISSION_RACE', `${label} escaped quarantine`);
  }
  return target;
}

function privateRealPath(root, target, label) {
  const relative = path.relative(root, target);
  if (relative.startsWith('..') || path.isAbsolute(relative)) {
    throw new ArchiveAdmissionError('ADMISSION_RACE', `${label} escaped quarantine`);
  }
  let current = root;
  for (const component of relative.split(path.sep).filter(Boolean)) {
    current = path.join(current, component);
    let stat;
    try { stat = lstatSync(current); }
    catch (error) { throw new ArchiveAdmissionError('ADMISSION_RACE', `${label} is unavailable: ${error.message}`); }
    if (stat.isSymbolicLink()) throw new ArchiveAdmissionError('ADMISSION_RACE', `${label} has a symlink ancestor`);
  }
  let resolved;
  try { resolved = realpathSync(target); }
  catch (error) { throw new ArchiveAdmissionError('ADMISSION_RACE', `${label} is unavailable: ${error.message}`); }
  if (resolved !== root && !resolved.startsWith(`${root}${path.sep}`)) {
    throw new ArchiveAdmissionError('ADMISSION_RACE', `${label} escaped quarantine`);
  }
  return resolved;
}

function copyHardlink(root, record) {
  const source = privatePath(root, record.hardlink_source_quarantine_path, 'hardlink source');
  const destination = privatePath(root, record.quarantine_path, 'hardlink destination');
  privateRealPath(root, source, 'hardlink source');
  privateRealPath(root, path.dirname(destination), 'hardlink destination parent');
  copyPrivateRegular(source, destination, record);
  privateRealPath(root, destination, 'hardlink destination');
}

export async function materializeHardlinks(quarantinePath, entries) {
  let root;
  try { root = realpathSync(path.resolve(quarantinePath)); }
  catch (error) { throw new ArchiveAdmissionError('ADMISSION_RACE', `quarantine is unavailable: ${error.message}`); }
  for (const record of entries) {
    if (record.hardlink_target) await copyHardlink(root, record);
  }
}
