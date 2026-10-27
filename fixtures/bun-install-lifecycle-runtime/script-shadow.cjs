const fs = require('node:fs');
fs.writeFileSync(process.env.SHADOW_LOG, 'package-script\n');
process.exit(78);
