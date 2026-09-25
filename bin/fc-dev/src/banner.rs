//! Pretty startup banner — version, upgrade status, important URLs.
//!
//! Printed to stderr (so it sits alongside log lines without polluting any
//! stdout JSON output the dev server might emit). ANSI colors are only
//! emitted to TTYs; piping fc-dev into a file or `tee` gets plain text.

use std::io::{IsTerminal, Write};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Box width (visible cells, excluding the surrounding border chars).
/// Picked so the longest content row — the "upgrade available" status
/// line — still has a few cells of right-hand padding.
const INNER: usize = 60;

/// `fn_ports`: the function host's private and public ports, when it runs.
pub fn print(api_port: u16, metrics_port: u16, fn_ports: Option<(u16, u16)>) {
    let upgrade = crate::version_check::cached_upgrade_available();
    let color = std::io::stderr().is_terminal();
    let mut out = std::io::stderr().lock();
    let _ = render(
        &mut out,
        api_port,
        metrics_port,
        fn_ports,
        upgrade.as_deref(),
        color,
    );
}

fn render<W: Write>(
    out: &mut W,
    api_port: u16,
    metrics_port: u16,
    fn_ports: Option<(u16, u16)>,
    upgrade: Option<&str>,
    color: bool,
) -> std::io::Result<()> {
    let c = Colors::new(color);

    // ─── top border with title ──────────────────────────────────────────
    let title = " FlowCatalyst Dev ";
    let dashes_left = 2;
    let dashes_right = INNER.saturating_sub(dashes_left + title.len());
    writeln!(
        out,
        "{}┌{}{}{}{}{}┐{}",
        c.dim,
        "─".repeat(dashes_left),
        c.reset,
        c.bold,
        title,
        format_args!("{}{}", c.reset, c.dim).to_string() + &"─".repeat(dashes_right),
        c.reset,
    )?;

    // ─── rows ──────────────────────────────────────────────────────────
    row(out, "version", VERSION, c.cyan, &c)?;
    match upgrade {
        Some(latest) => {
            let status = format!("↑ v{latest} available — run `fc-dev upgrade`");
            row(out, "status", &status, c.yellow, &c)?;
        }
        None => {
            row(out, "status", "✓ up to date", c.green, &c)?;
        }
    }
    row(
        out,
        "api",
        &format!("http://localhost:{api_port}"),
        c.plain,
        &c,
    )?;
    row(
        out,
        "metrics",
        &format!("http://localhost:{metrics_port}/metrics"),
        c.plain,
        &c,
    )?;
    if let Some((fn_port, public_port)) = fn_ports {
        row(
            out,
            "functions",
            &format!("http://127.0.0.1:{fn_port}/functions/…"),
            c.plain,
            &c,
        )?;
        row(
            out,
            "fn public",
            &format!("http://<name>.localhost:{public_port}/"),
            c.plain,
            &c,
        )?;
    }

    // ─── bottom border ─────────────────────────────────────────────────
    writeln!(out, "{}└{}┘{}", c.dim, "─".repeat(INNER), c.reset,)?;
    writeln!(out)?;
    Ok(())
}

/// Render one labelled row inside the box. Pads to `INNER` columns of
/// visible content, accounting for the unicode arrows/ticks consuming
/// one cell each.
fn row<W: Write>(
    out: &mut W,
    label: &str,
    value: &str,
    value_color: &str,
    c: &Colors,
) -> std::io::Result<()> {
    // Content layout: 2-space left margin, 11-char label column, value, padding.
    let label_col_width = 11;
    let left_margin = 2;
    let visible_value_width = display_width(value);
    let content_width = left_margin + label_col_width + visible_value_width;
    let pad_right = INNER.saturating_sub(content_width).max(1);

    writeln!(
        out,
        "{dim}│{reset}  {dim}{label:<lw$}{reset}{vc}{value}{reset}{pad}{dim}│{reset}",
        dim = c.dim,
        reset = c.reset,
        label = label,
        lw = label_col_width,
        vc = value_color,
        value = value,
        pad = " ".repeat(pad_right),
    )
}

/// Best-effort display width — counts the unicode "wide" chars we actually
/// use in this banner (↑ ✓ —) as single cells, which is what every common
/// terminal renders them as. Anything outside ASCII falls back to `chars()`,
/// not bytes, so multi-byte characters don't break alignment.
fn display_width(s: &str) -> usize {
    s.chars().count()
}

struct Colors {
    bold: &'static str,
    dim: &'static str,
    cyan: &'static str,
    green: &'static str,
    yellow: &'static str,
    plain: &'static str,
    reset: &'static str,
}

impl Colors {
    fn new(enable: bool) -> Self {
        if enable {
            Self {
                bold: "\x1b[1m",
                dim: "\x1b[2m",
                cyan: "\x1b[36m",
                green: "\x1b[32m",
                yellow: "\x1b[33m",
                plain: "",
                reset: "\x1b[0m",
            }
        } else {
            Self {
                bold: "",
                dim: "",
                cyan: "",
                green: "",
                yellow: "",
                plain: "",
                reset: "",
            }
        }
    }
}
