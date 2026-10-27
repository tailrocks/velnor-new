#!/usr/bin/env node
const fs = require('node:fs');
fs.writeFileSync(process.env.SHADOW_LOG, 'local-bin\n');
