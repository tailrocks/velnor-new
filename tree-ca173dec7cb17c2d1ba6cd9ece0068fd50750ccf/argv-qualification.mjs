import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const sourceDir = path.dirname(fileURLToPath(import.meta.url));
const staged = fs.mkdtempSync(path.join(sourceDir, '.argv-qualification-'));
for (const file of [
  'tar.js',
  'cacheUtils.js',
  'archive-admission.mjs',
  'archive-admission-errors.mjs',
  'archive-admission-policy.mjs',
  'archive-codec.mjs',
  'quarantine-path.mjs'
]) {
  fs.copyFileSync(path.join(sourceDir, file), path.join(staged, file));
}
fs.copyFileSync(
  path.join(sourceDir, '..', 'node_modules', '@actions', 'cache', 'lib', 'internal', 'constants.js'),
  path.join(staged, 'constants.js'),
);
const { createTar, extractTar } = await import(pathToFileURL(path.join(staged, 'tar.js')));

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'velnor-cache argv '));
const workspace = path.join(root, 'workspace with spaces');
const archiveFolder = path.join(root, 'archive with spaces');
fs.mkdirSync(path.join(workspace, 'payload'), { recursive: true });
fs.mkdirSync(archiveFolder);
fs.writeFileSync(path.join(workspace, 'payload', 'data'), 'owned payload\n');

process.env.GITHUB_WORKSPACE = workspace;
await createTar(archiveFolder, ['payload'], 'gzip');
const archivePath = path.join(archiveFolder, 'cache.tgz');
const quarantinePath = path.join(root, 'velnor', 'cache-staging', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
process.env.RUNNER_TEMP = root;
await extractTar(archivePath, 'gzip', {
  roots: [path.join(workspace, 'payload')],
  quarantinePath
});

const restored = path.join(quarantinePath, 'roots', '0', 'data');
if (fs.readFileSync(restored, 'utf8') !== 'owned payload\n') {
  throw new Error('space path roundtrip failed');
}
console.log(JSON.stringify({ root, archivePath, restored }));
fs.rmSync(staged, { recursive: true, force: true });
