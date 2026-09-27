//! `fn init <dir> [--lang rust|ts|js] [--runtime component|wasm|js]` (Java
//! `fn init`, `function-manifest-authoring.md` M3): scaffolds a function
//! from one of the templates compiled into fc-dev, so neither
//! `cargo generate`, npm nor a network call is needed:
//!
//! | `--lang` | template | runtime |
//! |---|---|---|
//! | `rust` (default) | `templates/function-rust` | `component` (or `wasm`) |
//! | `ts` | `templates/function-ts` | `js` |
//! | `js` | `templates/function-js` | `js` |
//!
//! The placeholders are filled in-process: `{{project-name}}` (the
//! kebab-case name) and, for Rust, `{{crate_name}}` (its snake-case form).
//! `manifest.json` gains a `$schema` pointing at the platform's manifest
//! schema.
//!
//! A Rust manifest says `runtime: component` (its entrypoint defaults to
//! `wasi:http/incoming-handler`). `--runtime wasm` writes the same component
//! as `runtime: wasm` with the `wasi_http_incoming_handler` entrypoint, for
//! a platform that predates `component` (Java's). A TypeScript or
//! JavaScript manifest says `runtime: js` (its entrypoint defaults to the
//! bundle's default export).
//!
//! Local only: it never contacts the platform. It refuses when any file it
//! would write already exists, and then writes nothing.

use std::path::{Path, PathBuf};

use super::credentials::platform_url_or_default;
use super::{CliError, Ctx, Io};

/// The Rust template, as `(relative path, contents)`. `cargo-generate.toml`
/// is `cargo generate`'s own and is not copied.
pub const TEMPLATE: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        include_str!("../../../../templates/function-rust/Cargo.toml"),
    ),
    (
        "README.md",
        include_str!("../../../../templates/function-rust/README.md"),
    ),
    (
        ".gitignore",
        include_str!("../../../../templates/function-rust/.gitignore"),
    ),
    (
        "manifest.json",
        include_str!("../../../../templates/function-rust/manifest.json"),
    ),
    (
        "src/lib.rs",
        include_str!("../../../../templates/function-rust/src/lib.rs"),
    ),
];

/// The TypeScript template (`--lang ts`).
pub const TEMPLATE_TS: &[(&str, &str)] = &[
    (
        "package.json",
        include_str!("../../../../templates/function-ts/package.json"),
    ),
    (
        "tsconfig.json",
        include_str!("../../../../templates/function-ts/tsconfig.json"),
    ),
    (
        "README.md",
        include_str!("../../../../templates/function-ts/README.md"),
    ),
    (
        ".gitignore",
        include_str!("../../../../templates/function-ts/.gitignore"),
    ),
    (
        "manifest.json",
        include_str!("../../../../templates/function-ts/manifest.json"),
    ),
    (
        "src/index.ts",
        include_str!("../../../../templates/function-ts/src/index.ts"),
    ),
    (
        "types/flowcatalyst-function.d.ts",
        include_str!("../../../../templates/function-ts/types/flowcatalyst-function.d.ts"),
    ),
];

/// The JavaScript template (`--lang js`).
pub const TEMPLATE_JS: &[(&str, &str)] = &[
    (
        "package.json",
        include_str!("../../../../templates/function-js/package.json"),
    ),
    (
        "jsconfig.json",
        include_str!("../../../../templates/function-js/jsconfig.json"),
    ),
    (
        "README.md",
        include_str!("../../../../templates/function-js/README.md"),
    ),
    (
        ".gitignore",
        include_str!("../../../../templates/function-js/.gitignore"),
    ),
    (
        "manifest.json",
        include_str!("../../../../templates/function-js/manifest.json"),
    ),
    (
        "src/index.js",
        include_str!("../../../../templates/function-js/src/index.js"),
    ),
    (
        "types/flowcatalyst-function.d.ts",
        include_str!("../../../../templates/function-js/types/flowcatalyst-function.d.ts"),
    ),
];

/// A template's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Ts,
    Js,
}

