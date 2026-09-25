fn main() {
    // Tailwind runs as the standalone CLI (no Node). By default Topcoat
    // downloads a pinned release into the target-dir cache; offline and CI
    // builds point `TAILWIND_CLI` at a local binary instead.
    println!("cargo:rerun-if-env-changed=TAILWIND_CLI");
    // Printing any rerun-if directive replaces Cargo's default "any file in
    // the package changed" detection, so name what the stylesheet depends
    // on: the theme and every source file Tailwind scans for classes.
    println!("cargo:rerun-if-changed=styles.css");
    println!("cargo:rerun-if-changed=src");
    let tailwind = topcoat::tailwind::BuildConfig::new().input("styles.css");
    let tailwind = if std::env::var_os("TAILWIND_CLI").is_some() {
        tailwind.executable_env("TAILWIND_CLI")
    } else {
        tailwind
    };
    tailwind.render().unwrap();

    // Lucide backs the `iconify_icon!` references in the vendored components
    // and in our own UI. Downloaded once into the target-dir cache.
    topcoat::icon::iconify::BuildConfig::new()
        .icon_set("lucide")
        .stage()
        .unwrap();
}
