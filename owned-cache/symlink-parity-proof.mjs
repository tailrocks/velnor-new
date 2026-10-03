import { createHash } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  createReadStream,
  linkSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync
} from 'node:fs';
import { execFileSync } from 'node:child_process';
import os from 'node:os';
import path from 'node:path';
import { createGunzip, gunzipSync, gzipSync } from 'node:zlib';
import { Parser } from 'tar/parse';
import { admitArchive, extractAdmittedArchive } from './archive-admission.mjs';

const TAR_ENV = Object.freeze({
  PATH: '/usr/bin:/bin:/usr/sbin:/sbin', LANG: 'C', LC_ALL: 'C',
  COPYFILE_DISABLE: '1', TAR_OPTIONS: '', GZIP: ''
});
const SOURCE_LIMIT = 256 * 1024 * 1024;

function requireCondition(condition, message) {
  if (!condition) throw new Error(message);
}

function tarFlags() {
  return process.platform === 'darwin'
    ? ['--no-xattrs', '--no-acls', '--no-fflags', '--no-mac-metadata']
    : ['--no-xattrs', '--no-acls'];
}

function makeArchive(archive, workspace, members) {
  execFileSync('/usr/bin/tar', [
    '-czf', archive, '--format=ustar', ...tarFlags(), '-C', workspace, ...members
  ], { env: TAR_ENV, stdio: 'ignore' });
}

async function rawArchiveEntries(archive) {
  const entries = [];
  const parser = new Parser({
    strict: true,
    onReadEntry(entry) {
      entries.push({ path: entry.path, type: entry.type, size: entry.size,
        mode: entry.mode, linkpath: entry.linkpath ?? null });
      entry.resume();
    }
  });
  return new Promise((resolve, reject) => {
    const source = createReadStream(archive);
    const decoder = createGunzip();
    const fail = error => { source.destroy(); decoder.destroy(); parser.destroy(); reject(error); };
    source.on('error', fail); decoder.on('error', fail); parser.on('error', fail);
    parser.on('end', () => resolve(entries));
    source.pipe(decoder).pipe(parser);
  });
}

function makeQuarantine(root, name) {
  const quarantine = path.join(root, name);
  mkdirSync(quarantine, { mode: 0o700 });
  chmodSync(quarantine, 0o700);
  return quarantine;
}

function quarantineMember(quarantine, entry) {
  return path.join(quarantine, entry.quarantine_path);
}

function observedEntries(quarantine, manifest) {
  return manifest.entries.map(entry => {
    const filename = quarantineMember(quarantine, entry);
    const observed = { member: entry.member, kind: entry.kind, mode: lstatSync(filename).mode & 0o7777 };
    if (entry.kind === 'symlink') observed.link_target = readlinkSync(filename);
    if (entry.hardlink_target) {
      const source = path.join(quarantine, entry.hardlink_source_quarantine_path);
      observed.hardlink_target = entry.hardlink_target;
      observed.independent_inode = lstatSync(filename).ino !== lstatSync(source).ino;
    }
    return observed;
  });
}

async function acceptAndRelocate(root, workspace, roots, archive, label) {
  const raw = await rawArchiveEntries(archive);
  const manifest = await admitArchive(archive, 'gzip', roots, { workspace });
  const first = makeQuarantine(root, `${label}-first`);
  const relocated = makeQuarantine(root, `${label}-relocated`);
  const firstManifest = await extractAdmittedArchive(archive, manifest, first);
  const relocatedManifest = await extractAdmittedArchive(archive, manifest, relocated);
  return {
    label,
    raw_members: raw,
    ordered_roots: manifest.ordered_roots,
    entries: observedEntries(first, firstManifest),
    relocated_entries: observedEntries(relocated, relocatedManifest),
    quarantine_paths: [first, relocated]
  };
}

function makeParityFixture(root) {
  const workspace = path.join(root, 'parity-workspace');
  const fileRoot = path.join(workspace, 'file-root');
  const directoryRoot = path.join(workspace, 'directory-root');
  fsMkdir(path.dirname(fileRoot));
  writeFileSync(fileRoot, 'direct root file\n');
  mkdirSync(directoryRoot, { recursive: true });
  writeFileSync(path.join(directoryRoot, 'gh'), 'gh payload\n');
  symlinkSync('gh', path.join(directoryRoot, 'link-gh'));
  symlinkSync('./gh', path.join(directoryRoot, 'link-dot-gh'));
  writeFileSync(path.join(directoryRoot, 'hard-target'), 'hard payload\n');
  linkSync(path.join(directoryRoot, 'hard-target'), path.join(directoryRoot, 'hard-alias'));
  const archive = path.join(root, 'parity.tgz');
  makeArchive(archive, workspace, ['file-root', 'directory-root']);
  return { workspace, roots: [fileRoot, directoryRoot], archive };
}

