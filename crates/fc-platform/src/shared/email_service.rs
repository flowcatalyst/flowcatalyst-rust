//! Email Service
//!
//! Abstraction for sending emails (password reset, notifications).
//! Ships with a `LogEmailService` for development and an `SmtpEmailService`
//! that sends real emails when SMTP is configured.

use async_trait::async_trait;
use tracing::{info, warn};

/// An email message to be sent.
#[derive(Debug, Clone)]
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub html_body: String,
    pub text_body: Option<String>,
}

/// Trait for sending emails.
#[async_trait]
pub trait EmailService: Send + Sync {
    async fn send(&self, message: &EmailMessage) -> Result<(), String>;
}

/// Development email service that logs emails instead of sending them.
pub struct LogEmailService;

#[async_trait]
impl EmailService for LogEmailService {
    async fn send(&self, message: &EmailMessage) -> Result<(), String> {
        info!(
            to = %message.to,
            subject = %message.subject,
            "[DEV] Email would be sent (SMTP not configured)"
        );
        Ok(())
    }
}

/// SMTP email service for production use.
/// Configure via environment variables — Go's names and semantics
/// (flowcatalyst-go `internal/platform/shared/email`): the `FC_`-prefixed name
/// wins, a blank value counts as unset.
/// - `FC_SMTP_HOST` / `SMTP_HOST` — SMTP server hostname (unset: log-only)
/// - `FC_SMTP_PORT` / `SMTP_PORT` — SMTP server port (default: 587)
/// - `FC_SMTP_USERNAME` / `SMTP_USERNAME` — SMTP auth username (blank: no AUTH)
/// - `FC_SMTP_PASSWORD` / `SMTP_PASSWORD` — SMTP auth password
/// - `FC_SMTP_FROM` / `SMTP_FROM` — Sender (default `noreply@flowcatalyst.local`)
/// - `FC_SMTP_SECURE` / `SMTP_SECURE` — `true`/`1`/`yes`/`on`: implicit TLS
///   (e.g. :465); anything else: STARTTLS (e.g. SendGrid on :587)
pub struct SmtpEmailService {
    host: String,
    port: u16,
    username: String,
    password: String,
    from: String,
    secure: bool,
}

/// The first non-blank value among `names`, trimmed (Go `envFirst`).
fn env_first(names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| {
        std::env::var(n)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

impl SmtpEmailService {
    /// Create from environment variables. Returns None if SMTP is not configured.
    pub fn from_env() -> Option<Self> {
        let host = env_first(&["FC_SMTP_HOST", "SMTP_HOST"])?;
        let port = env_first(&["FC_SMTP_PORT", "SMTP_PORT"])
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);
        let username = env_first(&["FC_SMTP_USERNAME", "SMTP_USERNAME"]).unwrap_or_default();
        let password = env_first(&["FC_SMTP_PASSWORD", "SMTP_PASSWORD"]).unwrap_or_default();
        let from = env_first(&["FC_SMTP_FROM", "SMTP_FROM"])
            .unwrap_or_else(|| "noreply@flowcatalyst.local".to_string());
        let secure = matches!(
            env_first(&["FC_SMTP_SECURE", "SMTP_SECURE"])
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "true" | "1" | "yes" | "on"
        );

        info!(host = %host, port, from = %from, secure, "SMTP email service configured");
        Some(Self {
            host,
            port,
            username,
            password,
            from,
            secure,
        })
    }
}

#[async_trait]
impl EmailService for SmtpEmailService {
    async fn send(&self, message: &EmailMessage) -> Result<(), String> {
        use lettre::{
            message::{header::ContentType, Mailbox},
            transport::smtp::authentication::Credentials,
            AsyncSmtpTransport, AsyncTransport, Message as LettreMessage, Tokio1Executor,
        };

        let from_mailbox: Mailbox = self
            .from
            .parse()
            .map_err(|e| format!("Invalid from address: {}", e))?;
        let to_mailbox: Mailbox = message
            .to
            .parse()
            .map_err(|e| format!("Invalid to address: {}", e))?;

        let email = LettreMessage::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(&message.subject)
            .header(ContentType::TEXT_HTML)
            .body(message.html_body.clone())
            .map_err(|e| format!("Failed to build email: {}", e))?;

        // secure=true: TLS from the first byte; otherwise STARTTLS (Go's
        // net/smtp upgrades when the server offers it, which SendGrid's :587
        // does; here the upgrade is required, so credentials never travel in
        // clear).
        let builder = if self.secure {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&self.host)
                .map_err(|e| format!("SMTP TLS connection failed: {}", e))?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.host)
                .map_err(|e| format!("SMTP STARTTLS connection failed: {}", e))?
        }
        .port(self.port);
        // Go authenticates only when a username is configured.
        let builder = if self.username.is_empty() {
            builder
        } else {
            builder.credentials(Credentials::new(
                self.username.clone(),
                self.password.clone(),
            ))
        };

        builder
            .build()
            .send(email)
            .await
            .map_err(|e| format!("Failed to send email: {}", e))?;

        info!(to = %message.to, subject = %message.subject, "Email sent successfully");
        Ok(())
    }
}

/// Create the appropriate email service based on environment configuration.
pub fn create_email_service() -> Box<dyn EmailService> {
    if let Some(smtp) = SmtpEmailService::from_env() {
        info!("SMTP email service configured");
        Box::new(smtp)
    } else {
        warn!("SMTP not configured — emails will be logged only");
        Box::new(LogEmailService)
    }
}
