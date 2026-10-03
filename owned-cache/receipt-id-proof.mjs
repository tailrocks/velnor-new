import fs from 'node:fs';
import vm from 'node:vm';
import assert from 'node:assert/strict';
const patch=JSON.parse(fs.readFileSync(new URL('service-receipt-patch.json',import.meta.url))).replacements[0].to;
const context={};vm.createContext(context);
vm.runInContext('this.decode=function(entryId){const finalizeResponse={entryId};let cacheId=-1;'+patch+';return cacheId;}',context);
for(const value of ['0','42','9007199254740991']) assert.equal(context.decode(value),Number(value));
for(const value of ['', ' ', '42bad', '-1', '1.5', '0x2a', '+42','9007199254740992']) assert.throws(()=>context.decode(value));
console.log(JSON.stringify({exactDecimalSafeId:true,positiveCases:3,negativeCases:8}));
