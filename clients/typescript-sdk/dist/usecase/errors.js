/**
 * Use case error hierarchy.
 *
 * Categorised for consistent HTTP mapping (400/403/404/409/500) in API layers.
 * Mirrors the FlowCatalyst platform's `UseCaseError` so consumer apps use the
 * same vocabulary as the platform.
 */
export const UseCaseError = {
    validation(code, message, details = {}) {
        return { type: "validation", code, message, details };
    },
    notFound(code, message, details = {}) {
        return { type: "not_found", code, message, details };
    },
    businessRule(code, message, details = {}) {
        return { type: "business_rule", code, message, details };
    },
    concurrency(code, message, details = {}) {
        return { type: "concurrency", code, message, details };
    },
    authorization(code, message, details = {}) {
        return { type: "authorization", code, message, details };
    },
    infrastructure(code, message, details = {}) {
        return { type: "infrastructure", code, message, details };
    },
    httpStatus(error) {
        switch (error.type) {
            case "validation":
                return 400;
            case "authorization":
                return 403;
            case "not_found":
                return 404;
            case "business_rule":
            case "concurrency":
                return 409;
            case "infrastructure":
                return 500;
        }
    },
    isUseCaseError(value) {
        if (typeof value !== "object" || value === null)
            return false;
        const o = value;
        return (typeof o["type"] === "string" &&
            typeof o["code"] === "string" &&
            typeof o["message"] === "string" &&
            typeof o["details"] === "object");
    },
};
