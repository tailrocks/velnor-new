export declare const ADMISSION_SCHEMA: 'velnor-cache-quarantine-v1';
export declare const ADMISSION_LIMITS: Readonly<{
    maxArchiveBytes: number;
    maxExpandedBytes: number;
    maxEntries: number;
    maxPathBytes: number;
    maxComponentBytes: number;
    maxLinkBytes: number;
    maxDepth: number;
    maxDecompressionRatio: number;
}>;
export declare class ArchiveAdmissionError extends Error {
    readonly code: string;
    constructor(code: string, message: string);
}
export { ArchiveConfigurationError } from './quarantine-path.mjs';
export interface ArchiveAdmissionOptions {
    workspace?: string;
}
export interface ArchiveAdmissionManifest {
    schema: 'velnor-cache-quarantine-v1';
    archive_sha256: string;
    archive_bytes: number;
    compression: string;
    workspace: string;
    ordered_roots: Array<{
        index: number;
        original_root: string;
        archive_root: string;
        quarantine_root: string;
    }>;
    entries: Array<{
        member: string;
        original_root_index: number;
        relative_path: string;
        quarantine_path: string;
        kind: string;
        mode: number;
        size: number;
        content_sha256: string | null;
        link_target: string | null;
        resolved_link_path: string | null;
    }>;
    totals: {
        entry_count: number;
        expanded_bytes: number;
    };
}
export declare function admitArchive(archivePath: string, compressionMethod: string, paths: string[], options?: ArchiveAdmissionOptions): Promise<ArchiveAdmissionManifest>;
export declare function extractAdmittedArchive(archivePath: string, manifest: ArchiveAdmissionManifest, quarantinePath: string): Promise<ArchiveAdmissionManifest>;
export declare function validateQuarantinePath(quarantinePath: string, runnerTemp?: string): string;
export declare function createQuarantinePath(quarantinePath: string, descriptorDigest: string, runnerTemp?: string): string;
export declare function writeAdmissionManifest(quarantinePath: string, manifest: ArchiveAdmissionManifest): string;
