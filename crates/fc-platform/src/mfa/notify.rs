//! Best-effort security notifications (Go `internal/platform/notify`): "your
//! password changed", "2FA was reset", "a recovery code was used", and so
//! on. A delivery failure is logged, never returned, so a notification can
//! never block the security action that triggered it.

use std::sync::Arc;

use tracing::warn;

use crate::shared::email_service::{EmailMessage, EmailService};
use crate::PlatformConfigRepository;

/// The brand name when none is configured (Go `branding.DefaultPlatformName`).
pub const DEFAULT_PLATFORM_NAME: &str = "FlowCatalyst";

const FOOTER: &str = "<p style=\"color:#888;font-size:12px\">If this wasn't you, \
                      contact your administrator immediately.</p>";

/// Resolves the configured platform name (`platform` / `branding` /
/// `platform-name`, global scope), re-read on every use so a change applies
/// at once (Go `branding.PlatformName`).
#[derive(Clone)]
pub struct PlatformName {
    pub configs: Option<Arc<PlatformConfigRepository>>,
}

impl PlatformName {
    pub async fn resolve(&self) -> String {
        let Some(configs) = &self.configs else {
            return DEFAULT_PLATFORM_NAME.to_string();
        };
        match configs
            .find_by_key("platform", "branding", "platform-name", "GLOBAL", None)
            .await
        {
            Ok(Some(c)) if !c.value.trim().is_empty() => c.value.trim().to_string(),
            _ => DEFAULT_PLATFORM_NAME.to_string(),
        }
    }
}

/// Sends the security notifications.
#[derive(Clone)]
pub struct Notifier {
    pub email: Arc<dyn EmailService>,
    pub name: PlatformName,
}

/// HTML-escape user-controlled text (a device label is the browser's
/// User-Agent). Go interpolates it raw; escaping only changes what a
/// hostile User-Agent can inject.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn method_label(method: &str) -> &str {
    match method {
        "TOTP" => "authenticator app",
        "EMAIL_PIN" => "email code",
        other => other,
    }
}

impl Notifier {
    async fn send(&self, to: &str, subject: &str, body: String) {
        if to.is_empty() {
            return;
        }
        let message = EmailMessage {
            to: to.to_string(),
            subject: subject.to_string(),
            html_body: body,
            text_body: None,
        };
        if let Err(e) = self.email.send(&message).await {
            warn!(to, subject, error = %e, "security notification not delivered");
        }
    }

    /// Welcome a user created with a password (Go `AccountCreated`).
    pub async fn account_created(&self, to: &str) {
        let name = self.name.resolve().await;
        self.send(
            to,
            "Your account has been created",
            format!(
                "<p>Your {name} account has been created.</p>\
                 <p>Sign in to get started. If two-factor authentication is required \
                 for your organisation, you'll be guided through setting it up.</p>"
            ),
        )
        .await;
    }

    pub async fn password_changed(&self, to: &str) {
        let name = self.name.resolve().await;
        self.send(
            to,
            "Your password was changed",
            format!("<p>Your {name} password was just changed.</p>{FOOTER}"),
        )
        .await;
    }

    pub async fn two_factor_enrolled(&self, to: &str, method: &str) {
        self.send(
            to,
            "Two-factor authentication enabled",
            format!(
                "<p>A new two-factor method ({}) was added to your account.</p>{FOOTER}",
                method_label(method)
            ),
        )
        .await;
    }

    pub async fn two_factor_method_removed(&self, to: &str, method: &str) {
        self.send(
            to,
            "Two-factor method removed",
            format!(
                "<p>A two-factor method ({}) was removed from your account.</p>{FOOTER}",
                method_label(method)
            ),
        )
        .await;
    }

    pub async fn two_factor_reset(&self, to: &str) {
        self.send(
            to,
            "Two-factor authentication was reset",
            format!(
                "<p>Your two-factor authentication has been reset. You'll be asked to set \
                 it up again the next time you sign in.</p>{FOOTER}"
            ),
        )
        .await;
    }

    pub async fn recovery_codes_regenerated(&self, to: &str) {
        self.send(
            to,
            "New recovery codes generated",
            format!(
                "<p>A new set of two-factor recovery codes was generated for your account. \
                 Your previous codes no longer work.</p>{FOOTER}"
            ),
        )
        .await;
    }

    pub async fn recovery_code_used(&self, to: &str) {
        self.send(
            to,
            "A recovery code was used to sign in",
            format!(
                "<p>One of your two-factor recovery codes was just used to sign in.</p>{FOOTER}"
            ),
        )
        .await;
    }

    pub async fn new_trusted_device(&self, to: &str, label: &str) {
        let mut body =
            "<p>A device was just remembered so it can skip two-factor prompts.</p>".to_string();
        if !label.is_empty() {
            body.push_str(&format!(
                "<p style=\"color:#555\">{}</p>",
                escape_html(label)
            ));
        }
        body.push_str(FOOTER);
        self.send(to, "A new device was remembered", body).await;
    }

    pub async fn reset_approval_needed(&self, to: &str, link: &str) {
        self.send(
            to,
            "A password reset needs your approval",
            format!(
                "<p>A user in your organisation has requested a password reset but has no \
                 authenticator or passkey on file, so it needs an administrator to \
                 approve it.</p><p><a href=\"{}\">Review the request</a></p>",
                escape_html(link)
            ),
        )
        .await;
    }
}
