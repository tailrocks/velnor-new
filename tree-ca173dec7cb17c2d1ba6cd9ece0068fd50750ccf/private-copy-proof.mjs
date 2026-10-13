import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { copyPrivateRegular } from './archive-private-copy.mjs';

if (process.versions.node.split('.')[0] !== '24') throw Error('Node 24 required');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'velnor-private-copy-proof-'));
const outside = path.join(root, 'outside');
fs.writeFileSync(outside, 'outside remains unchanged\n');
const source = path.join(root, 'source');
fs.writeFileSync(source, 'portable bytes\n', { mode: 0o644 });
const expected = {
  size: fs.statSync(source).size, mode: 0o644,
  content_sha256: createHash('sha256').update(fs.readFileSync(source)).digest('hex')
};

function reject(label, input, destination) {
  try { copyPrivateRegular(input, destination, expected); }
  catch (error) {
    if (error.code !== 'ADMISSION_RACE') throw error;
    return { label, rejected: true, code: error.code };
  }
  throw Error(`${label} unexpectedly accepted`);
}

fs.symlinkSync(outside, path.join(root, 'source-link'));
const results = [reject('source-symlink', path.join(root, 'source-link'), path.join(root, 'symlink-output'))];
if (fs.existsSync(path.join(root, 'symlink-output'))) throw Error('source symlink created destination');
fs.symlinkSync(outside, path.join(root, 'destination-link'));
results.push(reject('destination-symlink', source, path.join(root, 'destination-link')));
const fifo = path.join(root, 'fifo');
const fifoResult = spawnSync('/usr/bin/mkfifo', [fifo], { encoding: 'utf8' });
if (fifoResult.status !== 0) throw Error('FIFO fixture creation failed');
results.push(reject('source-fifo', fifo, path.join(root, 'fifo-output')));
if (fs.existsSync(path.join(root, 'fifo-output'))) throw Error('FIFO created destination');

async function changedInode() {
  const large = path.join(root, 'large');
  const destination = path.join(root, 'large-output');
  const replacement = path.join(root, 'replacement');
  const block = Buffer.alloc(1024 * 1024, 0x61);
  const hash = createHash('sha256');
  const fd = fs.openSync(large, 'wx', 0o644);
  try {
    for (let index = 0; index < 256; index++) { fs.writeSync(fd, block); hash.update(block); }
  }
  finally { fs.closeSync(fd); }
  fs.writeFileSync(replacement, 'different inode\n');
  const watcher = `
    const fs = require('node:fs'), path = require('node:path');
    const [root, source, replacement, destination] = process.argv.slice(1);
    const timeout = setTimeout(() => process.exit(2), 10000);
    const watch = fs.watch(root, (_event, name) => {
      if (String(name) !== path.basename(destination) || !fs.existsSync(destination)) return;
      fs.renameSync(replacement, source); watch.close(); clearTimeout(timeout);
      process.stdout.write('replaced\\n');
    });
    process.stdout.write('ready\\n');
  `;
  const worker = spawn(process.execPath, ['-e', watcher, root, large, replacement, destination], {
    env: { LANG: 'C', LC_ALL: 'C' }, stdio: ['ignore', 'pipe', 'pipe']
  });
  let stdout = '';
  const finished = new Promise((resolve, reject) => {
    worker.once('error', reject);
    worker.once('exit', code => code === 0 ? resolve() : reject(Error('replacement worker failed')));
  });
  await new Promise((resolve, reject) => {
    worker.once('error', reject);
    worker.stdout.on('data', chunk => { stdout += chunk.toString(); if (stdout.includes('ready\n')) resolve(); });
  });
  const record = { size: block.length * 256, mode: 0o644, content_sha256: hash.digest('hex') };
  let rejected = false;
  try { copyPrivateRegular(large, destination, record); }
  catch (error) { if (error.code !== 'ADMISSION_RACE') throw error; rejected = true; }
  await finished;
  if (!rejected || !stdout.includes('replaced\n')) throw Error('actual inode replacement did not reject');
  return { label: 'changed-source-inode', rejected, code: 'ADMISSION_RACE', actualReplacement: true };
}

results.push(await changedInode());
if (fs.readFileSync(outside, 'utf8') !== 'outside remains unchanged\n') throw Error('outside file changed');
console.log(JSON.stringify({ node: process.versions.node, root, results, outsideUnchanged: true,
  arbitrarySameUidAncestorRaceProof: false, qualified: false }, null, 2));
