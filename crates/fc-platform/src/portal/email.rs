//! The portal plane's emails (Go `passwordreset/api.linkEmailer`'s portal
//! templates, `branding.Theme.RenderEmail`, and `notify.PortalPasswordChanged`).
//!
//! Portal emails use neutral framing: brand text "Portal", no logo, no 2FA
//! copy, and the platform login theme's colours ([`Theme::portal`]).

use crate::shared::branding::{EmailContent, Theme};
use crate::shared::email_service::EmailMessage;

/// Go `SendPortalInviteLink`: the set-password invite. `platform` is the
/// platform's theme; the portal framing keeps only its colours.
pub fn invite_link(platform: &Theme, to: &str, invite_link: &str) -> EmailMessage {
    EmailMessage {
        to: to.to_string(),
        subject: "Join the portal".to_string(),
        html_body: platform.portal().render_email(&EmailContent {
            heading: "You've been invited to the portal",
            intro: "A portal account has been created for you. Click the button below to choose a password and sign in.",
            button_label: "Join the portal",
            button_url: invite_link,
            after_button: &["This link expires in 72 hours."],
            ..EmailContent::default()
        }),
        text_body: None,
    }
}

/// Go `SendPortalResetLink`: the forgot-password email.
pub fn reset_link(platform: &Theme, to: &str, reset_link: &str) -> EmailMessage {
    EmailMessage {
        to: to.to_string(),
        subject: "Reset your password".to_string(),
        html_body: platform.portal().render_email(&EmailContent {
            heading: "Reset your password",
            intro: "We received a request to reset your portal password. Click the button below to choose a new one.",
            button_label: "Reset password",
            button_url: reset_link,
            after_button: &[
                "This link expires in 15 minutes.",
                "If you didn't request this, you can safely ignore this email.",
            ],
            ..EmailContent::default()
        }),
        text_body: None,
    }
}

/// Go `SendPortalSSOInvite`: no password to set — the button opens the
/// portal, whose login routes the user to their organisation's sign-in.
pub fn sso_invite(platform: &Theme, to: &str, portal_url: &str) -> EmailMessage {
    EmailMessage {
        to: to.to_string(),
        subject: "You've been invited".to_string(),
        html_body: platform.portal().render_email(&EmailContent {
            heading: "You've been invited",
            intro: "You've been given access to a customer portal. Open it and sign in with your organisation account — no password setup needed.",
            button_label: "Open the portal",
            button_url: portal_url,
            ..EmailContent::default()
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
        let mut platform = Theme::defaults("Acme");
        platform.accent_color = "#123456".to_string();
        platform.logo_url = "https://cdn.acme.test/logo.png".to_string();
        let m = invite_link(&platform, "a@b.c", "https://x/auth/set-password?token=a&b");
        assert_eq!(m.subject, "Join the portal");
        assert!(m.html_body.contains("token=a&amp;b"));
        assert!(m.html_body.contains(">Portal</span>"));
        assert!(m.html_body.contains("This link expires in 72 hours."));
        // The platform's colours, never its name or logo.
        assert!(m.html_body.contains("background-color:#123456"));
        assert!(!m.html_body.contains("Acme"));
        assert!(!m.html_body.contains("logo.png"));
    }
}
