import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import cp from 'node:child_process';
import { zstdDecompressSync } from 'node:zlib';
import { createTar, extractTar } from '../node_modules/@actions/cache/lib/internal/tar.js';
import { getCacheVersion } from '../node_modules/@actions/cache/lib/internal/cacheUtils.js';
const root=fs.mkdtempSync(path.join(os.tmpdir(),'velnor-cache-owned-'));
const source=path.join(root,'source');fs.mkdirSync(source);
const payload=path.join(source,'payload');fs.mkdirSync(payload);
const file=path.join(payload,'data');fs.writeFileSync(file,'owned payload\n');fs.chmodSync(file,0o755);
fs.symlinkSync('data',path.join(payload,'link'));
const canary='VELNOR_METADATA_SECRET_CANARY_20261003';
if(process.platform==='darwin') {
 cp.execFileSync('/usr/bin/xattr',['-w','com.velnor.secret',canary,file]);
 fs.writeFileSync(file+'/..namedfork/rsrc',canary);
 cp.execFileSync('/bin/chmod',['+a','everyone allow read',file]);
 cp.execFileSync('/usr/bin/chflags',['hidden',file]);
} else {
 cp.execFileSync('setfattr',['-n','user.velnor.secret','-v',canary,file]);
 cp.execFileSync('setfacl',['-m','u:12345:r',file]);
 cp.execFileSync('/usr/bin/chattr',['+d',file]);
}
process.env.TAR_OPTIONS='--xattrs --acls';process.env.GZIP='--invalid-owned-probe';
process.env.GITHUB_WORKSPACE=source;
const sourceFlags=process.platform==='darwin'?cp.execFileSync('/bin/ls',['-ldO',file],{encoding:'utf8'}):cp.execFileSync('/usr/bin/lsattr',['-d',file],{encoding:'utf8'});
if(process.platform==='darwin'?!sourceFlags.includes('hidden'):!sourceFlags.split(' ')[0].includes('d'))throw Error('nonzero source flags fixture missing');
const results=[];
const probeEnvironment={...process.env};delete probeEnvironment.GZIP;delete probeEnvironment.TAR_OPTIONS;
for(const method of ['zstd-without-long']) {
 const archiveFolder=path.join(root,method);fs.mkdirSync(archiveFolder);
 process.env.GITHUB_WORKSPACE=source;
 await createTar(archiveFolder,['payload'],method);
 const archive=path.join(archiveFolder,method==='gzip'?'cache.tgz':'cache.tzst');
 const raw=zstdDecompressSync(fs.readFileSync(archive));
 process.env.RUNNER_TEMP=root;
 const quarantine=path.join(root,'velnor','cache-staging','b'.repeat(64));
 await extractTar(archive,method,{roots:[payload],quarantinePath:quarantine});
 const out=path.join(quarantine,'roots','0');
 const restored=path.join(out,'data');
 let attrs='';let acl='';let fork='';
 if(process.platform==='darwin') {
  attrs=cp.execFileSync('/usr/bin/xattr',[restored],{encoding:'utf8'});
  acl=cp.execFileSync('/bin/ls',['-le',restored],{encoding:'utf8'});
  try {fork=fs.readFileSync(restored+'/..namedfork/rsrc','utf8');}catch {}
 } else {
  attrs=cp.execFileSync('getfattr',['-d',restored],{encoding:'utf8'});
  acl=cp.execFileSync('getfacl',['-cp',restored],{encoding:'utf8'});
 }
 const result={method,archiveSha256:crypto.createHash('sha256').update(fs.readFileSync(archive)).digest('hex'),rawContainsCanary:raw.includes(Buffer.from(canary)),rawContainsXattr:raw.includes(Buffer.from('SCHILY.xattr'))||raw.includes(Buffer.from('LIBARCHIVE.xattr')),rawContainsAcl:raw.includes(Buffer.from('SCHILY.acl')),rawContainsFflags:raw.includes(Buffer.from('SCHILY.fflags'))||raw.includes(Buffer.from('LIBARCHIVE.fflags')),sourceFlags,rawContainsAppleDouble:raw.includes(Buffer.from([0,5,22,7])),content:fs.readFileSync(restored,'utf8'),mode:fs.statSync(restored).mode&0o777,link:fs.readlinkSync(path.join(out,'link')),attrs,acl,fork};
 if(result.rawContainsCanary||result.rawContainsXattr||result.rawContainsAcl||result.rawContainsFflags||result.rawContainsAppleDouble||attrs.includes('velnor.secret')||fork.includes(canary)||acl.includes('everyone allow')||acl.includes('12345')||result.content!=='owned payload\n'||result.mode!==0o755||result.link!=='data')throw Error(JSON.stringify(result));
 results.push(result);
}
const canonical=['/tmp/velnor/mise','/tmp/velnor/rustup','/tmp/velnor/cargo/bin'];
const version=getCacheVersion(canonical,'zstd-without-long',false);
if(version===getCacheVersion([...canonical].reverse(),'zstd-without-long',false))throw Error('version ordering negative');
console.log(JSON.stringify({platform:process.platform,root,node:process.version,tarVersion:cp.execFileSync('tar',['--version'],{encoding:'utf8'}),hostileTarEnvironmentIgnored:true,canonicalVersion:version,results},null,2));
