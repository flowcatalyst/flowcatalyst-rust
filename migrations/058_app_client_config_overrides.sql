-- An application's per-client configuration: the base-URL override and the
-- configuration document.
--
-- Go documents both on `ClientConfigResponse` (`baseUrlOverride`,
-- `configJson`) but has no column for either, so they are never populated
-- (flowcatalyst-go `application/client_config.go`: "transient (API-only)";
-- owner question APP-3). The Java platform stored them (`baseUrlOverride`,
-- `configJson` on the client config document). This platform stores them
-- here, sets them through `PUT /api/applications/{id}/clients/{clientId}`
-- and returns them on every client-config read.
--
-- base_url_override is as wide as app_applications.default_base_url, the
-- URL it overrides. Both columns are nullable: absent means "use the
-- application's default" / "no configuration document".
--
-- ADD COLUMN IF NOT EXISTS: re-running is a no-op.
ALTER TABLE app_client_configs
    ADD COLUMN IF NOT EXISTS base_url_override VARCHAR(500),
    ADD COLUMN IF NOT EXISTS config_json JSONB;
