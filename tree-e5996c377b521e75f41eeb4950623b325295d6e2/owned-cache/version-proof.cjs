const fs=require('node:fs'),vm=require('node:vm'),crypto=require('node:crypto');
const evidence={version:'6.1.0-velnor.1',salt:'velnor-cache-archive-v1',bundles:{},payloads:{}};
const engines={};
for(const name of ['restore','save','restore-only','save-only']) {
 const bytes=fs.readFileSync('dist/'+name+'/index.js'),source=bytes.toString();
 const start=source.indexOf('function getCacheVersion(');let i=source.indexOf('{',start),depth=1;
 if(start<0||source.indexOf('function getCacheVersion(',start+1)>=0)throw Error('nonunique function');
 while(depth){i++;if(source[i]==='{')depth++;if(source[i]==='}')depth--;}
 const fn=source.slice(start,i+1),salt=source.match(/const versionSalt = '[^']+';/)?.[0];
 if(salt!=="const versionSalt = 'velnor-cache-archive-v1';")throw Error('incorrect salt');
 for(const marker of ["args.push('--no-xattrs', '--no-acls')", "args.push('--no-fflags', '--no-mac-metadata')", "'--null', '--files-from'", "manifestPaths.join('\\0')", "COPYFILE_DISABLE: '1'"])if(!source.includes(marker))throw Error('missing '+marker);
 const context={external_crypto_namespaceObject:crypto,process:{platform:'linux'}};vm.createContext(context);vm.runInContext(salt+'\n'+fn+'\nthis.engine=getCacheVersion;',context);engines[name]=context.engine;
 evidence.bundles[name]={sha256:crypto.createHash('sha256').update(bytes).digest('hex'),functionSha256:crypto.createHash('sha256').update(fn).digest('hex')};
}
if(new Set(Object.values(evidence.bundles).map(bundle=>bundle.functionSha256)).size!==1)throw Error('functions differ');
for(const [payload,roots] of Object.entries({tools:['mise','rustup','cargo/bin','cargo/.crates.toml','cargo/.crates2.json'],sources:['cargo/registry/index','cargo/registry/cache','cargo/git/db']})) {
 const paths=roots.map(root=>'/home/runner/work/_temp/velnor/'+root),restore=engines.restore(paths,'zstd-without-long',false),save=engines.save(paths,'zstd-without-long',false),reversed=engines.save([...paths].reverse(),'zstd-without-long',false);
 if(restore!==save||reversed===save)throw Error('version failure');evidence.payloads[payload]={paths,restore,save,reversed};
}
console.log(JSON.stringify(evidence,null,2));
