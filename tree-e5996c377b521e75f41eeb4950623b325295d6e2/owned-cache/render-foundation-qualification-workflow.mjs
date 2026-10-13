import fs from 'node:fs';
import path from 'node:path';

// Build/publication owner API only, never a runtime Foundation trust input.
export function renderFoundationQualificationWorkflow(publishedReference) {
  if (typeof publishedReference !== 'string' ||
      !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/foundation-qualification@[0-9a-f]{40}$/.test(publishedReference))
    throw new Error('Foundation qualification requires actual immutable publication reference');
  const source = fs.readFileSync(path.join(import.meta.dirname,
    '../foundation-qualification/workflow.yml.in'), 'utf8');
  const marker = '__FOUNDATION_ACTION_REF__';
  if (source.split(marker).length !== 2) throw new Error('Foundation workflow template marker');
  return source.replace(marker, publishedReference);
}
