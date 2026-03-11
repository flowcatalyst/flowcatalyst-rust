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
/// Configure via environment variables:
/// - `FC_SMTP_HOST` — SMTP server hostname
/// - `FC_SMTP_PORT` — SMTP server port (default: 587)
/// - `FC_SMTP_USERNAME` — SMTP auth username
/// - `FC_SMTP_PASSWORD` — SMTP auth password
/// - `FC_SMTP_FROM` — Sender email address
pub struct SmtpEmailService {
    host: String,
    port: u16,
    username: String,
    password: String,
    from: String,
}

impl SmtpEmailService {
    /// Create from environment variables. Returns None if SMTP is not configured.
    pub fn from_env() -> Option<Self> {
        let host = std::env::var("FC_SMTP_HOST").ok()?;
        let port = std::env::var("FC_SMTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(587);
        let username = std::env::var("FC_SMTP_USERNAME").unwrap_or_default();
        let password = std::env::var("FC_SMTP_PASSWORD").unwrap_or_default();
        let from = std::env::var("FC_SMTP_FROM").unwrap_or_else(|_| "noreply@flowcatalyst.local".to_string());

        Some(Self { host, port, username, password, from })
    }
}

#[async_trait]
impl EmailService for SmtpEmailService {
    async fn send(&self, message: &EmailMessage) -> Result<(), String> {
        use lettre::{
            Message as LettreMessage,
            SmtpTransport, Transport,
            transport::smtp::authentication::Credentials,
            message::{header::ContentType, Mailbox},
        };

        let from_mailbox: Mailbox = self.from.parse()
            .map_err(|e| format!("Invalid from address: {}", e))?;
        let to_mailbox: Mailbox = message.to.parse()
            .map_err(|e| format!("Invalid to address: {}", e))?;

        let email = LettreMessage::builder()
            .from(from_mailbox)
            .to(to_mailbox)
            .subject(&message.subject)
            .header(ContentType::TEXT_HTML)
            .body(message.html_body.clone())
            .map_err(|e| format!("Failed to build email: {}", e))?;

        let creds = Credentials::new(self.username.clone(), self.password.clone());

        let mailer = SmtpTransport::starttls_relay(&self.host)
            .map_err(|e| format!("SMTP connection failed: {}", e))?
            .port(self.port)
            .credentials(creds)
            .build();

        mailer.send(&email)
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
