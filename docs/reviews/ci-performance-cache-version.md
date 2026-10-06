# Pinned cache hidden-version behavioral proof

Qualification: local pinned implementation execution; hosted restore/import and cold/warm/third-run performance remain separate gates.

Executed on 2026-10-03 with Node v26.10.0. Both actual bundled `getCacheVersion` function bodies and their bundled salt were extracted without modification and evaluated in Node VM. The supplied bindings were Node crypto and Linux platform; compression was `zstd`, cross-OS archive false. No rewritten hash implementation was tested.

Action source: `actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9`.

- [Restore bundle](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/dist/restore/index.js)
- [Save bundle](https://github.com/actions/cache/blob/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/dist/save/index.js)

Save bundle reused the existing audit copy. Restore bundle was absent and fetched from the exact source URL. Both whole-bundle SHA256 values are checked before execution. Extracted function bodies are byte identical. The path prefix below models a Linux fresh runner resolution of `${{ runner.temp }}/velnor`; hashes depend on the resolved prefix, so actual hosted paths must match between operations.

Canonical tool and source roots match `SnapshotLayer::roots()` in `crates/velnor-actions-workflow-renderer/src/cache_snapshot.rs` at audit time. Canonical restore/save versions agree for both payloads. Reversing path order changes the hidden version for both: ordered path equality is necessary.

## Reproduction

Save the script below as `/tmp/velnor-ci-runtime-audit/cache-version-canonical.js`. Obtain the exact bundles:

```sh
rtk proxy curl --fail --location https://raw.githubusercontent.com/actions/cache/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/dist/restore/index.js --output /tmp/velnor-ci-runtime-audit/cache-dist-restore.js
rtk proxy curl --fail --location https://raw.githubusercontent.com/actions/cache/55cc8345863c7cc4c66a329aec7e433d2d1c52a9/dist/save/index.js --output /tmp/velnor-ci-runtime-audit/cache-dist-save.js
rtk proxy node /tmp/velnor-ci-runtime-audit/cache-version-canonical.js
```

```js
const fs = require('node:fs');
const vm = require('node:vm');
const crypto = require('node:crypto');
const bundles = {
 restore: '/tmp/velnor-ci-runtime-audit/cache-dist-restore.js',
 save: '/tmp/velnor-ci-runtime-audit/cache-dist-save.js',
};
const expected = {
 restore: '79a05f72974c12e3796bf09c897fce453a855f5f4a4abc980be6f671c545954d',
 save: '9193aa9dbe5025f2bf39802ae241470af35c5e5737fd4bec6e1d6dbaad153bb4',
};
const engines = {};
const evidence = { actionSha: '55cc8345863c7cc4c66a329aec7e433d2d1c52a9', platform: 'linux', compression: 'zstd', crossOs: false, bundles: {} };
for (const [name, path] of Object.entries(bundles)) {
 const bytes = fs.readFileSync(path);
 const digest = crypto.createHash('sha256').update(bytes).digest('hex');
 if (digest !== expected[name]) throw Error('Unexpected bundle: ' + name);
 const source = bytes.toString();
 const marker = 'function getCacheVersion(';
 const start = source.indexOf(marker);
 if (start < 0 || source.indexOf(marker, start + 1) >= 0) throw Error('Function not unique');
 let cursor = source.indexOf('{', start), depth = 1;
 const begin = cursor;
 while (depth > 0) {
  cursor++;
  if (source[cursor] === '{') depth++;
  if (source[cursor] === '}') depth--;
 }
 const fn = source.slice(start, cursor + 1);
 const salt = source.match(/const versionSalt = '[^']+';/)?.[0];
 if (!salt) throw Error('Missing bundled salt');
 const context = { external_crypto_namespaceObject: crypto, process: { platform: 'linux' } };
 vm.createContext(context);
 vm.runInContext(salt + '\n' + fn + '\nthis.engine = getCacheVersion;', context);
 engines[name] = context.engine;
 evidence.bundles[name] = { path, sha256: digest, functionSha256: crypto.createHash('sha256').update(fn).digest('hex'), salt };
}
const prefix = '/home/runner/work/_temp/velnor/';
const payloads = {
 tools: ['mise', 'rustup', 'cargo/bin', 'cargo/.crates.toml', 'cargo/.crates2.json'],
 sources: ['cargo/registry/index', 'cargo/registry/cache', 'cargo/git/db'],
};
evidence.payloads = {};
for (const [name, roots] of Object.entries(payloads)) {
 const paths = roots.map(root => prefix + root);
 const restore = engines.restore(paths, 'zstd', false);
 const save = engines.save(paths, 'zstd', false);
 const reordered = engines.save([...paths].reverse(), 'zstd', false);
 if (restore !== save) throw Error('Canonical payload mismatch');
 if (reordered === restore) throw Error('Order negative failed');
 evidence.payloads[name] = { paths, restore, save, reversedPathOrder: reordered, equality: restore === save, orderNegative: restore !== reordered };
}
console.log(JSON.stringify(evidence, null, 2));
```

## Raw result

```json
{
  "actionSha": "55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
  "platform": "linux",
  "compression": "zstd",
  "crossOs": false,
  "bundles": {
    "restore": {
      "path": "/tmp/velnor-ci-runtime-audit/cache-dist-restore.js",
      "sha256": "79a05f72974c12e3796bf09c897fce453a855f5f4a4abc980be6f671c545954d",
      "functionSha256": "ede2818ebfb8d331562081703b0ab5f6b1bb97e58d89aad4394f9504508863db",
      "salt": "const versionSalt = '1.0';"
    },
    "save": {
      "path": "/tmp/velnor-ci-runtime-audit/cache-dist-save.js",
      "sha256": "9193aa9dbe5025f2bf39802ae241470af35c5e5737fd4bec6e1d6dbaad153bb4",
      "functionSha256": "ede2818ebfb8d331562081703b0ab5f6b1bb97e58d89aad4394f9504508863db",
      "salt": "const versionSalt = '1.0';"
    }
  },
  "payloads": {
    "tools": {
      "paths": [
        "/home/runner/work/_temp/velnor/mise",
        "/home/runner/work/_temp/velnor/rustup",
        "/home/runner/work/_temp/velnor/cargo/bin",
        "/home/runner/work/_temp/velnor/cargo/.crates.toml",
        "/home/runner/work/_temp/velnor/cargo/.crates2.json"
      ],
      "restore": "42e8f0aa35d7e574bedd4612c0c0171401cb1b4cfde667406c8873cd83bd5139",
      "save": "42e8f0aa35d7e574bedd4612c0c0171401cb1b4cfde667406c8873cd83bd5139",
      "reversedPathOrder": "4ad057026d733f9061b7cde6db5d24558b595dfd3a2d342ee25463d24475f848",
      "equality": true,
      "orderNegative": true
    },
    "sources": {
      "paths": [
        "/home/runner/work/_temp/velnor/cargo/registry/index",
        "/home/runner/work/_temp/velnor/cargo/registry/cache",
        "/home/runner/work/_temp/velnor/cargo/git/db"
      ],
      "restore": "0735773537c44f1dc16e76b6e5f97a3771f398579b6c82bed073d80e1c870b11",
      "save": "0735773537c44f1dc16e76b6e5f97a3771f398579b6c82bed073d80e1c870b11",
      "reversedPathOrder": "4ce272abe68ab9e4140a3dd5cb226583afda5fbe996485e5712149df5597d670",
      "equality": true,
      "orderNegative": true
    }
  }
}
```

## macOS archive metadata negative qualification

Local Darwin arm64 (27.0.0), `/usr/bin/tar`: bsdtar 3.5.3/libarchive 3.7.4.
Exact pinned save bundle above selects GNU `gtar` on Darwin when present;
otherwise BSD `tar`. This host has no `gtar`. Its `getTarArgs` and
`getCompressionProgram` were extracted unchanged and executed with the
supported `zstd-without-long` compression branch. The bundle contains no
`COPYFILE_DISABLE` or metadata suppression flags. `execCommands` passes
`process.env` into the child command.

Reproduction script: `/tmp/velnor-ci-runtime-audit/cache-mac-metadata.cjs`;
fixture: `/tmp/velnor-cache-mac-meta-WZj2E8`. A regular owned file contained
`owned payload`, while its `com.velnor.secret` xattr and resource fork
contained only a test canary. Exact generated command:

```sh
"/usr/bin/tar" --posix -cf cache.tzst --exclude cache.tzst -P -C /tmp/velnor-cache-mac-meta-WZj2E8 --files-from manifest.txt --use-compress-program zstdmt
```

| Action process environment | Compressed archive SHA256 | Secret in decompressed archive | Secret restored as xattr/fork |
|---|---|---|---|
| `COPYFILE_DISABLE` absent | `237a98ef547a838feea6e7a28d4ee4d09f31d24e260788fe7835df21f55ab8d8` | yes | yes/yes |
| `COPYFILE_DISABLE=1` | `0ed780b5bbd83678c5f071baba442765be36f5d2dd9986f8eed2276b5944789a` | yes | no/no |

Both archive listings show only `payload/` and `payload/data`; listing names
alone miss metadata leakage. Decompressed bytes contain a
`LIBARCHIVE.xattr.com.velnor.secret` base64 value and plaintext
`SCHILY.xattr.com.velnor.secret=VELNOR_METADATA_SECRET_CANARY_20261003`.
`COPYFILE_DISABLE=1` alone fails the archive exclusion gate even though it
suppresses restored metadata. Local `tar` documentation says `--no-xattrs`
disables extended attributes; the pinned action has no typed tar-flags input.
Reject metadata-bearing owned payloads before export, or qualify supported
upstream archive controls; do not claim this environment setting fixes the
transport boundary. Parent security/cache owners received this finding.

The initial `zstd` branch generated an unquoted `zstdmt --long=30` shell
command and failed with BSD tar's `Option --long=30 is not supported`.
The metadata experiment therefore uses the supported no-long branch and
does not claim qualification of the action's full default process parser.

## Expanded BSD archive and clone probes

Exact fixture `/tmp/velnor-cache-mac-meta-T5w1i7` added a file ACL
`everyone allow read` to the xattr/resource-fork canary. Source bundle SHA256
remains `9193aa9dbe5025f2bf39802ae241470af35c5e5737fd4bec6e1d6dbaad153bb4`.
`/usr/bin/tar` executable SHA256: `ad4e73ac2e00b3d69969b2e8da466f08c7968a51f424380ecb4d9ff6779b28b9`.
No product transport changes were made.

The `explicit-flags` experiment appends local BSD-supported
`--no-xattrs --no-acls --no-fflags --no-mac-metadata` to the exact extracted
command. This removes canary bytes and AppleDouble/PAX xattr metadata; the
roundtrip retains regular content and omits the added ACL. These flags are
**not supported inputs to the current pinned cache action**. This is a local
archive design comparison, not a released integration or hosted proof.

The `clean-copy` experiment reads regular data bytes into a new tree, then
runs `xattr -cr` and `chmod -RN` on that clone. It removes private canary
metadata and the ACL, but `com.apple.provenance` remains on the clone and
appears in PAX headers. A simple clone therefore cannot establish a blanket
metadata-free payload guarantee. Reject unknown/private attributes and ACLs
with a reviewed exact safe-attribute policy, or implement supported owner
archive suppression; enforce a quiescent owner during observation and save.

GNU/Linux qualification remains source-only here: the exact action selects
GNU tar on Linux and prefers `gtar` on Darwin if available. GNU tar's
[extended attribute manual](https://www.gnu.org/software/tar/manual/html_node/Extended-File-Attributes.html)
states extended attributes and ACLs are disabled by default, with explicit
`--no-xattrs` and `--no-acls` controls. The inherited process environment must
also be constrained, since tar options can change transport semantics.
No local GNU/Linux runtime was available for this probe.

Raw archive-byte counts (zstd-decompressed):

```json
[
  {
    "variant": "default",
    "archiveSha256": "20c06a847626e0e3432f112d0312479017f347291abe4f3ca885fbbbfc18c1e5",
    "AppleDoubleMagicOffset": 1536,
    "SCHILYHeaderCount": 4,
    "LIBARCHIVEHeaderCount": 4,
    "secretPlaintextCount": 4
  },
  {
    "variant": "copyfile-env",
    "archiveSha256": "cabbe0cb25c37f6860d38d9ac27d57c964517ab12d7cf6a54f7947f8fdd973fb",
    "AppleDoubleMagicOffset": -1,
    "SCHILYHeaderCount": 4,
    "LIBARCHIVEHeaderCount": 4,
    "secretPlaintextCount": 2
  },
  {
    "variant": "explicit-flags",
    "archiveSha256": "c565b29fc19aa6caed788c70ab02fd39d9a18565a220f4faacdc7c504721193f",
    "AppleDoubleMagicOffset": -1,
    "SCHILYHeaderCount": 0,
    "LIBARCHIVEHeaderCount": 0,
    "secretPlaintextCount": 0
  },
  {
    "variant": "clean-copy",
    "archiveSha256": "5e1530bb13ce5b8db7029e0abf184ca53f125ce92a0c0688c277e2df80efff01",
    "AppleDoubleMagicOffset": -1,
    "SCHILYHeaderCount": 2,
    "LIBARCHIVEHeaderCount": 2,
    "secretPlaintextCount": 0
  }
]
```

Full reproduction script; save as
`/tmp/velnor-ci-runtime-audit/cache-mac-metadata-variants.cjs` and execute
`rtk proxy node /tmp/velnor-ci-runtime-audit/cache-mac-metadata-variants.cjs`.
It intentionally tests comparison flags outside the action API:

```js
const fs=require('node:fs'),vm=require('node:vm'),crypto=require('node:crypto'),cp=require('node:child_process'),path=require('node:path');
const source=fs.readFileSync('/tmp/velnor-ci-runtime-audit/cache-dist-save.js','utf8');
const sha=crypto.createHash('sha256').update(source).digest('hex');
if(sha!=='9193aa9dbe5025f2bf39802ae241470af35c5e5737fd4bec6e1d6dbaad153bb4')throw Error('bundle mismatch');
function extract(name){const start=source.indexOf('function '+name+'(');if(start<0)throw Error(name);let i=source.indexOf('{',start),d=1;while(d){i++;if(source[i]==='{')d++;if(source[i]==='}')d--;}return source.slice(start,i+1);}
const root=fs.mkdtempSync('/tmp/velnor-cache-mac-meta-');
const context={process:{platform:'darwin',env:{GITHUB_WORKSPACE:root},cwd:()=>root},external_path_:path,CompressionMethod:{Gzip:'gzip',Zstd:'zstd',ZstdWithoutLong:'zstd-without-long'},CacheFilename:{Gzip:'cache.tgz',Zstd:'cache.tzst'},ArchiveToolType:{GNU:'gnu',BSD:'bsd'},ManifestFilename:'manifest.txt',tar_IS_WINDOWS:false};
vm.createContext(context);
const begin=source.indexOf('var tar_awaiter ='),end=source.indexOf('\n};',begin)+3;
vm.runInContext(source.slice(begin,end)+'\n'+['getTarArgs','getCompressionProgram','getWorkingDirectory','getCacheFileName'].map(extract).join('\n'),context);
(async()=>{
 const payload=path.join(root,'payload');fs.mkdirSync(payload);const file=path.join(payload,'data');fs.writeFileSync(file,'owned payload\n');
 const canary='VELNOR_METADATA_SECRET_CANARY_20261003';
 cp.execFileSync('/usr/bin/xattr',['-w','com.velnor.secret',canary,file]);
 fs.writeFileSync(file+'/..namedfork/rsrc',canary);
 cp.execFileSync('/bin/chmod',['+a','everyone allow read',file]);
 fs.writeFileSync(path.join(root,'manifest.txt'),'payload\n');
 const args=await context.getTarArgs({path:'/usr/bin/tar',type:'bsd'},'zstd-without-long','create');
 const compression=await context.getCompressionProgram({path:'/usr/bin/tar',type:'bsd'},'zstd-without-long');
 const command=[...args,...compression].join(' ');
 const results=[];
 for(const variant of ['default','copyfile-env','explicit-flags','clean-copy']){
  const disabled=variant!=='default';
  let activeCommand=command;
  if(variant==='explicit-flags')activeCommand += ' --no-xattrs --no-acls --no-fflags --no-mac-metadata';
  if(variant==='clean-copy'){
   const clean=path.join(root,'clean');fs.mkdirSync(clean);fs.writeFileSync(path.join(clean,'data'),fs.readFileSync(file));
   cp.execFileSync('/usr/bin/xattr',['-cr',clean]);cp.execFileSync('/bin/chmod',['-RN',clean]);
   fs.writeFileSync(path.join(root,'manifest.txt'),'clean\n');
  }
  const env={...process.env};delete env.COPYFILE_DISABLE;if(disabled)env.COPYFILE_DISABLE='1';
  cp.execFileSync('/bin/sh',['-c',activeCommand],{cwd:root,env});
  const archive=path.join(root,variant+'.tzst');fs.renameSync(path.join(root,'cache.tzst'),archive);
  const raw=cp.execFileSync('/opt/homebrew/bin/zstd',['-dc',archive]);
  const listing=cp.execFileSync('/usr/bin/tar',['-tf',archive],{encoding:'utf8'});
  const out=path.join(root,'out-'+variant);fs.mkdirSync(out);
  cp.execFileSync('/usr/bin/tar',['-xf',archive,'-C',out],{env});
  const restored=path.join(out,variant==='clean-copy'?'clean/data':'payload/data');
  let xattrs='';try{xattrs=cp.execFileSync('/usr/bin/xattr',['-p','com.velnor.secret',restored],{encoding:'utf8',stdio:['ignore','pipe','pipe']});}catch{}
  let fork='';try{fork=fs.readFileSync(restored+'/..namedfork/rsrc','utf8');}catch{}
  results.push({variant,COPYFILE_DISABLE:disabled?'1':'absent',command:activeCommand,listing,archiveContainsAcl:raw.includes(Buffer.from('SCHILY.acl')), archiveSha256:crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex'),archiveRawContainsSecret:raw.includes(Buffer.from(canary)),restoredXattrContainsSecret:xattrs.includes(canary),restoredResourceForkContainsSecret:fork.includes(canary),restoredContent:fs.readFileSync(restored,'utf8')});
 }
 console.log(JSON.stringify(results,null,2));
 if(!results[0].archiveRawContainsSecret||!results[1].archiveRawContainsSecret||results[2].archiveRawContainsSecret||results[3].archiveRawContainsSecret||results[2].archiveContainsAcl||results[3].archiveContainsAcl)throw Error('metadata qualification failed');
 console.log(JSON.stringify({actionSha:'55cc8345863c7cc4c66a329aec7e433d2d1c52a9',bundleSha256:sha,root,results},null,2));
})().catch(e=>{console.error(e);process.exitCode=1;});
```
