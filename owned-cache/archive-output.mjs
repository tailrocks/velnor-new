import {
  closeSync, constants, fstatSync, lstatSync, openSync, realpathSync, statSync
} from 'node:fs';
import path from 'node:path';
import { requestedRoots } from './archive-admission-policy.mjs';
import { ArchiveConfigurationError } from './quarantine-path.mjs';

function fail() {
  throw new ArchiveConfigurationError('ARCHIVE_OUTPUT_OVERLAP',
    'archive control files must be outside selected source roots');
}

function same(left, right) {
  return left.dev === right.dev && left.ino === right.ino;
}

function hold(actual, directory) {
  if (!constants.O_NOFOLLOW || !constants.O_NONBLOCK || !constants.O_DIRECTORY) fail();
  const flags = constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK |
    (directory ? constants.O_DIRECTORY : 0);
  const fd = openSync(actual, flags);
  try {
    const identity = fstatSync(fd, { bigint: true });
    if (directory ? !identity.isDirectory() : !identity.isFile()) fail();
    return { fd, actual, identity };
  } catch (error) {
    closeSync(fd);
    throw error;
  }
}

function checkHeld(held) {
  for (const entry of held) {
    if (!same(entry.identity, fstatSync(entry.fd, { bigint: true })) ||
        !same(entry.identity, lstatSync(entry.actual, { bigint: true }))) fail();
  }
}

// Fresh quiescent job namespace required: Node has no dirfd-relative openat.
// Actual directory identities, rather than lexical prefixes, prove containment.
export function validateArchiveOutput(folder, sources, workspace, controlNames) {
  const roots = requestedRoots(sources, workspace);
  const held = [];
  try {
    const selected = roots.map(root => {
      const actual = realpathSync.native(root.original_root);
      const kind = statSync(actual, { bigint: true });
      const entry = hold(actual, kind.isDirectory());
      held.push(entry);
      return entry;
    });
    const actualFolder = realpathSync.native(folder);
    let ancestor = actualFolder;
    while (true) {
      const entry = hold(ancestor, true);
      held.push(entry);
      if (selected.some(root => same(root.identity, entry.identity))) fail();
      const parent = path.dirname(ancestor);
      if (parent === ancestor) break;
      ancestor = parent;
    }
    for (const name of controlNames) {
      const candidate = path.join(actualFolder, name);
      let existing;
      try { existing = lstatSync(candidate, { bigint: true }); }
      catch (error) { if (error.code === 'ENOENT') continue; throw error; }
      // Creator owns fresh regular control files; aliases are never overwritten.
      if (!existing.isFile() || existing.nlink !== 1n) fail();
      const entry = hold(candidate, false);
      held.push(entry);
      if (selected.some(root => same(root.identity, entry.identity))) fail();
    }
    checkHeld(held);
  } finally {
    for (const entry of held.reverse()) closeSync(entry.fd);
  }
}
