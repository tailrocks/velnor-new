import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

if (process.versions.node.split('.')[0] !== '24') throw Error('Node 24 required');
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const npm = path.resolve(path.dirname(process.execPath), '../lib/node_modules/npm/bin/npm-cli.js');
const excluded = new Set(['.git', 'node_modules', 'dist', 'lib', 'coverage', '.cache', '.eslintcache']);
const rebuild = fs.mkdtempSync(path.join(os.tmpdir(), 'velnor-cache-rebuild-'));
const logs = fs.mkdtempSync(path.join(os.tmpdir(), 'velnor-cache-build-logs-'));
let commandIndex = 0;
fs.cpSync(root, rebuild, {
  recursive: true,
  filter: source => !path.relative(root, source).split(path.sep).some(part => excluded.has(part) || part.startsWith('.argv-'))
});

function run(directory, argv) {
  const result = spawnSync(process.execPath, argv, {
    cwd: directory,
    env: { ...process.env, PATH: `${path.dirname(process.execPath)}:/usr/bin:/bin:/usr/sbin:/sbin` },
    encoding: 'utf8', maxBuffer: 16 * 1024 * 1024
  });
  fs.writeFileSync(path.join(logs, `${String(commandIndex++).padStart(3, '0')}.json`), JSON.stringify({
    directory, executable: process.execPath, argv, status: result.status,
    stdout: result.stdout, stderr: result.stderr, error: result.error?.message
  }, null, 2));
  if (result.status !== 0) throw Error(`Build command failed: ${argv[0]}\n${result.stderr}\n${result.stdout}`);
}

function assets(directory) {
  const values = {};
  for (const role of ['restore', 'save', 'restore-only', 'save-only']) {
    for (const file of fs.readdirSync(path.join(directory, 'dist', role)).sort()) {
      const relative = `dist/${role}/${file}`;
      const bytes = fs.readFileSync(path.join(directory, relative));
      values[relative] = crypto.createHash('sha256').update(bytes).digest('hex');
    }
  }
  return values;
}

function sourceFiles(directory, prefix = '') {
  const values = {};
  for (const name of fs.readdirSync(path.join(directory, prefix)).sort()) {
    if (excluded.has(name) || name.startsWith('.argv-')) continue;
    const relative = prefix ? `${prefix}/${name}` : name;
    const absolute = path.join(directory, relative);
    const stat = fs.lstatSync(absolute);
    if (stat.isDirectory()) Object.assign(values, sourceFiles(directory, relative));
    else {
      if (!stat.isFile() && !stat.isSymbolicLink()) throw Error('Unsupported source type');
      const bytes = stat.isSymbolicLink() ? Buffer.from(fs.readlinkSync(absolute)) : fs.readFileSync(absolute);
      values[relative] = { type: stat.isSymbolicLink() ? 'symlink' : 'file', mode: stat.mode & 0o777, sha256: crypto.createHash('sha256').update(bytes).digest('hex') };
    }
  }
  return values;
}

function build(directory) {
  run(directory, [npm, 'ci', '--ignore-scripts', '--no-audit']);
  run(directory, ['owned-cache/apply.mjs']);
  run(directory, ['owned-cache/apply.mjs']);
  run(directory, ['node_modules/typescript/bin/tsc']);
  for (const [role, source] of [['restore', 'restore'], ['save', 'save'], ['restore-only', 'restoreOnly'], ['save-only', 'saveOnly']]) {
    run(directory, ['node_modules/@vercel/ncc/dist/ncc/cli.js', 'build', '-o', `dist/${role}`, `src/${source}.ts`]);
  }
  return assets(directory);
}

const postimage = sourceFiles(root);
if (JSON.stringify(postimage) !== JSON.stringify(sourceFiles(rebuild))) throw Error('Clean rebuild source copy differs');
const first = build(root);
const second = build(rebuild);
if (JSON.stringify(first) !== JSON.stringify(second)) throw Error('Different-directory build assets differ');
if (JSON.stringify(postimage) !== JSON.stringify(sourceFiles(root))) throw Error('Source changed during build');
console.log(JSON.stringify({ node: process.versions.node, root, rebuild, logs, qualified: false, reproducible: true, files: first, postimage }, null, 2));
