-- Schema fingerprint: one sorted text line per table, column, index, constraint and
-- sequence of the public schema. Run with `psql -At -f` (see scripts/schema-fingerprint.sh).
--
-- Excluded on purpose: migration trackers (goose_db_version, _schema_migrations,
-- _fc_migrations), and partition CHILD tables (their names carry dates; the parent's
-- partition key is fingerprinted instead). Data is not fingerprinted. NOT NULL constraint
-- objects (PostgreSQL 18 lists them in pg_constraint) are left out: a column line already says
-- NOT NULL, and the fingerprint must read the same on every server version.
WITH rel AS (
  SELECT c.oid, c.relname, c.relkind
  FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
  WHERE n.nspname = 'public'
    AND c.relkind IN ('r', 'p')
    AND NOT c.relispartition
    AND c.relname NOT IN ('goose_db_version', '_schema_migrations', '_fc_migrations')
)
SELECT line FROM (
  SELECT format('table %s kind=%s', r.relname, r.relkind) AS line FROM rel r
  UNION ALL
  SELECT format('partition-key %s %s', r.relname, pg_get_partkeydef(r.oid))
  FROM rel r WHERE r.relkind = 'p'
  UNION ALL
  SELECT format('column %s.%s %s %s default=%s',
                r.relname, a.attname, format_type(a.atttypid, a.atttypmod),
                CASE WHEN a.attnotnull THEN 'NOT NULL' ELSE 'NULL' END,
                coalesce(regexp_replace(pg_get_expr(d.adbin, d.adrelid), '\s+', ' ', 'g'), '-'))
  FROM rel r
  JOIN pg_attribute a ON a.attrelid = r.oid AND a.attnum > 0 AND NOT a.attisdropped
  LEFT JOIN pg_attrdef d ON d.adrelid = r.oid AND d.adnum = a.attnum
  UNION ALL
  SELECT format('index %s.%s %s', r.relname, ic.relname,
                regexp_replace(pg_get_indexdef(i.indexrelid), '\s+', ' ', 'g'))
  FROM rel r
  JOIN pg_index i ON i.indrelid = r.oid
  JOIN pg_class ic ON ic.oid = i.indexrelid
  UNION ALL
  SELECT format('constraint %s.%s %s', r.relname, co.conname,
                regexp_replace(pg_get_constraintdef(co.oid), '\s+', ' ', 'g'))
  FROM rel r
  JOIN pg_constraint co ON co.conrelid = r.oid AND co.contype <> 'n'
) fp
ORDER BY line COLLATE "C";