function fsMkdir(directory) {
  mkdirSync(directory, { recursive: true });
}

function makeSymlinkFixture(root, name, target, extra) {
  const workspace = path.join(root, `${name}-workspace`);
  const payload = path.join(workspace, 'payload');
  mkdirSync(payload, { recursive: true });
  if (extra) extra(payload);
  symlinkSync(target, path.join(payload, 'link'));
  const archive = path.join(root, `${name}.tgz`);
  makeArchive(archive, workspace, ['payload']);
  return { workspace, payload, archive };
}

async function rejectAdmission(fixture) {
  try {
    await admitArchive(fixture.archive, 'gzip', [fixture.payload], { workspace: fixture.workspace });
    return { accepted: true, code: null };
  }
  catch (error) {
    return { accepted: false, code: error.code ?? 'UNKNOWN' };
  }
}

function tarField(header, offset, length) {
  return header.subarray(offset, offset + length).toString('utf8').replace(/\0.*$/, '');
}

function writeTarField(header, offset, length, value) {
  const bytes = Buffer.from(`${value}\0`);
  requireCondition(bytes.length <= length, 'tar field too long');
  header.fill(0, offset, offset + length);
  bytes.copy(header, offset);
}

function updateChecksum(header) {
  header.fill(0x20, 148, 156);
  let sum = 0;
  for (const byte of header) sum += byte;
  header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148, 8, 'ascii');
}

function mutateHardlinkHeaders(archive, mutate) {
  const decoded = gunzipSync(readFileSync(archive));
  const links = [];
  for (let offset = 0; offset + 512 <= decoded.length; offset += 512) {
    const header = decoded.subarray(offset, offset + 512);
    if (header.every(byte => byte === 0)) break;
    if (header[156] === 0x31) links.push({ header, path: tarField(header, 0, 100), linkpath: tarField(header, 157, 100) });
  }
  requireCondition(links.length > 0, 'hardlink fixture has no Link member');
  mutate(links);
  for (const link of links) updateChecksum(link.header);
  writeFileSync(archive, gzipSync(decoded));
  return links.map(link => ({ path: tarField(link.header, 0, 100), linkpath: tarField(link.header, 157, 100) }));
}

function makeHardlinkFixture(root, name, count = 2) {
  const workspace = path.join(root, `${name}-workspace`);
  const payload = path.join(workspace, 'payload');
  mkdirSync(payload, { recursive: true });
  writeFileSync(path.join(payload, 'base'), `${name} bytes\n`);
  for (let index = 1; index < count; index += 1) {
    linkSync(path.join(payload, 'base'), path.join(payload, `alias-${index}`));
  }
  const archive = path.join(root, `${name}.tgz`);
  makeArchive(archive, workspace, ['payload']);
  return { workspace, payload, archive };
}

async function hardlinkRejects(root) {
  const changed = makeHardlinkFixture(root, 'hardlink-changed-mode');
  const changedLinks = mutateHardlinkHeaders(changed.archive, links => {
    const mode = (0o755).toString(8).padStart(7, '0');
    writeTarField(links[0].header, 100, 8, mode);
  });
  const chain = makeHardlinkFixture(root, 'hardlink-chain', 3);
  const chainLinks = mutateHardlinkHeaders(chain.archive, links => {
    writeTarField(links[1].header, 157, 100, links[0].path);
  });
  const outside = makeHardlinkFixture(root, 'hardlink-outside');
  mutateHardlinkHeaders(outside.archive, links => {
    writeTarField(links[0].header, 157, 100, '../outside');
  });
  return {
    changed_mode: { raw_links: changedLinks, result: await rejectAdmission(changed) },
    chain: { raw_links: chainLinks, result: await rejectAdmission(chain) },
    outside: { result: await rejectAdmission(outside) }
  };
}

