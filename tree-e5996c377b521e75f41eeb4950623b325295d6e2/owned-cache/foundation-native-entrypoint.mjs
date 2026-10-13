import fs from 'node:fs';
import { captureFreshFoundation } from './foundation-native-bootstrap.mjs';

function output(name, value) {
  if (!process.env.GITHUB_OUTPUT || /[\r\n]/.test(value))
    throw new Error('Foundation output boundary');
  fs.appendFileSync(process.env.GITHUB_OUTPUT, `${name}=${value}\n`);
}

try {
  const observation = captureFreshFoundation();
  const result = observation.qualification_positive();
  const freshGh = await observation.launchFreshGhQualification();
  output('receipt-path', result.receiptPath);
  output('receipt-sha256', result.receiptSha256);
  output('observation-path', result.observationPath);
  output('observation-sha256', result.observationSha256);
  output('control-root', result.controlRoot);
  output('fresh-gh-receipt-path', freshGh.receiptPath);
  output('fresh-gh-receipt-sha256', freshGh.receiptSha256);
  process.stdout.write('Foundation native qualification positives passed; host record awaits independent review.\n');
} catch {
  process.stderr.write('Foundation native qualification failed; no profile issued.\n');
  process.exitCode = 1;
}
