//! What a function's bundle may import, and the modules behind those
//! specifiers.
//!
//! A `js` function is **one ES module bundle**: everything it needs from
//! npm is bundled in (esbuild, see `templates/function-ts`). The only
//! imports left are the host's own modules, a JS projection of
//! `wit/flowcatalyst-function`, one per WIT interface:
//!
//! | specifier | WIT interface | exports |
//! |---|---|---|
//! | `flowcatalyst:function/config` | `config` | `get(key)` |
//! | `flowcatalyst:function/secrets` | `secrets` | `get(key)` |
//! | `flowcatalyst:function/log` | `log` | `log(level, message)`, `trace`…`error` |
//! | `flowcatalyst:function/events` | `events` | `emit(event)` (`emit-event`) |
//! | `flowcatalyst:function/invocation` | `invocation` | `context()` |
//! | `flowcatalyst:function` | all of them | `config`, `secrets`, `log`, `events`, `invocation` |
//!
//! A later WIT interface (e.g. `db`) becomes one more row here and one more
//! module declaration in `types/flowcatalyst-function.d.ts`. Anything else
//! (a relative path, a bare npm name, `node:*`, a URL) is refused at load
//! with `JS_IMPORT_NOT_ALLOWED`.
//!
//! **The module graph of one isolate**: the main module (the host's) first
//! imports `flowcatalyst:host/internal`, which takes the host API off
//! `globalThis` (where the bootstrap left it) and removes `Deno`,
//! `__bootstrap` and `WebAssembly` from the global object, *before* the
//! bundle's own top-level code runs. Then it imports the bundle, and binds
//! the dispatcher to the entrypoint export.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use deno_core::{
    FastString, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, ResolutionKind,
    SourceCodeCacheInfo,
};
use deno_error::JsErrorBox;
use std::collections::hash_map::DefaultHasher;
use std::future::Future;
use std::pin::Pin;

/// The bundle's own specifier.
pub const BUNDLE: &str = "function:///bundle.js";
/// The host's module around the bundle (the main module): exports the
/// dispatcher bound to the entrypoint.
pub const MAIN: &str = "function:///main.js";
/// Host-internal: seals the global object, and holds the host API for the
/// modules below. Importable only by the host's own modules.
const INTERNAL: &str = "flowcatalyst:host/internal";

const INTERNAL_SOURCE: &str = "\
const host = globalThis.__fcHost;
delete globalThis.__fcHost;
delete globalThis.Deno;
delete globalThis.__bootstrap;
delete globalThis.WebAssembly;
export default host;
export const dispatcher = host.dispatcher;
";

/// Every module a bundle may import, with its source.
pub const HOST_MODULES: [(&str, &str); 6] = [
    (
        "flowcatalyst:function/config",
        "import host from \"flowcatalyst:host/internal\";\nexport const { get } = host.config;\n",
    ),
    (
        "flowcatalyst:function/secrets",
        "import host from \"flowcatalyst:host/internal\";\nexport const { get } = host.secrets;\n",
    ),
    (
        "flowcatalyst:function/log",
        "import host from \"flowcatalyst:host/internal\";\n\
         export const { log, trace, debug, info, warn, error } = host.log;\n",
    ),
    (
        "flowcatalyst:function/events",
        "import host from \"flowcatalyst:host/internal\";\nexport const { emit } = host.events;\n",
    ),
    (
        "flowcatalyst:function/invocation",
        "import host from \"flowcatalyst:host/internal\";\nexport const { context } = host.invocation;\n",
    ),
    (
        "flowcatalyst:function",
        "import host from \"flowcatalyst:host/internal\";\n\
         export const { config, secrets, log, events, invocation } = host;\n",
    ),
];

/// The main module: seal, then the bundle's entrypoint bound to the
/// dispatcher.
pub fn main_source(entrypoint: &str) -> String {
    format!(
        "import {{ dispatcher }} from {INTERNAL:?};\n\
         import * as fn from {BUNDLE:?};\n\
         export const invoke = dispatcher(fn, {entrypoint:?});\n"
    )
}