impl Lang {
    fn parse(raw: &str) -> Result<Lang, CliError> {
        match raw.to_ascii_lowercase().as_str() {
            "rust" => Ok(Lang::Rust),
            "ts" | "typescript" => Ok(Lang::Ts),
            "js" | "javascript" => Ok(Lang::Js),
            other => Err(CliError::Usage(format!(
                "--lang must be rust, ts or js, got \"{other}\""
            ))),
        }
    }

    pub fn template(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Lang::Rust => TEMPLATE,
            Lang::Ts => TEMPLATE_TS,
            Lang::Js => TEMPLATE_JS,
        }
    }

    /// The template directory, relative to the repository root.
    #[cfg(test)]
    pub fn dir(self) -> &'static str {
        match self {
            Lang::Rust => "templates/function-rust",
            Lang::Ts => "templates/function-ts",
            Lang::Js => "templates/function-js",
        }
    }
}

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// The directory to write the project into.
    pub dir: PathBuf,

    /// The function runtime: for Rust `component` (the default) or `wasm`
    /// (a platform without `component`); for TypeScript and JavaScript `js`
    /// (the only one). JVM functions are scaffolded by Java's fcdev.
    #[arg(long)]
    pub runtime: Option<String>,

    /// The language: rust, ts (TypeScript) or js (JavaScript).
    #[arg(long, default_value = "rust")]
    pub lang: String,

    /// The crate or package name (default: the directory's name).
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,

    /// Write manifest.json only.
    #[arg(long)]
    pub manifest_only: bool,

    /// Depend on a local fc-function-pdk checkout instead of the git one.
    #[arg(long, value_name = "DIR")]
    pub pdk_path: Option<PathBuf>,
}

pub fn run(ctx: &Ctx<'_>, args: &InitArgs, io: &mut Io<'_>) -> Result<i32, CliError> {
    let lang = Lang::parse(&args.lang)?;
    let runtime = args
        .runtime
        .as_deref()
        .unwrap_or(if lang == Lang::Rust {
            "component"
        } else {
            "js"
        })
        .to_ascii_lowercase();
    let as_wasm = match (lang, runtime.as_str()) {
        (_, "jvm") => {
            return Err(CliError::Usage(
                "fc-dev's function host runs wasm components and JS bundles; scaffold a JVM \
                 function with Java's `fcdev fn init --runtime jvm`"
                    .into(),
            ))
        }
        (Lang::Rust, "component") => false,
        (Lang::Rust, "wasm") => true,
        (Lang::Rust, other) => {
            return Err(CliError::Usage(format!(
                "--runtime for --lang rust must be component or wasm, got \"{other}\""
            )))
        }
        (Lang::Ts | Lang::Js, "js") => false,
        (Lang::Ts | Lang::Js, other) => {
            return Err(CliError::Usage(format!(
                "--runtime for --lang {} must be js, got \"{other}\"",
                args.lang.to_ascii_lowercase()
            )))
        }
    };
    let raw_name = match &args.name {
        Some(name) => name.clone(),
        None => default_name(&args.dir),
    };
    let project_name = project_name(&raw_name)?;
    let platform_url = platform_url_or_default(
        ctx.args.platform_url.as_deref(),
        ctx.env,
        &ctx.credentials_file(),
    );
    let pdk_path =
        match &args.pdk_path {
            Some(path) => Some(std::path::absolute(path).map_err(|e| {
                CliError::Other(format!("could not resolve {}: {e}", path.display()))
            })?),
            None => None,
        };

    let mut files = render(
        lang,
        &project_name,
        &platform_url,
        pdk_path.as_deref(),
        args.manifest_only,
    );
    if as_wasm {
        for (rel, text) in &mut files {
            if rel == "manifest.json" {
                *text = as_wasm_manifest(text);
            }
        }
    }
    let existing: Vec<String> = files
        .iter()
        .map(|(rel, _)| args.dir.join(rel))
        .filter(|p| p.exists())
        .map(|p| p.display().to_string())
        .collect();
    if !existing.is_empty() {
        writeln!(
            io.err,
            "refusing to overwrite existing file(s), nothing written: {}",
            existing.join(", ")
        )?;
        return Err(CliError::Reported);
    }
    for (rel, contents) in &files {
        let path = args.dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
    }

    if args.manifest_only {
        writeln!(io.out, "wrote {}", args.dir.join("manifest.json").display())?;
        return Ok(0);
    }
    let crate_name = project_name.replace('-', "_");
    let address = format!("<app>.default.{project_name}");
    let written: Vec<&str> = files.iter().map(|(rel, _)| rel.as_str()).collect();
    writeln!(
        io.out,
        "wrote {}: {}",
        args.dir.display(),
        written.join(", ")
    )?;
    let artifact = match lang {
        Lang::Rust => format!("target/wasm32-wasip2/release/{crate_name}.wasm"),
        Lang::Ts | Lang::Js => "dist/function.mjs".to_string(),
    };
    writeln!(io.out, "next steps:")?;
    writeln!(io.out, "  cd {}", args.dir.display())?;
    writeln!(io.out, "  fc-dev fn build")?;
    writeln!(io.out, "  fc-dev fn config set {address} GREETING=Hello")?;
    writeln!(io.out, "  fc-dev fn deploy {artifact} {address}")?;
    writeln!(io.out, "  fc-dev fn invoke {address} --path /hello/world")?;
    Ok(0)
}

