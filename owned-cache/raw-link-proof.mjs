import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { gunzipSync } from 'node:zlib';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { createHash } from 'node:crypto';
import { admitArchive, extractAdmittedArchive } from './archive-admission.mjs';
import { createArchiveEncoder } from './archive-codec.mjs';

if (process.versions.node.split('.')[0] !== '24') throw Error('Node24 required');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'velnor-raw-link-proof-'));
const cases = [
  ['symlink-before-parent', 'dlink/../file', false],
  ['file-before-parent', 'file/../file', false],
  ['missing-before-parent', 'missing/../file', false],
  ['file-trailing-slash', 'file/', false],
  ['crossroot-missing-before-parent', '../b/missing/../file', false],
  ['real-directory-before-parent', 'directory/../file', true],
  ['directory-dot-before-parent', 'directory/./../file', true],
  ['direct-crossroot-file', '../b/file', true],
  ['direct-crossroot-directory', '../b', true],
  ['direct-dot-file', './file', true],
  ['directory-trailing-slash', 'directory/', true]
];

function fixture(label, target) {
  const workspace = path.join(root, label);
  const a = path.join(workspace, 'a'), b = path.join(workspace, 'b');
  fs.mkdirSync(path.join(a, 'directory'), { recursive: true });
  fs.mkdirSync(b);
  fs.writeFileSync(path.join(a, 'file'), 'admitted a bytes\n');
  fs.writeFileSync(path.join(b, 'file'), 'admitted b bytes\n');
  fs.writeFileSync(path.join(workspace, 'file'), 'unadmitted fixture bytes\n');
  fs.symlinkSync('../b', path.join(a, 'dlink'));
  fs.symlinkSync(target, path.join(a, 'link'));
  const archive = path.join(root, `${label}.tgz`);
  const args = ['--format=ustar', '-czf', archive, '-C', workspace, '--no-xattrs', '--no-acls'];
  if (process.platform === 'darwin') args.push('--no-fflags', '--no-mac-metadata');
  args.push('a', 'b');
  const result = spawnSync('/usr/bin/tar', args, { env: { LANG: 'C', LC_ALL: 'C', COPYFILE_DISABLE: '1' }, encoding: 'utf8' });
  if (result.status !== 0) throw Error('fixture tar creation failed');
  let actual;
  try { actual = fs.realpathSync.native(path.join(a, 'link')); }
  catch (error) { actual = error.code; }
  const originalLink = path.join(a, 'link');
  let actualOpen;
  try {
    const stat = fs.statSync(originalLink);
    actualOpen = stat.isDirectory() ? 'directory' : createHash('sha256').update(fs.readFileSync(originalLink)).digest('hex');
  }
  catch (error) { actualOpen = error.code; }
  return { workspace, roots: [a, b], archive, actual, actualOpen, originalLink };
}

async function check(label, target, accepted, method) {
  const value = fixture(`${label}-${method}`, target);
  let archive = value.archive;
  if (method === 'zstd-without-long') {
    archive = `${archive}.tzst`;
    await pipeline(Readable.from([gunzipSync(fs.readFileSync(value.archive))]),
      createArchiveEncoder(method), fs.createWriteStream(archive, { flags: 'wx', mode: 0o600 }));
  }
  const stage = path.join(root, `${label}-${method}-stage`);
  let manifest;
  try { manifest = await admitArchive(archive, method, value.roots, { workspace: value.workspace }); }
  catch (error) {
    if (accepted || error.code !== 'ADMISSION_LINK' || fs.existsSync(stage)) throw error;
    return { label, target, method, accepted: false, code: error.code, actualOriginalTarget: value.actual, actualOriginalOpen: value.actualOpen, writes: false };
  }
  if (!accepted) throw Error(`${label} incorrectly accepted`);
  fs.mkdirSync(stage, { mode: 0o700 });
  await extractAdmittedArchive(archive, manifest, stage);
  const observed = path.join(stage, 'roots/0/link');
  const stat = fs.statSync(observed);
  if (stat.isDirectory() !== fs.statSync(value.originalLink).isDirectory()) throw Error('staged target kind differs');
  if (stat.isFile() && !fs.readFileSync(observed).equals(fs.readFileSync(value.originalLink))) throw Error('staged target bytes differ');
  return { label, target, method, accepted: true, actualOriginalTarget: value.actual,
    stagedRawTarget: fs.readlinkSync(observed), stagedKind: stat.isDirectory() ? 'directory' : 'file' };
}

const results = [];
for (const method of ['gzip', 'zstd-without-long']) {
  for (const [label, target, accepted] of cases) results.push(await check(label, target, accepted, method));
}
const sources = Object.fromEntries(['archive-admission-policy.mjs', 'archive-admission.mjs', 'raw-link-proof.mjs'].map(name =>
  [name, createHash('sha256').update(fs.readFileSync(new URL(name, import.meta.url))).digest('hex')]));
console.log(JSON.stringify({ node: process.versions.node, root, results, sources, qualified: false }, null, 2));
