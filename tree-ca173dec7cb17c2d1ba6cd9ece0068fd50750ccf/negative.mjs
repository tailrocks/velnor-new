import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import cp from 'node:child_process';
const target=new URL('../node_modules/@actions/cache/lib/internal/upstream-tar.js',import.meta.url);
fs.copyFileSync(new URL('upstream-tar.js',import.meta.url),target);
const {createTar}=await import(target);
const root=fs.mkdtempSync(path.join(os.tmpdir(),'velnor-cache-upstream-'));
const source=path.join(root,'source');fs.mkdirSync(source);
const file=path.join(source,'data');fs.writeFileSync(file,'owned payload\n');
const canary='VELNOR_METADATA_SECRET_CANARY_20261003';
if(process.platform==='darwin') {
 cp.execFileSync('/usr/bin/xattr',['-w','com.velnor.secret',canary,file]);
 fs.writeFileSync(file+'/..namedfork/rsrc',canary);
 cp.execFileSync('/bin/chmod',['+a','everyone allow read',file]);
} else {
 cp.execFileSync('setfattr',['-n','user.velnor.secret','-v',canary,file]);
 cp.execFileSync('setfacl',['-m','u:12345:r',file]);
 process.env.TAR_OPTIONS='--xattrs --acls';
}
process.env.GITHUB_WORKSPACE=source;delete process.env.GZIP;
const archiveFolder=path.join(root,'archive');fs.mkdirSync(archiveFolder);
await createTar(archiveFolder,['data'],'gzip');
const archive=path.join(archiveFolder,'cache.tgz');
const raw=cp.execFileSync('gzip',['-dc',archive]);
if(!raw.includes(Buffer.from(canary)))throw Error('negative control failed');
console.log(JSON.stringify({platform:process.platform,root,upstreamTransportLeaksCanary:true,rawContainsXattr:raw.includes(Buffer.from('SCHILY.xattr')),rawContainsAcl:raw.includes(Buffer.from('SCHILY.acl'))},null,2));
fs.unlinkSync(target);
