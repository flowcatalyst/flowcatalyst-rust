/**
 * Use case error hierarchy.
 *
 * Categorised for consistent HTTP mapping (400/403/404/409/500) in API layers.
 * Mirrors the FlowCatalyst platform's `UseCaseError` so consumer apps use the
 * same vocabulary as the platform.
 */
export interface UseCaseErrorBase {
    readonly type: string;
    readonly code: string;
    readonly message: string;
    readonly details: Record<string, unknown>;
}
export interface ValidationError extends UseCaseErrorBase {
    readonly type: "validation";
}
export interface NotFoundError extends UseCaseErrorBase {
    readonly type: "not_found";
}
export interface BusinessRuleViolation extends UseCaseErrorBase {
    readonly type: "business_rule";
}
export interface ConcurrencyError extends UseCaseErrorBase {
    readonly type: "concurrency";
}
export interface AuthorizationError extends UseCaseErrorBase {
    readonly type: "authorization";
}
/** Infrastructure failure — e.g. outbox write failed, DB unavailable. Maps to 500. */
export interface InfrastructureError extends UseCaseErrorBase {
    readonly type: "infrastructure";
}
export type UseCaseError = ValidationError | NotFoundError | BusinessRuleViolation | ConcurrencyError | AuthorizationError | InfrastructureError;
export declare const UseCaseError: {
    validation(code: string, message: string, details?: Record<string, unknown>): ValidationError;
    notFound(code: string, message: string, details?: Record<string, unknown>): NotFoundError;
    businessRule(code: string, message: string, details?: Record<string, unknown>): BusinessRuleViolation;
    concurrency(code: string, message: string, details?: Record<string, unknown>): ConcurrencyError;
    authorization(code: string, message: string, details?: Record<string, unknown>): AuthorizationError;
    infrastructure(code: string, message: string, details?: Record<string, unknown>): InfrastructureError;
    httpStatus(error: UseCaseError): number;
    isUseCaseError(value: unknown): value is UseCaseError;
};
//# sourceMappingURL=errors.d.ts.map