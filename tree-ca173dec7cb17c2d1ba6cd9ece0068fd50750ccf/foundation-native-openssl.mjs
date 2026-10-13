import fs from 'node:fs';
import path from 'node:path';

export function opensslConfigurationRoots() {
  const roots = new Set(['/etc/ssl/openssl.cnf', '/usr/lib/ssl/openssl.cnf']);
  const seen = new Set();
  function inspect(filename) {
    roots.add(filename);
    if (!fs.existsSync(filename)) return;
    const real = fs.realpathSync(filename);
    if (seen.has(real)) return;
    if (seen.size >= 1024) throw new Error('Foundation OpenSSL config bound');
    seen.add(real);
    const stat = fs.statSync(real);
    if (stat.isDirectory()) {
      const children = fs.readdirSync(real);
      if (children.length > 1024) throw new Error('Foundation OpenSSL directory bound');
      for (const child of children)
        if (/\.(cnf|conf)$/.test(child)) inspect(path.join(real, child));
      return;
    }
    if (!stat.isFile() || stat.size > 1024 * 1024)
      throw new Error('Foundation OpenSSL config bound');
    for (const raw of fs.readFileSync(real, 'utf8').split('\n')) {
      const line = raw.split('#')[0].trim();
      const include = line.match(/^\.include\s+(.+)$/);
      const module = line.match(/^(?:module|dynamic_path)\s*=\s*(.+)$/);
      if (!include && !module) continue;
      const value = (include ?? module)[1].trim().replace(/^"(.*)"$/, '$1');
      if (!path.isAbsolute(value) || path.normalize(value) !== value || value.includes('$'))
        throw new Error('Foundation OpenSSL unresolved configuration');
      if (include) inspect(value);
      else {
        roots.add(value);
        roots.add(path.dirname(value));
      }
    }
  }
  for (const filename of [...roots]) inspect(filename);
  return [...roots].sort();
}
