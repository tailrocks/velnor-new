const fs = require('node:fs');

const summary = process.env.GITHUB_STEP_SUMMARY;
if (typeof summary === 'string' && summary !== '') {
  fs.appendFileSync(summary, 'post-ran\n');
}
fs.writeFileSync('post-ran', 'post-ran\n');
