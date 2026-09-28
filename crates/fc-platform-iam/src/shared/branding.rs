//! The branded layout of the platform's link emails (Go
//! `internal/platform/branding/theme.go`): the login theme — logo, colours,
//! brand name — read from platform config and applied to a table-based,
//! inline-styled HTML email, so the reset, invite and portal emails look as
//! they did from Go.
//!
//! The theme lives at (`platform`, `login`, `theme`, GLOBAL), the row
//! `GET /api/public/login-theme` reads; every field is optional and falls
//! back to the SPA's defaults. The brand name is the configured platform name
//! (`platform` / `branding` / `platform-name`) unless the theme names one.

use std::sync::{Arc, LazyLock};

use base64::Engine;
use serde::Deserialize;

use crate::mfa::notify::PlatformName;
use crate::platform_config::repository::PlatformConfigRepository;

/// Go `DefaultPrimaryColor` (the SPA's `loginTheme.ts` default).
pub const DEFAULT_PRIMARY_COLOR: &str = "#102a43";
/// Go `DefaultAccentColor`.
pub const DEFAULT_ACCENT_COLOR: &str = "#0967d2";

/// Hex (`#rgb` … `#rrggbbaa`) or `rgb()` / `rgba()` only, so a configured
/// colour can't break out of the inline style it is placed in (Go
/// `colorPattern`).
static COLOR: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)^#[0-9a-f]{3,8}$|^rgba?\([0-9.,%\s]+\)$").expect("colour pattern")
});

/// The resolved branding of a transactional email (Go `branding.Theme`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub brand_name: String,
    pub primary_color: String,
    pub accent_color: String,
    /// A hosted logo image URL; empty when unset.
    pub logo_url: String,
    /// Raw SVG markup; empty when unset.
    pub logo_svg: String,
    pub footer_text: String,
}

/// The stored theme's fields the emails use (Go `rawTheme`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTheme {
    brand_name: Option<String>,
    primary_color: Option<String>,
    accent_color: Option<String>,
    logo_url: Option<String>,
    logo_svg: Option<String>,
    footer_text: Option<String>,
}

fn safe_color(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if COLOR.is_match(value) {
        value.to_string()
    } else {
        fallback.to_string()
    }
}

/// Go `html.EscapeString`.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&#34;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

impl Theme {
    /// The defaults under `brand_name`.
    pub fn defaults(brand_name: impl Into<String>) -> Self {
        Self {
            brand_name: brand_name.into(),
            primary_color: DEFAULT_PRIMARY_COLOR.to_string(),
            accent_color: DEFAULT_ACCENT_COLOR.to_string(),
            logo_url: String::new(),
            logo_svg: String::new(),
            footer_text: String::new(),
        }
    }

    /// The stored theme layered over the defaults (Go `LoadTheme`). A
    /// missing, unreadable or malformed row leaves the defaults; `None`
    /// configs give the defaults under the default platform name.
    pub async fn load(configs: Option<&Arc<PlatformConfigRepository>>) -> Self {
        let brand = PlatformName {
            configs: configs.cloned(),
        }
        .resolve()
        .await;
        let mut theme = Self::defaults(brand);
        let Some(configs) = configs else {
            return theme;
        };
        let stored = match configs
            .find_by_key("platform", "login", "theme", "GLOBAL", None)
            .await
        {
            Ok(Some(c)) if !c.value.trim().is_empty() => c.value,
            Ok(_) => return theme,
            Err(e) => {
                tracing::warn!(error = %e, "branding: login-theme lookup failed");
                return theme;
            }
        };
        let raw: RawTheme = match serde_json::from_str(&stored) {
            Ok(raw) => raw,
            Err(e) => {
                tracing::warn!(error = %e, "branding: login-theme is not valid JSON");
                return theme;
            }
        };
        if let Some(name) = raw.brand_name.as_deref().map(str::trim) {
            if !name.is_empty() {
                theme.brand_name = name.to_string();
            }
        }
        if let Some(c) = raw.primary_color {
            theme.primary_color = safe_color(&c, &theme.primary_color);
        }
        if let Some(c) = raw.accent_color {
            theme.accent_color = safe_color(&c, &theme.accent_color);
        }
        if let Some(u) = raw.logo_url {
            theme.logo_url = u.trim().to_string();
        }
        if let Some(s) = raw.logo_svg {
            theme.logo_svg = s.trim().to_string();
        }
        if let Some(f) = raw.footer_text {
            theme.footer_text = f.trim().to_string();
        }
        theme
    }

    /// The neutral portal framing Go's portal emails use: brand text
    /// "Portal", this theme's colours, no logo.
    pub fn portal(&self) -> Self {
        Self {
            brand_name: "Portal".to_string(),
            primary_color: self.primary_color.clone(),
            accent_color: self.accent_color.clone(),
            logo_url: String::new(),
            logo_svg: String::new(),
            footer_text: "This is an automated message. Please do not reply to this email."
                .to_string(),
        }
    }

