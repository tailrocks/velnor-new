const fs = require('node:fs');

fs.writeFileSync('main-ran', 'main-ran\n');
process.stdout.write('main-ran\n');
process.exit(1);
