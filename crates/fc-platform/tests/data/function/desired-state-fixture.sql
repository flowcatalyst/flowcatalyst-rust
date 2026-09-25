-- The desired-state golden fixture: rows valid under both schemas (Java
-- V1-V17, Rust 001-035). Loaded by DesiredStateGoldenGen.java into a
-- Java-migrated database and by function_desired_state_golden_test.rs into a
-- Rust-migrated one; both build the document of each pool at
-- 2026-09-25T12:00:00Z with the app key MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=
-- (the bytes of "0123456789abcdef0123456789abcdef"), and must write the same
-- bytes. Functions are inserted in reverse address order so the sort is on
-- the address, never on insertion order.
--
-- Pools: edge (most cases), batch (a candidate and an alias-only version
-- whose live versions are elsewhere), other (empty), broken (a corrupt live
-- version: CORRUPT_ROW).

-- ── Functions ───────────────────────────────────────────────────────────────
INSERT INTO fn_functions (id, application_id, application_code, service_name, name, client_id, runtime, status, created_at, updated_at) VALUES
 ('fnc_F09', 'app_ZETA',  'zeta',  'bad',     'live',     'clt_ACME', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F10', 'app_SHOP',  'shop',  'bad',     'cand',     'clt_ACME', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F06', 'app_SHOP',  'shop',  'older',   'pub',      'clt_ACME', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F01', 'app_SHOP',  'shop',  'orders',  'create',   'clt_ACME', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F04', 'app_SHOP',  'shop',  'orders',  'disabled', 'clt_ACME', 'WASM', 'DISABLED', '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F05', 'app_SHOP',  'shop',  'move',    'pool',     'clt_ACME', 'JVM',  'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F02', 'app_SHOP',  'shop',  'billing', 'invoice',  NULL,       'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F08', 'app_SHOP',  'shop',  'alias',   'other',    'clt_ACME', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F11', 'app_BLANK', 'blank', 'svc',     'fn',       'clt_ACME', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F07', 'app_BETA',  'beta',  'first',   'pub',      'clt_BETA', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z'),
 ('fnc_F03', 'app_NOSA',  'alpha', 'svc',     'fn',       'clt_ACME', 'WASM', 'ACTIVE',   '2026-09-01T00:00:00Z', '2026-09-01T00:00:00Z');

-- ── Versions ────────────────────────────────────────────────────────────────
-- shop.orders.create: live v1 (warm, webhook, signed, settings, two named
-- aliases), v2 aliased `qa` only, v3 the newest PUBLISHED (candidate; its
-- own manifest says warm, a candidate is still lazy).
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, signature_bundle, signer_issuer, signer_subject, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F01V1', 'fnc_F01', 1, 'platform://fnc_F01/0101010101010101010101010101010101010101010101010101010101010101',
  'sha256:0101010101010101010101010101010101010101010101010101010101010101',
  '{"mediaType":"application/vnd.dev.sigstore.bundle.v0.3+json"}', 'https://token.actions.githubusercontent.com', 'repo:acme/orders:ref:refs/heads/main',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":true,"limits":{"maxDurationMs":5000,"maxConcurrency":8,"wasmMemoryMb":32},"endpoints":[{"path":"/hook","auth":"webhook","methods":["POST"],"maxBodyBytes":1048576,"timeoutMs":5000},{"path":"/orders/*","auth":"none","maxBodyBytes":65536,"timeoutMs":2000}],"subscriptions":[{"eventType":"shop:orders:order:created","path":"/hook","mode":"IMMEDIATE","maxRetries":3,"timeoutSeconds":30,"dataOnly":false}],"schedules":[{"cron":"0 0 * * * *","timezone":"UTC","path":"/hook","payload":{"k":[1,2.5,"ü"]}}],"public":[{"hostname":"api.shop.example.com","pathPrefix":"/orders","aliasPrefixes":["qa","beta"]}],"config":["REGION","TIMEOUT"],"secrets":["API_KEY","QUOTED"],"db":[],"httpAllow":["api.carrier.example"]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z'),
 ('fnv_F01V2', 'fnc_F01', 2, 'oci://registry.example/shop/orders@sha256:0202020202020202020202020202020202020202020202020202020202020202',
  'sha256:0202020202020202020202020202020202020202020202020202020202020202', NULL, NULL, NULL,
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[{"path":"/orders/*","auth":"platform","maxBodyBytes":1048576,"timeoutMs":30000}],"subscriptions":[],"schedules":[],"public":[],"config":["REGION"],"secrets":[],"db":[],"httpAllow":[]}',
  'PUBLISHED', 'prn_PUB', '2026-09-03T00:00:00Z', NULL),
 ('fnv_F01V3', 'fnc_F01', 3, 'file:///artifacts/shop-orders-3.wasm',
  'sha256:0303030303030303030303030303030303030303030303030303030303030303', NULL, NULL, NULL,
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":true,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[{"path":"/hook","auth":"webhook","maxBodyBytes":1048576,"timeoutMs":30000}],"subscriptions":[],"schedules":[],"public":[],"config":["REGION","NEW_KEY"],"secrets":["API_KEY"],"db":[],"httpAllow":[]}',
  'PUBLISHED', 'prn_PUB', '2026-09-04T00:00:00Z', NULL);

-- shop.billing.invoice: platform-owned (no clientId), a webhook endpoint (the
-- application's oldest ACTIVE service account signs), a config value that
-- needs escaping.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F02V1', 'fnc_F02', 1, 'oci://registry.example/shop/billing@sha256:1111111111111111111111111111111111111111111111111111111111111111',
  'sha256:1111111111111111111111111111111111111111111111111111111111111111',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[{"path":"/events/*","auth":"webhook","maxBodyBytes":1048576,"timeoutMs":30000}],"subscriptions":[],"schedules":[],"public":[],"config":["GREETING"],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z');

-- alpha.svc.fn: a webhook endpoint but no service account at all; a public
-- route with no alias prefixes.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F03V1', 'fnc_F03', 1, 'oci://registry.example/alpha@sha256:3333333333333333333333333333333333333333333333333333333333333333',
  'sha256:3333333333333333333333333333333333333333333333333333333333333333',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[{"path":"/events/*","auth":"webhook","maxBodyBytes":1048576,"timeoutMs":30000}],"subscriptions":[],"schedules":[],"public":[{"hostname":"a.example.com","pathPrefix":"/"}],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z');

-- shop.orders.disabled: live in edge, but the function is DISABLED.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F04V1', 'fnc_F04', 1, 'oci://registry.example/disabled@sha256:4444444444444444444444444444444444444444444444444444444444444444',
  'sha256:4444444444444444444444444444444444444444444444444444444444444444',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":true,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z');

-- shop.move.pool (JVM): live v1 in edge; candidate v2 in batch, with a db
-- connection whose DSN secret is declared through db[].secretRef.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F05V1', 'fnc_F05', 1, 'oci://registry.example/move@sha256:5151515151515151515151515151515151515151515151515151515151515151',
  'sha256:5151515151515151515151515151515151515151515151515151515151515151',
  '{"runtime":"jvm","entrypoint":"com.acme.Move","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z'),
 ('fnv_F05V2', 'fnc_F05', 2, 'oci://registry.example/move@sha256:5252525252525252525252525252525252525252525252525252525252525252',
  'sha256:5252525252525252525252525252525252525252525252525252525252525252',
  '{"runtime":"jvm","entrypoint":"com.acme.Move","pool":"batch","warm":true,"limits":{"maxDurationMs":30000,"maxConcurrency":32},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":["API_KEY"],"db":[{"name":"main","secretRef":"DB_DSN","poolSize":4}],"httpAllow":[]}',
  'PUBLISHED', 'prn_PUB', '2026-09-03T00:00:00Z', NULL);

-- shop.older.pub: live v2; v1 PUBLISHED but older than live, so no candidate.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F06V1', 'fnc_F06', 1, 'oci://registry.example/older@sha256:6161616161616161616161616161616161616161616161616161616161616161',
  'sha256:6161616161616161616161616161616161616161616161616161616161616161',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'PUBLISHED', 'prn_PUB', '2026-09-02T00:00:00Z', NULL),
 ('fnv_F06V2', 'fnc_F06', 2, 'oci://registry.example/older@sha256:6262626262626262626262626262626262626262626262626262626262626262',
  'sha256:6262626262626262626262626262626262626262626262626262626262626262',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-03T00:00:00Z', '2026-09-03T00:01:00Z');

-- beta.first.pub: nothing live yet; its only version is a candidate.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F07V1', 'fnc_F07', 1, 'oci://registry.example/beta@sha256:7777777777777777777777777777777777777777777777777777777777777777',
  'sha256:7777777777777777777777777777777777777777777777777777777777777777',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":true,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'PUBLISHED', 'prn_PUB', '2026-09-02T00:00:00Z', NULL);

-- shop.alias.other: live v1 in edge; alias `canary` on v2 (READY) in batch.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F08V1', 'fnc_F08', 1, 'oci://registry.example/alias@sha256:8181818181818181818181818181818181818181818181818181818181818181',
  'sha256:8181818181818181818181818181818181818181818181818181818181818181',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z'),
 ('fnv_F08V2', 'fnc_F08', 2, 'oci://registry.example/alias@sha256:8282828282828282828282828282828282828282828282828282828282828282',
  'sha256:8282828282828282828282828282828282828282828282828282828282828282',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"batch","warm":true,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-03T00:00:00Z', '2026-09-03T00:01:00Z');

-- zeta.bad.live: a corrupt live version (runtime unreadable, pool readable:
-- broken). Only the `broken` pool's build fails.
-- shop.bad.cand: live v1 fine in edge; a corrupt candidate v2 in edge is
-- skipped without failing the build.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F09V1', 'fnc_F09', 1, 'oci://corrupt',
  'sha256:9999999999999999999999999999999999999999999999999999999999999999',
  '{"runtime":"cobol","entrypoint":"x","pool":"broken"}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z'),
 ('fnv_F10V1', 'fnc_F10', 1, 'oci://registry.example/cand@sha256:a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1',
  'sha256:a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z'),
 ('fnv_F10V2', 'fnc_F10', 2, 'oci://corrupt-candidate',
  'sha256:a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2',
  '{"runtime":"wasm","pool":"edge"}',
  'PUBLISHED', 'prn_PUB', '2026-09-03T00:00:00Z', NULL);

-- blank.svc.fn: a webhook endpoint; its application's only signing secret
-- is blank, so none is sent.
INSERT INTO fn_versions (id, function_id, version, artifact_ref, digest, manifest, state, published_by, published_at, ready_at) VALUES
 ('fnv_F11V1', 'fnc_F11', 1, 'oci://registry.example/blank@sha256:b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1',
  'sha256:b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1',
  '{"runtime":"wasm","entrypoint":"wasi_http_incoming_handler","pool":"edge","warm":false,"limits":{"maxDurationMs":30000,"maxConcurrency":32,"wasmMemoryMb":64},"endpoints":[{"path":"/in","auth":"webhook","maxBodyBytes":1048576,"timeoutMs":30000}],"subscriptions":[],"schedules":[],"public":[],"config":[],"secrets":[],"db":[],"httpAllow":[]}',
  'READY', 'prn_PUB', '2026-09-02T00:00:00Z', '2026-09-02T00:01:00Z');

-- ── Aliases ─────────────────────────────────────────────────────────────────
INSERT INTO fn_aliases (function_id, alias, version_id, updated_by, updated_at) VALUES
 ('fnc_F01', 'live',   'fnv_F01V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F01', 'beta',   'fnv_F01V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F01', 'alpha',  'fnv_F01V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F01', 'qa',     'fnv_F01V2', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F02', 'live',   'fnv_F02V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F03', 'live',   'fnv_F03V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F04', 'live',   'fnv_F04V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F05', 'live',   'fnv_F05V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F06', 'live',   'fnv_F06V2', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F08', 'live',   'fnv_F08V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F08', 'canary', 'fnv_F08V2', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F09', 'live',   'fnv_F09V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F10', 'live',   'fnv_F10V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z'),
 ('fnc_F11', 'live',   'fnv_F11V1', 'prn_PROMOTER', '2026-09-05T00:00:00Z');

-- ── Settings ────────────────────────────────────────────────────────────────
-- Declared only: EXTRA and UNDECLARED never reach a host; TIMEOUT and
-- NEW_KEY have no value, so they are missing settings.
INSERT INTO fn_config (function_id, key, value, updated_by, updated_at) VALUES
 ('fnc_F01', 'REGION',   'eu-west-1',         'prn_PUB', '2026-09-05T00:00:00Z'),
 ('fnc_F01', 'EXTRA',    'not declared',      'prn_PUB', '2026-09-05T00:00:00Z'),
 ('fnc_F02', 'GREETING', E'café\t\x1f<a href="x">&amp;\\   \U0001F600', 'prn_PUB', '2026-09-05T00:00:00Z');

INSERT INTO fn_secrets (function_id, key, value_ref, updated_by, updated_at) VALUES
 ('fnc_F01', 'API_KEY',    'encrypted:AZkmIcsVeV8+F+fmNytRVbt2sVhawZPE/l6LCJBIPKyGCJGk6sk=', 'prn_PUB', '2026-09-05T00:00:00Z'),
 ('fnc_F01', 'QUOTED',     'encrypted:AejlAR1TSBXV8gRzerSkNqx6vW/fC/peRmTbL55tHzkl9LXB4mmfblDIKcyJidslAXi8', 'prn_PUB', '2026-09-05T00:00:00Z'),
 ('fnc_F01', 'UNDECLARED', 'encrypted:AfzOlwihZUYNhCuncH3RI3E5lY+817QzgiIa0Omnt/33Yh/eT/D9/Yk91v2rZQ==', 'prn_PUB', '2026-09-05T00:00:00Z'),
 ('fnc_F05', 'API_KEY',    'encrypted:AZkmIcsVeV8+F+fmNytRVbt2sVhawZPE/l6LCJBIPKyGCJGk6sk=', 'prn_PUB', '2026-09-05T00:00:00Z');

-- ── Public routes ───────────────────────────────────────────────────────────
INSERT INTO fn_routes (id, function_id, hostname, path_prefix, alias_prefixes, created_at) VALUES
 ('fnr_R1', 'fnc_F01', 'api.shop.example.com', '/orders', '{qa,beta}', '2026-09-05T00:00:00Z'),
 ('fnr_R2', 'fnc_F03', 'a.example.com',        '/',       '{}',        '2026-09-05T00:00:00Z'),
 ('fnr_R3', 'fnc_F01', 'api.shop.example.com', '/admin',  '{}',        '2026-09-05T00:00:00Z'),
 ('fnr_R4', 'fnc_F04', 'disabled.example.com', '/',       '{}',        '2026-09-05T00:00:00Z');

-- ── Service accounts (webhook signing secrets) ─────────────────────────────
-- app_SHOP: the oldest ACTIVE account signs; an older inactive one and a
-- newer active one do not.
INSERT INTO iam_service_accounts (id, code, name, application_id, active, wh_auth_type, wh_auth_token_ref, wh_signing_secret_ref, created_at, updated_at) VALUES
 ('sac_S1', 'ds-shop-inactive', 'Shop inactive', 'app_SHOP', false, 'BEARER_TOKEN',
  'encrypted:AVLNBu0IWDHzHUVpjd5VaaLmom96l9g2JifttjNay63eh7p7ohyKzNlOYg==',
  'encrypted:Acks3BTF9DaOE85sL+ItBUheL3q3MIJBbmn7r6Y21QAC3IXmNDKA+ELWLw==', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
 ('sac_S2', 'ds-shop-oldest', 'Shop oldest', 'app_SHOP', true, 'BEARER_TOKEN',
  'encrypted:AVLNBu0IWDHzHUVpjd5VaaLmom96l9g2JifttjNay63eh7p7ohyKzNlOYg==',
  'encrypted:AXsvS98UtHgFdtYgfI7VaZFVpHwrNdyrCHiPb/bweLB9jyz/H9ELRWDprQfOlDhg', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z'),
 ('sac_S3', 'ds-shop-newer', 'Shop newer', 'app_SHOP', true, 'BEARER_TOKEN',
  'encrypted:AVLNBu0IWDHzHUVpjd5VaaLmom96l9g2JifttjNay63eh7p7ohyKzNlOYg==',
  'encrypted:ATvNH+N9sCyAOrp+MxNDNjDZQ5V31cXHxH3gO4MkCF2V2o3skcOFZg==', '2026-03-01T00:00:00Z', '2026-03-01T00:00:00Z'),
 ('sac_S4', 'ds-blank', 'Blank', 'app_BLANK', true, 'BEARER_TOKEN',
  'encrypted:AVLNBu0IWDHzHUVpjd5VaaLmom96l9g2JifttjNay63eh7p7ohyKzNlOYg==',
  'encrypted:AQqtfad9LtWcOWyZpy16I9OdLZoSczjohfNyQ540DWA=', '2026-02-01T00:00:00Z', '2026-02-01T00:00:00Z');

-- ── Hosts (now = 2026-09-25T12:00:00Z; live window 45 s) ────────────────────
-- Two live edge hosts report versions the document no longer names (unload,
-- deduplicated and sorted); a stale edge host's reports are ignored; the
-- batch host still holds shop.move.pool#1, whose live version is in edge.
INSERT INTO fn_hosts (id, pool, state, loaded, started_at, last_heartbeat) VALUES
 ('host-edge-1', 'edge', 'ACTIVE',
  '[{"address":"shop.orders.create","version":1,"state":"LOADED"},{"address":"shop.gone.fn","version":4,"state":"LOADED"},{"address":"shop.orders.create","version":9,"state":"FAILED","error":"LOAD:WASM_INVALID"}]',
  '2026-09-25T11:00:00Z', '2026-09-25T11:59:50Z'),
 ('host-edge-2', 'edge', 'DRAINING',
  '[{"address":"shop.gone.fn","version":4,"state":"REGISTERED"},{"address":"aaa.bbb.ccc","version":1,"state":"LOADED"},{"address":"shop.orders.create","version":3,"state":"REGISTERED"}]',
  '2026-09-25T11:00:00Z', '2026-09-25T11:59:15Z'),
 ('host-edge-stale', 'edge', 'ACTIVE',
  '[{"address":"stale.only.fn","version":1,"state":"LOADED"}]',
  '2026-09-25T10:00:00Z', '2026-09-25T11:59:14Z'),
 ('host-batch-1', 'batch', 'ACTIVE',
  '[{"address":"shop.move.pool","version":1,"state":"LOADED"},{"address":"shop.move.pool","version":2,"state":"REGISTERED"}]',
  '2026-09-25T11:00:00Z', '2026-09-25T11:59:59Z');