function candidatePaths() {
  const home = process.env.HOME || '';
  return [
    { label: 'rust', path: path.join(home, '.cargo', 'bin', 'rustup') },
    { label: 'node', path: process.execPath },
    { label: 'node-mise', path: path.join(home, '.local/share/mise/installs/node/24.20.0/bin/node') },
    { label: 'python', path: '/usr/bin/python3' },
    { label: 'python-local', path: '/usr/local/bin/python3' },
    { label: 'python-homebrew', path: '/opt/homebrew/bin/python3' }
  ];
}

function sha256File(filename) {
  return createHash('sha256').update(readFileSync(filename)).digest('hex');
}

async function actualSourceGroup(root, candidate) {
  let stat;
  let real;
  try {
    stat = lstatSync(candidate.path);
    real = realpathSync(candidate.path);
    stat = lstatSync(real);
  }
  catch { return { label: candidate.label, path: candidate.path, status: 'absent' }; }
  const result = { label: candidate.label, path: candidate.path, realpath: real, host_nlink: stat.nlink, bytes: stat.size };
  if (!stat.isFile()) return { ...result, status: 'non-regular' };
  if (stat.size > SOURCE_LIMIT) return { ...result, status: 'skipped-source-too-large' };
  const fixture = path.join(root, `actual-${candidate.label}`);
  mkdirSync(fixture);
  const workspace = path.join(fixture, 'workspace');
  const payload = path.join(workspace, 'payload');
  mkdirSync(payload, { recursive: true });
  const source = path.join(payload, 'source');
  const alias = path.join(payload, 'alias');
  copyFileSync(real, source);
  linkSync(source, alias);
  const archive = path.join(fixture, 'archive.tgz');
  makeArchive(archive, workspace, ['payload']);
  const manifest = await admitArchive(archive, 'gzip', [payload], { workspace });
  const quarantine = makeQuarantine(fixture, 'quarantine');
  await extractAdmittedArchive(archive, manifest, quarantine);
  const sourceStat = lstatSync(path.join(quarantine, 'roots', '0', 'source'));
  const aliasStat = lstatSync(path.join(quarantine, 'roots', '0', 'alias'));
  requireCondition(sourceStat.isFile() && aliasStat.isFile() && sourceStat.ino !== aliasStat.ino, 'actual source group was not flattened');
  return { ...result, status: 'synthetic-source-bytes-positive', source_sha256: sha256File(real), independent_inodes: true };
}

function sourceHashes() {
  return Object.fromEntries(['archive-admission.mjs', 'archive-hardlinks.mjs', 'archive-private-copy.mjs', 'archive-admission-policy.mjs', 'metadata_container.py', 'symlink-parity-proof.mjs']
    .map(file => [file, sha256File(new URL(file, import.meta.url))]));
}

const root = mkdtempSync(path.join(os.tmpdir(), 'velnor-symlink-parity-'));
try {
  const parity = makeParityFixture(root);
  const parityResult = await acceptAndRelocate(root, parity.workspace, parity.roots, parity.archive, 'parity');
  const absolute = makeSymlinkFixture(root, 'absolute-link', '/etc/passwd', payload => writeFileSync(path.join(payload, 'data'), 'x'));
  const escape = makeSymlinkFixture(root, 'external-escape', '../../outside', payload => writeFileSync(path.join(payload, 'data'), 'x'));
  const chain = makeSymlinkFixture(root, 'symlink-chain', 'first', payload => {
    writeFileSync(path.join(payload, 'target'), 'x');
    symlinkSync('target', path.join(payload, 'first'));
  });
  const hardlinkResults = await hardlinkRejects(root);
  const actualGroups = [];
  for (const candidate of candidatePaths()) {
    if (actualGroups.some(result => result.path === candidate.path)) continue;
    actualGroups.push(await actualSourceGroup(root, candidate));
  }
  console.log(JSON.stringify({
    proof_source: realpathSync(new URL('symlink-parity-proof.mjs', import.meta.url)),
    fixture_root: root,
    source_hashes: sourceHashes(),
    parity: parityResult,
    rejects: {
      absolute: await rejectAdmission(absolute),
      external_escape: await rejectAdmission(escape),
      symlink_chain: await rejectAdmission(chain)
    },
    hardlinks: hardlinkResults,
    actual_groups: actualGroups,
    foundation_qualification_claimed: false
  }, null, 2));
} finally {
  rmSync(root, { recursive: true, force: true });
}
