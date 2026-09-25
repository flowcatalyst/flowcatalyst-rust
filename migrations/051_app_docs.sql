-- Go's 044_app_docs (flowcatalyst-go internal/migrate/sql/044_app_docs.sql,
-- Up section, verbatim). Application-synced documentation: apps push their
-- Markdown docs through the SDK sync surface
-- (POST /api/applications/{appCode}/docs/sync) the same way they sync event
-- types and OpenAPI specs. Declarative full-replace per app; read by
-- administrators in Platform -> Documentation.
--
-- Idempotent: a database Go has migrated already has the table.
CREATE TABLE IF NOT EXISTS app_docs (
    id VARCHAR(17) PRIMARY KEY,
    application_id VARCHAR(17) NOT NULL,
    slug VARCHAR(120) NOT NULL,
    title VARCHAR(200) NOT NULL,
    content TEXT NOT NULL,
    position INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE (application_id, slug)
);
CREATE INDEX IF NOT EXISTS idx_app_docs_application ON app_docs(application_id, position);