/// A loaded version's code: the bundle, its main module, and V8's code
/// cache for the bundle (made at load, used by every request's isolate so
/// that it does not parse the bundle again).
#[derive(Clone)]
pub struct VersionCode {
    pub bundle: Arc<str>,
    pub main: Arc<str>,
    /// Identifies the bundle's source to V8's cache check.
    pub hash: u64,
    pub code_cache: Option<Arc<[u8]>>,
}

impl VersionCode {
    pub fn new(bundle: &str, entrypoint: &str) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        bundle.hash(&mut hasher);
        Self {
            bundle: bundle.into(),
            main: main_source(entrypoint).into(),
            hash: hasher.finish(),
            code_cache: None,
        }
    }

    /// Bytes this version holds while loaded.
    pub fn held_bytes(&self) -> usize {
        self.bundle.len() + self.main.len() + self.code_cache.as_ref().map_or(0, |c| c.len())
    }
}

/// One isolate's loader: the bundle, the main module and the host modules,
/// nothing else. The first refused import is recorded, so the load can say
/// `JS_IMPORT_NOT_ALLOWED` rather than `JS_INVALID`; a code cache V8 makes
/// for the bundle is kept for the version.
pub struct FunctionModules {
    code: VersionCode,
    refused: RefCell<Option<String>>,
    made_cache: RefCell<Option<Vec<u8>>>,
}

impl FunctionModules {
    pub fn new(code: VersionCode) -> Rc<Self> {
        Rc::new(Self {
            code,
            refused: RefCell::new(None),
            made_cache: RefCell::new(None),
        })
    }

    /// The first import refused, if any.
    pub fn refused(&self) -> Option<String> {
        self.refused.borrow().clone()
    }

    /// The code cache V8 made for the bundle (when it was given none).
    pub fn made_cache(&self) -> Option<Vec<u8>> {
        self.made_cache.borrow_mut().take()
    }

    fn source(&self, specifier: &str) -> Option<FastString> {
        match specifier {
            BUNDLE => Some(self.code.bundle.clone().into()),
            MAIN => Some(self.code.main.clone().into()),
            INTERNAL => Some(FastString::from_static(INTERNAL_SOURCE)),
            other => HOST_MODULES
                .iter()
                .find(|(name, _)| *name == other)
                .map(|(_, source)| FastString::from_static(source)),
        }
    }
}

/// The refusal message: what was imported, and what may be.
pub fn not_allowed(specifier: &str) -> String {
    let allowed: Vec<&str> = HOST_MODULES.iter().map(|(name, _)| *name).collect();
    format!(
        "import of '{specifier}' is not allowed: a function is one ES module bundle, which may \
         import only {}",
        allowed.join(", ")
    )
}

fn is_host_module(specifier: &str) -> bool {
    HOST_MODULES.iter().any(|(name, _)| *name == specifier)
}

impl ModuleLoader for FunctionModules {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        let internal_ok = specifier == INTERNAL && (referrer == MAIN || is_host_module(referrer));
        let known =
            specifier == BUNDLE || specifier == MAIN || internal_ok || is_host_module(specifier);
        if !known {
            let message = not_allowed(specifier);
            self.refused.borrow_mut().get_or_insert(message.clone());
            return Err(JsErrorBox::type_error(message));
        }
        ModuleSpecifier::parse(specifier)
            .map_err(|e| JsErrorBox::type_error(format!("{specifier}: {e}")))
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        _referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let result = match self.source(specifier.as_str()) {
            Some(code) => {
                let code_cache = (specifier.as_str() == BUNDLE).then(|| SourceCodeCacheInfo {
                    hash: self.code.hash,
                    data: self
                        .code
                        .code_cache
                        .as_ref()
                        .map(|c| Cow::Owned(c.to_vec())),
                });
                Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(code),
                    specifier,
                    code_cache,
                ))
            }
            None => Err(JsErrorBox::type_error(not_allowed(specifier.as_str()))),
        };
        ModuleLoadResponse::Sync(result)
    }

    fn code_cache_ready(
        &self,
        specifier: ModuleSpecifier,
        hash: u64,
        code_cache: &[u8],
    ) -> Pin<Box<dyn Future<Output = ()>>> {
        if specifier.as_str() == BUNDLE && hash == self.code.hash {
            *self.made_cache.borrow_mut() = Some(code_cache.to_vec());
        }
        Box::pin(async {})
    }
}
