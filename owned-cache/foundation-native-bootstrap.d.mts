export type NativeFoundationQualification = {
  receiptPath: string;
  receiptSha256: string;
  observationPath: string;
  observationSha256: string;
  controlRoot: string;
};
export interface FreshFoundationObservation {
  readonly control_root: string;
  readonly payload_roots: readonly string[];
  readonly ca_file: '/etc/ssl/certs/ca-certificates.crt';
  require_current(): void;
  qualification_positive(): NativeFoundationQualification;
  launchFreshGhQualification(): Promise<{
    receiptPath: string; receiptSha256: string; controlRoot: string;
  }>;
}
export function captureFreshFoundation(): FreshFoundationObservation;
