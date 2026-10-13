import assert from 'node:assert/strict';
import fs from 'node:fs';
import { inventoryFoundation } from './foundation-python-inventory.mjs';
import { qualifiedFoundationPython } from './foundation-python-verifier.mjs';

// Run in an isolated Linux container as root, never on a runner foundation tree.
if (process.platform !== 'linux' || process.getuid() !== 0)
  throw new Error('foundation proof requires isolated Linux root fixture');
const root = fs.mkdtempSync('/foundation-proof-');
const hostile = fs.mkdtempSync('/foundation-hostile-');
try {
  fs.chmodSync(root, 0o755);
  fs.chmodSync(hostile, 0o777);
  const filename = `${root}/struct.py`;
  fs.writeFileSync(filename, 'canonical', { mode: 0o644 });
  const roots = [root, `${root}/future/import.zip`];
  const original = inventoryFoundation(roots);
  assert.deepEqual(inventoryFoundation(roots), original);
  fs.writeFileSync(filename, 'changed');
  assert.notDeepEqual(inventoryFoundation(roots), original);
  fs.writeFileSync(filename, 'canonical');
  fs.writeFileSync(`${root}/extra.pyc`, 'injected', { mode: 0o644 });
  assert.notDeepEqual(inventoryFoundation(roots), original);
  fs.unlinkSync(`${root}/extra.pyc`);
  fs.mkdirSync(`${root}/future`, { mode: 0o755 });
  assert.notDeepEqual(inventoryFoundation(roots), original);
  fs.symlinkSync(filename, `${root}/link`);
  assert.equal(inventoryFoundation(roots).find(entry =>
    entry.path === `${root}/link`).kind, 'symlink');
  fs.writeFileSync(`${hostile}/rootowned.py`, 'injected', { mode: 0o644 });
  fs.symlinkSync(`${hostile}/rootowned.py`, `${root}/escape`);
  assert.throws(() => inventoryFoundation(roots), /mutable/);
  assert.deepEqual(qualifiedFoundationPython(),
    { available: false, reason: 'unqualified-foundation-image' });
  process.stdout.write('7 foundation security fixture probes passed; no hosted qualification.\n');
} finally {
  fs.rmSync(root, { recursive: true, force: true });
  fs.rmSync(hostile, { recursive: true, force: true });
}