/// The files to write, relative to the project directory.
pub fn render(
    lang: Lang,
    project_name: &str,
    platform_url: &str,
    pdk_path: Option<&Path>,
    manifest_only: bool,
) -> Vec<(String, String)> {
    let crate_name = project_name.replace('-', "_");
    lang.template()
        .iter()
        .filter(|(rel, _)| !manifest_only || *rel == "manifest.json")
        .map(|(rel, contents)| {
            let mut text = contents
                .replace("{{project-name}}", project_name)
                .replace("{{crate_name}}", &crate_name);
            if *rel == "manifest.json" {
                text = with_schema(&text, platform_url);
            }
            if *rel == "Cargo.toml" {
                if let Some(pdk) = pdk_path {
                    text = with_local_pdk(&text, pdk);
                }
            }
            (rel.to_string(), text)
        })
        .collect()
}

/// The template's `runtime: component` as `runtime: wasm` plus the
/// component's manifest-safe entrypoint.
fn as_wasm_manifest(manifest: &str) -> String {
    manifest.replacen(
        "\"runtime\": \"component\",",
        "\"runtime\": \"wasm\",\n  \"entrypoint\": \"wasi_http_incoming_handler\",",
        1,
    )
}

/// `"$schema"` as the manifest's first key.
fn with_schema(manifest: &str, platform_url: &str) -> String {
    let schema = format!(
        "{{\n  \"$schema\": \"{}/api/schemas/function-manifest.json\",",
        platform_url.trim_end_matches('/')
    );
    match manifest.trim_start().strip_prefix('{') {
        Some(rest) => format!("{schema}{rest}"),
        None => manifest.to_string(),
    }
}

