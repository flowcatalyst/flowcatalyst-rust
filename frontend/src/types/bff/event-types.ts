/**
 * BFF Event Type contracts — local type stubs.
 *
 * These mirror the shapes returned by the BFF endpoints
 * so the frontend can consume them without depending on the
 * TypeScript backend package.
 */

export interface BffSpecVersion {
	version: string;
	mimeType: string;
	schema?: string;
	schemaType: "JSON_SCHEMA" | "PROTO" | "XSD";
	status: "FINALISING" | "CURRENT" | "DEPRECATED";
}

export interface BffEventType {
	id: string;
	code: string;
	application: string;
	subdomain: string;
	aggregate: string;
	event: string;
	name: string;
	description?: string;
	status: "CURRENT" | "ARCHIVED";
	clientScoped: boolean;
	specVersions: BffSpecVersion[];
	createdAt: string;
	updatedAt: string;
}

export interface BffEventTypeListResponse {
	items: BffEventType[];
	total: number;
}

export interface BffFilterOptionsResponse {
	options: string[];
}

export interface BffCreateEventTypeRequest {
	code: string;
	name: string;
	description?: string;
	clientScoped: boolean;
}

export interface BffUpdateEventTypeRequest {
	name?: string;
	description?: string;
}

export interface BffAddSchemaRequest {
	version: string;
	mimeType: string;
	schema: string;
	schemaType: "JSON_SCHEMA" | "PROTO" | "XSD";
}

// Re-export with aliases matching the original package exports
export type {
	BffEventType as EventType,
	BffSpecVersion as SpecVersion,
	BffEventTypeListResponse as EventTypeListResponse,
	BffFilterOptionsResponse as FilterOptionsResponse,
	BffCreateEventTypeRequest as CreateEventTypeRequest,
	BffUpdateEventTypeRequest as UpdateEventTypeRequest,
	BffAddSchemaRequest as AddSchemaRequest,
};

// Derived enum types
export type EventTypeStatus = BffEventType["status"];
export type SchemaType = BffSpecVersion["schemaType"];
export type SpecVersionStatus = BffSpecVersion["status"];
