import { inventoryFoundation, canonicalInventory } from './foundation-python-inventory.mjs';

// Observations become authority only after source review, immutable publication,
// and exact hosted-image qualification. Caller records/digests are never accepted.
const QUALIFIED_HOSTED_PROFILES = Object.freeze([]);

export function qualifiedFoundationPython() {
  const image = {
    platform: process.platform,
    architecture: process.arch,
    imageOS: process.env.ImageOS,
    imageVersion: process.env.ImageVersion
  };
  const profile = QUALIFIED_HOSTED_PROFILES.find(candidate =>
    canonicalInventory(candidate.host) === canonicalInventory(image));
  if (!profile) return { available: false, reason: 'unqualified-foundation-image' };
  try {
    const actual = inventoryFoundation(profile.roots);
    if (canonicalInventory(actual) !== canonicalInventory(profile.inventory))
      return { available: false, reason: 'foundation-closure-mismatch' };
    if (!actual.some(entry => entry.path === profile.executable && entry.kind === 'file'))
      return { available: false, reason: 'foundation-closure-mismatch' };
    return { available: true, executable: profile.executable,
      argv: Object.freeze(['-I', '-S', '-B']),
      cwd: '/',
      environment: Object.freeze({ LANG: 'C', LC_ALL: 'C' }) };
  } catch {
    return { available: false, reason: 'foundation-closure-unavailable' };
  }
}