    /// The banner's `<img src>` (Go `LogoSrc`): an http(s) or `data:image/`
    /// logo URL wins, else the SVG as a base64 data URI, else empty.
    pub fn logo_src(&self) -> String {
        let url = self.logo_url.trim();
        if !url.is_empty() {
            let lower = url.to_lowercase();
            if lower.starts_with("https://")
                || lower.starts_with("http://")
                || lower.starts_with("data:image/")
            {
                return url.to_string();
            }
        }
        let svg = self.logo_svg.trim();
        if svg.is_empty() {
            return String::new();
        }
        format!(
            "data:image/svg+xml;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(svg)
        )
    }

    /// Go `RenderEmail`: `content` in the branded layout — a header banner
    /// (the logo when set, else the brand name in white), an accent button
    /// with a plain-link fallback, and a footer. All text is escaped; the
    /// colours were validated on load.
    pub fn render_email(&self, content: &EmailContent<'_>) -> String {
        let src = self.logo_src();
        let header = if src.is_empty() {
            format!(
                "<span style=\"color:#ffffff;font-size:20px;font-weight:700;\">{}</span>",
                escape(&self.brand_name)
            )
        } else {
            format!(
                "<img src=\"{}\" alt=\"{}\" height=\"40\" style=\"height:40px;max-height:40px;\
                 display:block;margin:0 auto;border:0;outline:none;text-decoration:none;\" />",
                escape(&src),
                escape(&self.brand_name)
            )
        };
        let (primary, accent) = (&self.primary_color, &self.accent_color);

        let mut body = String::new();
        if !content.heading.is_empty() {
            body.push_str(&format!(
                "<h1 style=\"margin:0 0 16px;font-size:22px;font-weight:700;color:{primary};\">{}</h1>",
                escape(content.heading)
            ));
        }
        if !content.intro.is_empty() {
            body.push_str(&format!(
                "<p style=\"margin:0 0 24px;font-size:15px;line-height:1.6;color:#33475b;\">{}</p>",
                escape(content.intro)
            ));
        }
        if !content.button_label.is_empty() && !content.button_url.is_empty() {
            let url = escape(content.button_url);
            body.push_str(&format!(
                "<table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin:0 0 24px;\"><tr>\
                 <td style=\"border-radius:6px;background-color:{accent};\">\
                 <a href=\"{url}\" style=\"display:inline-block;padding:12px 28px;font-size:15px;\
                 font-weight:600;color:#ffffff;text-decoration:none;border-radius:6px;\">{label}</a></td></tr></table>\
                 <p style=\"margin:0 0 24px;font-size:13px;line-height:1.6;color:#62748b;\">\
                 Or paste this link into your browser:<br>\
                 <a href=\"{url}\" style=\"color:{accent};word-break:break-all;\">{url}</a></p>",
                label = escape(content.button_label),
            ));
        }
        for p in content.after_button.iter().filter(|p| !p.trim().is_empty()) {
            body.push_str(&format!(
                "<p style=\"margin:0 0 16px;font-size:14px;line-height:1.6;color:#33475b;\">{}</p>",
                escape(p)
            ));
        }

        let footer = match content.footer.trim() {
            "" => format!(
                "This is an automated message from {}. Please do not reply to this email.",
                self.brand_name
            ),
            f => f.to_string(),
        };

        format!(
            "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
             <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>\
             <body style=\"margin:0;padding:0;background-color:#f4f6f8;\">\
             <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" \
             style=\"background-color:#f4f6f8;padding:24px 0;\"><tr><td align=\"center\">\
             <table role=\"presentation\" width=\"600\" cellpadding=\"0\" cellspacing=\"0\" \
             style=\"width:600px;max-width:600px;background-color:#ffffff;border-radius:10px;\
             overflow:hidden;border:1px solid #e3e8ee;\">\
             <tr><td style=\"background-color:{primary};padding:24px 32px;text-align:center;\">\
             {header}</td></tr>\
             <tr><td style=\"padding:32px;font-family:Arial,Helvetica,sans-serif;\">{body}</td></tr>\
             <tr><td style=\"padding:20px 32px;background-color:#f4f6f8;border-top:1px solid #e3e8ee;\
             font-family:Arial,Helvetica,sans-serif;font-size:12px;line-height:1.5;color:#8a94a6;text-align:center;\">\
             {footer}</td></tr>\
             </table></td></tr></table></body></html>",
            footer = escape(&footer),
        )
    }
}

