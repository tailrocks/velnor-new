export declare const METADATA_CONTAINER_SOURCE_SHA256: string;
export declare const METADATA_CONTAINER_MAX_PREFIX_BYTES: number;
export declare const METADATA_CONTAINER_MAX_CANDIDATES: number;
export declare const METADATA_CONTAINER_MAX_TOTAL_PREFIX_BYTES: number;
export interface MetadataPredicateBudget {
    candidateCount: number;
    prefixBytes: number;
}
export declare function inspectMetadataPrefix(prefix: Uint8Array, size: number, budget?: MetadataPredicateBudget): void;
