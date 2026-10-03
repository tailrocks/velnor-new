export declare class ArchiveConfigurationError extends Error {
    readonly code: string;
    constructor(code: string, message: string);
}
export declare function validateQuarantinePath(quarantinePath: string, runnerTemp?: string): string;
export declare function createQuarantinePath(quarantinePath: string, descriptorDigest: string, runnerTemp?: string): string;
export declare function writeAdmissionManifest(quarantinePath: string, manifest: {
    schema: 'velnor-cache-quarantine-v1';
    entries: unknown[];
}): string;
