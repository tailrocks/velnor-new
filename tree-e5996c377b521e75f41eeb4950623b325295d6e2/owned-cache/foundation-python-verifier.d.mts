export type FoundationPython = {
  available: true;
  executable: string;
  argv: readonly ['-I', '-S', '-B'];
  cwd: '/';
  environment: Readonly<{ LANG: 'C'; LC_ALL: 'C' }>;
} | {
  available: false;
  reason: 'unqualified-foundation-image' | 'foundation-closure-mismatch' |
    'foundation-closure-unavailable';
};
export function qualifiedFoundationPython(): FoundationPython;
