const fs = require('node:fs');
const out = { bun: process.versions.bun || null, execPath: process.execPath, argv: process.argv };
fs.writeFileSync(process.env.LIFECYCLE_RECEIPT, JSON.stringify(out) + '\n');
