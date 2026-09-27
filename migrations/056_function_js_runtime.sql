-- Owner decisions 6 and 27 (2026-09-25): the function contract gains
-- `runtime: js` (a single ES module bundle run in a V8 isolate by the Rust
-- function host), alongside `jvm`, `wasm` and `component` (037).
--
-- Only the CHECK widens. `fn_hosts.runtimes` (037) already carries whatever
-- a host reports, so a host with the JS runtime reports ["component","js",
-- "wasm"] with no schema change.

ALTER TABLE fn_functions DROP CONSTRAINT IF EXISTS fn_functions_runtime_check;
ALTER TABLE fn_functions ADD CONSTRAINT fn_functions_runtime_check
    CHECK (runtime IN ('JVM', 'WASM', 'COMPONENT', 'JS'));