/// The body of a branded email (Go `branding.EmailContent`).
#[derive(Debug, Default)]
pub struct EmailContent<'a> {
    pub heading: &'a str,
    pub intro: &'a str,
    pub button_label: &'a str,
    pub button_url: &'a str,
    pub after_button: &'a [&'a str],
    /// Small print; empty means the automated-message note.
    pub footer: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(url: &str) -> EmailContent<'_> {
        EmailContent {
            heading: "Reset your password",
            intro: "We received a request.",
            button_label: "Reset password",
            button_url: url,
            after_button: &["This link expires in 15 minutes.", "  "],
            footer: "",
        }
    }

    /// Byte for byte what Go's `Theme{BrandName: "FlowCatalyst", …defaults}
    /// .RenderEmail(…)` produces for the same content.
    #[test]
    fn the_default_layout_matches_go() {
        let html = Theme::defaults("FlowCatalyst").render_email(&content("https://x/r?token=a&b"));
        let go = "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head><body style=\"margin:0;padding:0;background-color:#f4f6f8;\"><table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" style=\"background-color:#f4f6f8;padding:24px 0;\"><tr><td align=\"center\"><table role=\"presentation\" width=\"600\" cellpadding=\"0\" cellspacing=\"0\" style=\"width:600px;max-width:600px;background-color:#ffffff;border-radius:10px;overflow:hidden;border:1px solid #e3e8ee;\"><tr><td style=\"background-color:#102a43;padding:24px 32px;text-align:center;\"><span style=\"color:#ffffff;font-size:20px;font-weight:700;\">FlowCatalyst</span></td></tr><tr><td style=\"padding:32px;font-family:Arial,Helvetica,sans-serif;\"><h1 style=\"margin:0 0 16px;font-size:22px;font-weight:700;color:#102a43;\">Reset your password</h1><p style=\"margin:0 0 24px;font-size:15px;line-height:1.6;color:#33475b;\">We received a request.</p><table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin:0 0 24px;\"><tr><td style=\"border-radius:6px;background-color:#0967d2;\"><a href=\"https://x/r?token=a&amp;b\" style=\"display:inline-block;padding:12px 28px;font-size:15px;font-weight:600;color:#ffffff;text-decoration:none;border-radius:6px;\">Reset password</a></td></tr></table><p style=\"margin:0 0 24px;font-size:13px;line-height:1.6;color:#62748b;\">Or paste this link into your browser:<br><a href=\"https://x/r?token=a&amp;b\" style=\"color:#0967d2;word-break:break-all;\">https://x/r?token=a&amp;b</a></p><p style=\"margin:0 0 16px;font-size:14px;line-height:1.6;color:#33475b;\">This link expires in 15 minutes.</p></td></tr><tr><td style=\"padding:20px 32px;background-color:#f4f6f8;border-top:1px solid #e3e8ee;font-family:Arial,Helvetica,sans-serif;font-size:12px;line-height:1.5;color:#8a94a6;text-align:center;\">This is an automated message from FlowCatalyst. Please do not reply to this email.</td></tr></table></td></tr></table></body></html>";
        assert_eq!(html, go);
    }

    #[test]
    fn a_logo_replaces_the_brand_text_and_colours_are_validated() {
        let mut theme = Theme::defaults("Acme");
        theme.logo_svg = "<svg/>".to_string();
        assert_eq!(theme.logo_src(), "data:image/svg+xml;base64,PHN2Zy8+");
        theme.logo_url = "javascript:alert(1)".to_string();
        assert_eq!(
            theme.logo_src(),
            "data:image/svg+xml;base64,PHN2Zy8+",
            "a non-http logo URL falls back to the SVG"
        );
        theme.logo_url = "https://cdn.acme.test/logo.png".to_string();
        let html = theme.render_email(&content("https://x"));
        assert!(
            html.contains("<img src=\"https://cdn.acme.test/logo.png\" alt=\"Acme\" height=\"40\"")
        );
        assert!(!html.contains(">Acme</span>"));

        assert_eq!(safe_color(" #AbC ", "#000"), "#AbC");
        assert_eq!(
            safe_color("rgba(1, 2, 3, 0.5)", "#000"),
            "rgba(1, 2, 3, 0.5)"
        );
        assert_eq!(safe_color("red;background:url(x)", "#000"), "#000");
    }

    #[test]
    fn the_portal_framing_keeps_the_colours_only() {
        let mut theme = Theme::defaults("Acme");
        theme.primary_color = "#111111".to_string();
        theme.logo_url = "https://cdn.acme.test/logo.png".to_string();
        let portal = theme.portal();
        assert_eq!(portal.brand_name, "Portal");
        assert_eq!(portal.primary_color, "#111111");
        assert_eq!(portal.logo_src(), "");
        let html = portal.render_email(&content("https://x"));
        assert!(html.contains(">Portal</span>"));
        assert!(html.contains("This is an automated message from Portal."));
    }
}
