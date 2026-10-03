import { copyFileSync, existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
const upstream={"tar.js": "7fdb340eb93cedfb97e27c1a9599969cbbe3512ba63baf940da0a6ed5c92e029", "cacheUtils.js": "ba1b2de90f5abc0475065f0eb3017db727dd548b53cd6e4053fd70bf223ab75a"};
for (const file of Object.keys(upstream)) {
  const source=new URL(file, import.meta.url);
  const target=new URL('../node_modules/@actions/cache/lib/internal/' + file, import.meta.url);
  const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
  const current=hash(readFileSync(target));
  if (current!==upstream[file] && current!==hash(readFileSync(source))) throw Error('Unexpected toolkit source: '+file);
  copyFileSync(source,target);
}

const helpers = [
  'archive-admission.mjs', 'archive-admission.d.mts',
  'archive-admission-errors.mjs', 'archive-admission-policy.mjs', 'archive-hardlinks.mjs',
  'archive-private-copy.mjs',
  'archive-output.mjs',
  'archive-codec.mjs', 'quarantine-path.mjs', 'quarantine-path.d.mts',
  'appledouble-admission.mjs', 'appledouble-admission.d.mts', 'metadata_container.py',
  'foundation-python-verifier.mjs', 'foundation-python-verifier.d.mts',
  'foundation-python-inventory.mjs'
];
for (const file of helpers) {
  const source = new URL(file, import.meta.url);
  const target = new URL('../node_modules/@actions/cache/lib/internal/' + file, import.meta.url);
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  if (existsSync(target) && hash(readFileSync(target)) !== hash(readFileSync(source))) {
    throw Error('Unexpected owned helper source: ' + file);
  }
  copyFileSync(source, target);
}

const directory=new URL('.',import.meta.url);
const patches=readdirSync(directory).filter(name=>name.endsWith('-patch.json'))
  .map(name=>JSON.parse(readFileSync(new URL(name,directory),'utf8')));
const baselines = JSON.parse(readFileSync(new URL('upstream-patch-sources.json', directory), 'utf8'));
for (const path of new Set(patches.map(patch => patch.path))) {
  const target = new URL('../' + path, import.meta.url);
  const matching = patches.filter(patch => patch.path === path);
  const baseline = baselines[path];
  if (!baseline) throw Error('Missing pinned patch source: ' + path);
  const bytes = Buffer.from(baseline.sourceBase64, 'base64');
  const digest = value => createHash('sha256').update(value).digest('hex');
  if (digest(bytes) !== baseline.sha256 || matching.some(patch => patch.upstreamSha256 !== baseline.sha256)) {
    throw Error('Unexpected pinned patch source: ' + path);
  }
  let source = bytes.toString('utf8');
  for (const replacement of matching.flatMap(patch => patch.replacements)) {
    if (source.split(replacement.from).length !== 2) throw Error('Nonunique patch: ' + path);
    source = source.replace(replacement.from, replacement.to);
  }
  const current = digest(readFileSync(target));
  if (current === digest(source)) continue;
  if (current !== baseline.sha256) throw Error('Unexpected patch source: ' + path);
  writeFileSync(target, source);
}