fn with_local_pdk(cargo_toml: &str, pdk: &Path) -> String {
    let path = pdk.to_string_lossy().replace('\\', "/");
    cargo_toml
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("fc-function-pdk") {
                format!("fc-function-pdk = {{ path = \"{path}\" }}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn default_name(dir: &Path) -> String {
    std::path::absolute(dir)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "function".to_string())
}

/// cargo-generate's `project-name`: kebab-case, and a valid package name.
fn project_name(raw: &str) -> Result<String, CliError> {
    let name = raw.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    let valid = name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(name)
    } else {
        Err(CliError::Usage(format!(
            "\"{raw}\" is not a usable crate name: start with a letter, then letters, digits, - \
             or _ (pass --name)"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FnArgs, FnCommand, OutputMode};
    use super::*;
    use std::collections::BTreeSet;

    fn args(dir: &Path, extra: InitArgs) -> FnArgs {
        FnArgs {
            platform_url: Some("http://localhost:9999".into()),
            client_id: None,
            client_secret: None,
            output: OutputMode::Text,
            credentials_file: Some(dir.join("no-such-fn-cli.json")),
            command: FnCommand::Init(extra),
        }
    }

    fn init_args(dir: PathBuf) -> InitArgs {
        InitArgs {
            dir,
            runtime: None,
            lang: "rust".into(),
            name: None,
            manifest_only: false,
            pdk_path: None,
        }
    }

    fn run_init(fn_args: &FnArgs) -> (i32, String, String) {
        let (mut out, mut err, mut stdin) = (Vec::new(), Vec::new(), std::io::empty());
        let env = |_: &str| None;
        let code = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(super::super::run_with(
                fn_args,
                &env,
                &mut Io {
                    out: &mut out,
                    err: &mut err,
                    stdin: &mut stdin,
                },
            ));
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn scaffolds_the_rust_template_with_its_placeholders_filled() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Order_Mapper");
        let (code, out, err) = run_init(&args(tmp.path(), init_args(dir.clone())));
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("fc-dev fn build"), "{out}");

        let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"order-mapper\""), "{cargo}");
        assert!(cargo.contains("fc-function-pdk = { git = "), "{cargo}");
        let lib = std::fs::read_to_string(dir.join("src/lib.rs")).unwrap();
        assert!(
            lib.starts_with("//! order-mapper: a FlowCatalyst function."),
            "{lib}"
        );
        let readme = std::fs::read_to_string(dir.join("README.md")).unwrap();
        assert!(readme.contains("release/order_mapper.wasm"), "{readme}");
        assert!(dir.join(".gitignore").exists());
        for (rel, _) in TEMPLATE {
            let text = std::fs::read_to_string(dir.join(rel)).unwrap();
            assert!(
                !text.contains("{{project-name}}") && !text.contains("{{crate_name}}"),
                "{rel}"
            );
        }

        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest["$schema"],
            "http://localhost:9999/api/schemas/function-manifest.json"
        );
        assert_eq!(manifest["runtime"], "component");
        assert!(manifest.get("entrypoint").is_none(), "it defaults");
    }

    #[test]
    fn runtime_wasm_writes_the_component_under_the_entrypoint_alias() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("legacy");
        let (code, _, err) = run_init(&args(
            tmp.path(),
            InitArgs {
                runtime: Some("wasm".into()),
                ..init_args(dir.clone())
            },
        ));
        assert_eq!(code, 0, "{err}");
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["runtime"], "wasm");
        assert_eq!(manifest["entrypoint"], "wasi_http_incoming_handler");
        assert_eq!(manifest["config"], serde_json::json!(["GREETING"]));
    }

    #[test]
    fn refuses_to_overwrite_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hello");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), "{}").unwrap();
        let (code, _, err) = run_init(&args(tmp.path(), init_args(dir.clone())));
        assert_eq!(code, 1);
        assert!(err.contains("refusing to overwrite"), "{err}");
        assert!(!dir.join("Cargo.toml").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("manifest.json")).unwrap(),
            "{}"
        );
    }

    #[test]
    fn manifest_only_and_a_local_pdk() {
        let tmp = tempfile::tempdir().unwrap();
        let only = tmp.path().join("only");
        let (code, _, err) = run_init(&args(
            tmp.path(),
            InitArgs {
                manifest_only: true,
                ..init_args(only.clone())
            },
        ));
        assert_eq!(code, 0, "{err}");
        assert!(only.join("manifest.json").exists());
        assert!(!only.join("Cargo.toml").exists());

        let files = render(
            Lang::Rust,
            "x",
            "http://p",
            Some(Path::new("/src/fc-function-pdk")),
            false,
        );
        let cargo = &files.iter().find(|(r, _)| r == "Cargo.toml").unwrap().1;
        assert!(
            cargo.contains("fc-function-pdk = { path = \"/src/fc-function-pdk\" }"),
            "{cargo}"
        );
        assert!(!cargo.contains("git ="), "{cargo}");
    }

    #[test]
    fn jvm_and_other_languages_are_usage_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let jvm = InitArgs {
            runtime: Some("jvm".into()),
            ..init_args(tmp.path().join("j"))
        };
        assert_eq!(run_init(&args(tmp.path(), jvm)).0, 2);
        let go = InitArgs {
            lang: "go".into(),
            ..init_args(tmp.path().join("g"))
        };
        assert_eq!(run_init(&args(tmp.path(), go)).0, 2);
        let ts_component = InitArgs {
            lang: "ts".into(),
            runtime: Some("component".into()),
            ..init_args(tmp.path().join("t"))
        };
        assert_eq!(run_init(&args(tmp.path(), ts_component)).0, 2);
        let rust_js = InitArgs {
            runtime: Some("js".into()),
            ..init_args(tmp.path().join("r"))
        };
        assert_eq!(run_init(&args(tmp.path(), rust_js)).0, 2);
        assert!(!tmp.path().join("j").exists());
        assert!(!tmp.path().join("t").exists());
    }

    #[test]
    fn scaffolds_the_typescript_and_javascript_templates_as_runtime_js() {
        for (lang, source) in [("ts", "src/index.ts"), ("js", "src/index.js")] {
            let tmp = tempfile::tempdir().unwrap();
            let dir = tmp.path().join("Order_Mapper");
            let (code, out, err) = run_init(&args(
                tmp.path(),
                InitArgs {
                    lang: lang.into(),
                    ..init_args(dir.clone())
                },
            ));
            assert_eq!(code, 0, "{err}");
            assert!(out.contains("fc-dev fn deploy dist/function.mjs"), "{out}");
            let package: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).unwrap())
                    .unwrap();
            assert_eq!(package["name"], "order-mapper");
            assert!(package["scripts"]["build"]
                .as_str()
                .unwrap()
                .contains("--external:flowcatalyst:*"));
            let text = std::fs::read_to_string(dir.join(source)).unwrap();
            assert!(
                text.starts_with("// order-mapper: a FlowCatalyst function."),
                "{text}"
            );
            assert!(dir.join("types/flowcatalyst-function.d.ts").exists());
            let manifest: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(manifest["runtime"], "js");
            assert!(manifest.get("entrypoint").is_none(), "it defaults");
            assert_eq!(
                manifest["$schema"],
                "http://localhost:9999/api/schemas/function-manifest.json"
            );
        }
    }

    /// Each template ships the host's own declarations, byte for byte.
    #[test]
    fn the_templates_declarations_are_the_hosts() {
        let canonical =
            include_str!("../../../../crates/fc-fnhost-js/types/flowcatalyst-function.d.ts");
        for lang in [Lang::Ts, Lang::Js] {
            let copy = lang
                .template()
                .iter()
                .find(|(rel, _)| *rel == "types/flowcatalyst-function.d.ts")
                .unwrap()
                .1;
            assert_eq!(
                copy,
                canonical,
                "{}/types/flowcatalyst-function.d.ts is not the host's: copy \
                 crates/fc-fnhost-js/types/flowcatalyst-function.d.ts over it",
                lang.dir()
            );
        }
    }

    /// Every template file is embedded, and the templates use no
    /// placeholder the renderer does not fill.
    #[test]
    fn the_embedded_templates_are_the_whole_templates() {
        for lang in [Lang::Rust, Lang::Ts, Lang::Js] {
            embedded_template_is_the_whole_template(lang);
        }
    }

    fn embedded_template_is_the_whole_template(lang: Lang) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(lang.dir());
        let mut on_disk = BTreeSet::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path
                        .file_name()
                        .is_some_and(|n| n != "target" && n != "node_modules" && n != "dist")
                    {
                        stack.push(path);
                    }
                } else {
                    let rel = path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    if rel != "cargo-generate.toml"
                        && rel != "Cargo.lock"
                        && rel != "package-lock.json"
                    {
                        on_disk.insert(rel);
                    }
                }
            }
        }
        let embedded: BTreeSet<String> =
            lang.template().iter().map(|(r, _)| r.to_string()).collect();
        assert_eq!(embedded, on_disk, "{}", lang.dir());

        let mut placeholders = BTreeSet::new();
        for (_, text) in lang.template() {
            let mut rest = *text;
            while let Some(start) = rest.find("{{") {
                let after = &rest[start + 2..];
                let end = after.find("}}").unwrap_or(after.len());
                let inner = &after[..end];
                if inner
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    placeholders.insert(inner.to_string());
                }
                rest = &after[end.min(after.len())..];
            }
        }
        let expected = match lang {
            Lang::Rust => BTreeSet::from(["crate_name".to_string(), "project-name".to_string()]),
            Lang::Ts | Lang::Js => BTreeSet::from(["project-name".to_string()]),
        };
        assert_eq!(placeholders, expected, "{}", lang.dir());
    }
}
