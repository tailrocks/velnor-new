import fs from 'node:fs';
import crypto from 'node:crypto';
const root=new URL('../',import.meta.url);
const lock=JSON.parse(fs.readFileSync(new URL('package-lock.json',root)));
const records=[],notices=['THIRD PARTY NOTICES\n\nExact production graph from owned package-lock.json.\n'];
notices.push('Shared velnor metadata predicate; MIT license selected from MIT OR Apache-2.0.\n');
notices.push(fs.readFileSync(new URL('LICENSE.velnor-shared-MIT',import.meta.url),'utf8'));
for(const [path,entry] of Object.entries(lock.packages)) {
 if(!path||entry.dev)continue;
 const directory=new URL(path+'/',root);
 const metadata=JSON.parse(fs.readFileSync(new URL('package.json',directory)));
 const licenseFiles=fs.readdirSync(directory).filter(name=>/^(license|copying)/i.test(name)&&fs.statSync(new URL(name,directory)).isFile()).sort();
 const record={path,version:entry.version,integrity:entry.integrity,license:metadata.license,licenseFiles:[]};
 notices.push('\n*****\n'+path.replace(/^node_modules\//,'')+'@'+entry.version+'\n');
 for(const name of licenseFiles) {
  const bytes=fs.readFileSync(new URL(name,directory));
  record.licenseFiles.push({path:path+'/'+name,sha256:crypto.createHash('sha256').update(bytes).digest('hex')});
  notices.push(bytes.toString());
 }
 if(!licenseFiles.length)notices.push('Published package license declaration: '+metadata.license+'\nPackage author: '+String(metadata.author||'')+'\nThe exact registry package contains no separate license file.\n');
 records.push(record);
}
fs.writeFileSync(new URL('licenses.json',import.meta.url),JSON.stringify(records,null,2)+'\n');
fs.writeFileSync(new URL('.licenses/NOTICE',root),notices.join('\n'));
console.log(JSON.stringify({productionPackages:records.length,noSeparateLicenseFile:records.filter(record=>!record.licenseFiles.length).map(record=>record.path)}));
