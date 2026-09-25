//! The portal plane's emails (Go `passwordreset/api.linkEmailer`'s portal
//! templates, `branding.Theme.RenderEmail`, and `notify.PortalPasswordChanged`).
//!
//! Portal emails use neutral framing: brand text "Portal", no logo, no 2FA
//! copy. Go takes the colours from the platform's login theme; Rust has no
//! login-theme store yet, so the theme defaults apply (Go's
//! `DefaultPrimaryColor` / `DefaultAccentColor`).

use crate::shared::email_service::EmailMessage;

const PRIMARY_COLOR: &str = "#102a43";
const ACCENT_COLOR: &str = "#0967d2";
const BRAND_NAME: &str = "Portal";

struct EmailContent<'a> {
    heading: &'a str,
    intro: &'a str,
    button_label: &'a str,
    button_url: &'a str,
    after_button: &'a [&'a str],
}

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

/// Go `Theme.RenderEmail`: a table-based, inline-styled layout.
fn render(c: &EmailContent<'_>) -> String {
    let header = format!(
        "<span style=\"color:#ffffff;font-size:20px;font-weight:700;\">{}</span>",
        escape(BRAND_NAME)
    );
    let mut body = String::new();
    if !c.heading.is_empty() {
        body.push_str(&format!(
            "<h1 style=\"margin:0 0 16px;font-size:22px;font-weight:700;color:{PRIMARY_COLOR};\">{}</h1>",
            escape(c.heading)
        ));
    }
    if !c.intro.is_empty() {
        body.push_str(&format!(
            "<p style=\"margin:0 0 24px;font-size:15px;line-height:1.6;color:#33475b;\">{}</p>",
            escape(c.intro)
        ));
    }
    if !c.button_label.is_empty() && !c.button_url.is_empty() {
        let url = escape(c.button_url);
        body.push_str(&format!(
            "<table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin:0 0 24px;\"><tr>\
             <td style=\"border-radius:6px;background-color:{ACCENT_COLOR};\">\
             <a href=\"{url}\" style=\"display:inline-block;padding:12px 28px;font-size:15px;\
             font-weight:600;color:#ffffff;text-decoration:none;border-radius:6px;\">{label}</a></td></tr></table>\
             <p style=\"margin:0 0 24px;font-size:13px;line-height:1.6;color:#62748b;\">\
             Or paste this link into your browser:<br>\
             <a href=\"{url}\" style=\"color:{ACCENT_COLOR};word-break:break-all;\">{url}</a></p>",
            label = escape(c.button_label),
        ));
    }
    for p in c.after_button.iter().filter(|p| !p.trim().is_empty()) {
        body.push_str(&format!(
            "<p style=\"margin:0 0 16px;font-size:14px;line-height:1.6;color:#33475b;\">{}</p>",
            escape(p)
        ));
    }
    let footer = format!(
        "This is an automated message from {BRAND_NAME}. Please do not reply to this email."
    );
    format!(
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>\
         <body style=\"margin:0;padding:0;background-color:#f4f6f8;\">\
         <table role=\"presentation\" width=\"100%\" cellpadding=\"0\" cellspacing=\"0\" \
         style=\"background-color:#f4f6f8;padding:24px 0;\"><tr><td align=\"center\">\
         <table role=\"presentation\" width=\"600\" cellpadding=\"0\" cellspacing=\"0\" \
         style=\"width:600px;max-width:600px;background-color:#ffffff;border-radius:10px;\
         overflow:hidden;border:1px solid #e3e8ee;\">\
         <tr><td style=\"background-color:{PRIMARY_COLOR};padding:24px 32px;text-align:center;\">\
         {header}</td></tr>\
         <tr><td style=\"padding:32px;font-family:Arial,Helvetica,sans-serif;\">{body}</td></tr>\
         <tr><td style=\"padding:20px 32px;background-color:#f4f6f8;border-top:1px solid #e3e8ee;\
         font-family:Arial,Helvetica,sans-serif;font-size:12px;line-height:1.5;color:#8a94a6;text-align:center;\">\
         {footer}</td></tr>\
         </table></td></tr></table></body></html>",
        footer = escape(&footer),
    )
}

/// Go `SendPortalInviteLink`: the set-password invite.
pub fn invite_link(to: &str, invite_link: &str) -> EmailMessage {
    EmailMessage {
        to: to.to_string(),
        subject: "Join the portal".to_string(),
        html_body: render(&EmailContent {
            heading: "You've been invited to the portal",
            intro: "A portal account has been created for you. Click the button below to choose a password and sign in.",
            button_label: "Join the portal",
            button_url: invite_link,
            after_button: &["This link expires in 72 hours."],
        }),
        text_body: None,
    }
}

/// Go `SendPortalResetLink`: the forgot-password email.
pub fn reset_link(to: &str, reset_link: &str) -> EmailMessage {
    EmailMessage {
        to: to.to_string(),
        subject: "Reset your password".to_string(),
        html_body: render(&EmailContent {
            heading: "Reset your password",
            intro: "We received a request to reset your portal password. Click the button below to choose a new one.",
            button_label: "Reset password",
            button_url: reset_link,
            after_button: &[
                "This link expires in 15 minutes.",
                "If you didn't request this, you can safely ignore this email.",
            ],
        }),
        text_body: None,
    }
}

/// Go `SendPortalSSOInvite`: no password to set — the button opens the
/// portal, whose login routes the user to their organisation's sign-in.
pub fn sso_invite(to: &str, portal_url: &str) -> EmailMessage {
    EmailMessage {
        to: to.to_string(),
        subject: "You've been invited".to_string(),
        html_body: render(&EmailContent {
            heading: "You've been invited",
            intro: "You've been given access to a customer portal. Open it and sign in with your organisation account — no password setup needed.",
            button_label: "Open the portal",
            button_url: portal_url,
            after_button: &[],
        }),
        text_body: None,
    }
}

/// Go `notify.PortalPasswordChanged`.
pub fn password_changed(to: &str) -> EmailMessage {
    EmailMessage {
        to: to.to_string(),
        subject: "Your portal password was changed".to_string(),
        html_body: "<p>Your portal password was just changed.</p>\
                    <p style=\"color:#888;font-size:12px\">If this wasn't you, \
                    contact your administrator immediately.</p>"
            .to_string(),
        text_body: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_escaped_and_framing_is_neutral() {
        let m = invite_link("a@b.c", "https://x/auth/set-password?token=a&b");
        assert_eq!(m.subject, "Join the portal");
        assert!(m.html_body.contains("token=a&amp;b"));
        assert!(m.html_body.contains(">Portal</span>"));
        assert!(m.html_body.contains("This link expires in 72 hours."));
    }
}
