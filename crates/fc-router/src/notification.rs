//! Notification Service - Teams webhook and email notifications
//!
//! Provides:
//! - Microsoft Teams webhook notifications via Adaptive Cards
//! - Email notifications via SMTP (requires `email` feature)
//! - Batching support for warning notifications
//! - Severity filtering

use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde_json::json;
use tracing::{debug, error, info, warn};

use fc_common::{Warning, WarningSeverity};

/// Notification service trait
#[async_trait]
pub trait NotificationService: Send + Sync {
    /// Send a warning notification
    async fn notify_warning(&self, warning: &Warning);

    /// Send a critical error notification
    async fn notify_critical_error(&self, message: &str, source: &str);

    /// Send a system event notification
    async fn notify_system_event(&self, event_type: &str, message: &str);

    /// Check if notifications are enabled
    fn is_enabled(&self) -> bool;
}

/// Notification configuration
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    /// Teams webhook URL
    pub teams_webhook_url: Option<String>,
    /// Whether Teams notifications are enabled
    pub teams_enabled: bool,
    /// Minimum severity to send notifications
    pub min_severity: WarningSeverity,
    /// Batch interval in seconds (0 = no batching)
    pub batch_interval_seconds: u64,
    /// Email notification configuration (requires `email` feature)
    #[cfg(feature = "email")]
    pub email_config: Option<EmailConfig>,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            teams_webhook_url: None,
            teams_enabled: false,
            min_severity: WarningSeverity::Warn,
            batch_interval_seconds: 300, // 5 minutes
            #[cfg(feature = "email")]
            email_config: None,
        }
    }
}

/// No-op notification service for when notifications are disabled
pub struct NoOpNotificationService;

#[async_trait]
impl NotificationService for NoOpNotificationService {
    async fn notify_warning(&self, _warning: &Warning) {}
    async fn notify_critical_error(&self, _message: &str, _source: &str) {}
    async fn notify_system_event(&self, _event_type: &str, _message: &str) {}
    fn is_enabled(&self) -> bool {
        false
    }
}

/// Microsoft Teams webhook notification service
pub struct TeamsWebhookNotificationService {
    client: reqwest::Client,
    webhook_url: String,
    enabled: bool,
}

impl TeamsWebhookNotificationService {
    pub fn new(webhook_url: String, enabled: bool) -> Self {
        info!(
            enabled = enabled,
            "TeamsWebhookNotificationService initialized"
        );
        Self {
            client: reqwest::Client::new(),
            webhook_url,
            enabled,
        }
    }

    /// Build Adaptive Card JSON for a warning
    fn build_warning_card(&self, warning: &Warning) -> serde_json::Value {
        let color = self.get_severity_color(&warning.severity);
        let severity_str = format!("{:?}", warning.severity);
        let category_str = format!("{:?}", warning.category);
        let timestamp = warning.created_at.format("%Y-%m-%dT%H:%M:%S").to_string();

        json!({
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "content": {
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": [
                        {
                            "type": "Container",
                            "style": "emphasis",
                            "items": [{
                                "type": "ColumnSet",
                                "columns": [
                                    {
                                        "type": "Column",
                                        "width": "auto",
                                        "items": [{
                                            "type": "TextBlock",
                                            "text": "⚠️",
                                            "size": "Large"
                                        }]
                                    },
                                    {
                                        "type": "Column",
                                        "width": "stretch",
                                        "items": [
                                            {
                                                "type": "TextBlock",
                                                "text": "FlowCatalyst Alert",
                                                "weight": "Bolder",
                                                "size": "Large"
                                            },
                                            {
                                                "type": "TextBlock",
                                                "text": format!("{} - {}", severity_str, category_str),
                                                "color": color,
                                                "weight": "Bolder",
                                                "size": "Medium",
                                                "spacing": "None"
                                            }
                                        ]
                                    }
                                ]
                            }]
                        },
                        {
                            "type": "FactSet",
                            "facts": [
                                { "title": "Category:", "value": category_str },
                                { "title": "Source:", "value": &warning.source },
                                { "title": "Time:", "value": timestamp }
                            ]
                        },
                        {
                            "type": "TextBlock",
                            "text": "Message",
                            "weight": "Bolder",
                            "separator": true
                        },
                        {
                            "type": "TextBlock",
                            "text": &warning.message,
                            "wrap": true,
                            "spacing": "Small"
                        }
                    ]
                }
            }]
        })
    }

    /// Build Adaptive Card for critical error
    fn build_critical_error_card(&self, message: &str, source: &str) -> serde_json::Value {
        json!({
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "content": {
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": [
                        {
                            "type": "Container",
                            "style": "attention",
                            "items": [{
                                "type": "TextBlock",
                                "text": "🚨 CRITICAL ERROR",
                                "weight": "Bolder",
                                "size": "ExtraLarge",
                                "color": "Attention"
                            }]
                        },
                        {
                            "type": "FactSet",
                            "facts": [
                                { "title": "Source:", "value": source }
                            ]
                        },
                        {
                            "type": "TextBlock",
                            "text": message,
                            "wrap": true,
                            "spacing": "Medium"
                        },
                        {
                            "type": "TextBlock",
                            "text": "⚡ Immediate action required",
                            "weight": "Bolder",
                            "color": "Attention",
                            "separator": true
                        }
                    ]
                }
            }]
        })
    }

    /// Build Adaptive Card for system event
    fn build_system_event_card(&self, event_type: &str, message: &str) -> serde_json::Value {
        json!({
            "attachments": [{
                "contentType": "application/vnd.microsoft.card.adaptive",
                "content": {
                    "type": "AdaptiveCard",
                    "version": "1.4",
                    "body": [
                        {
                            "type": "Container",
                            "style": "accent",
                            "items": [{
                                "type": "TextBlock",
                                "text": format!("ℹ️ System Event: {}", event_type),
                                "weight": "Bolder",
                                "size": "Large"
                            }]
                        },
                        {
                            "type": "TextBlock",
                            "text": message,
                            "wrap": true,
                            "spacing": "Medium"
                        }
                    ]
                }
            }]
        })
    }

    /// Get Teams color for severity level
    fn get_severity_color(&self, severity: &WarningSeverity) -> &'static str {
        match severity {
            WarningSeverity::Critical | WarningSeverity::Error => "Attention",
            WarningSeverity::Warn => "Warning",
            WarningSeverity::Info => "Accent",
        }
    }

    /// Send JSON payload to Teams webhook
    async fn send_to_teams(&self, payload: serde_json::Value) -> Result<(), reqwest::Error> {
        let response = self
            .client
            .post(&self.webhook_url)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(
                status = %status,
                body = %body,
                "Teams webhook returned error"
            );
        }

        Ok(())
    }
}

#[async_trait]
impl NotificationService for TeamsWebhookNotificationService {
    async fn notify_warning(&self, warning: &Warning) {
        if !self.enabled {
            return;
        }

        let payload = self.build_warning_card(warning);
        if let Err(e) = self.send_to_teams(payload).await {
            error!(
                error = %e,
                category = ?warning.category,
                "Failed to send Teams notification"
            );
        } else {
            info!(
                severity = ?warning.severity,
                category = ?warning.category,
                "Teams notification sent"
            );
        }
    }

    async fn notify_critical_error(&self, message: &str, source: &str) {
        if !self.enabled {
            return;
        }

        let payload = self.build_critical_error_card(message, source);
        if let Err(e) = self.send_to_teams(payload).await {
            error!(
                error = %e,
                "Failed to send Teams critical error notification"
            );
        } else {
            info!("Teams critical error notification sent");
        }
    }

    async fn notify_system_event(&self, event_type: &str, message: &str) {
        if !self.enabled {
            return;
        }

        let payload = self.build_system_event_card(event_type, message);
        if let Err(e) = self.send_to_teams(payload).await {
            error!(
                error = %e,
                event_type = %event_type,
                "Failed to send Teams system event notification"
            );
        } else {
            debug!(
                event_type = %event_type,
                "Teams system event notification sent"
            );
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }
}

// ---------------------------------------------------------------------------
// Email notification service (behind `email` feature)
// ---------------------------------------------------------------------------

/// Configuration for the email notification channel.
#[cfg(feature = "email")]
#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// SMTP server hostname
    pub smtp_host: String,
    /// SMTP server port (typically 587 for STARTTLS, 465 for implicit TLS)
    pub smtp_port: u16,
    /// Optional SMTP username for authentication
    pub smtp_username: Option<String>,
    /// Optional SMTP password for authentication
    pub smtp_password: Option<String>,
    /// Whether to use TLS when connecting to the SMTP server
    pub smtp_tls: bool,
    /// Sender address (RFC 5322 mailbox, e.g. "FlowCatalyst <alerts@example.com>")
    pub from_address: String,
    /// Recipient addresses
    pub to_addresses: Vec<String>,
    /// Whether email notifications are enabled
    pub enabled: bool,
}

/// Email notification service using SMTP via `lettre`.
///
/// Sends HTML-formatted emails with severity-based colour coding that matches
/// the Java platform's notification format.
#[cfg(feature = "email")]
pub struct EmailNotificationService {
    transport: lettre::AsyncSmtpTransport<lettre::Tokio1Executor>,
    from: lettre::message::Mailbox,
    to: Vec<lettre::message::Mailbox>,
    enabled: bool,
}

#[cfg(feature = "email")]
impl EmailNotificationService {
    /// Create a new `EmailNotificationService` from the given configuration.
    ///
    /// Returns `Err` if the SMTP transport cannot be built or addresses are
    /// unparseable.
    pub fn new(config: EmailConfig) -> Result<Self, String> {
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::AsyncSmtpTransport;

        let from: lettre::message::Mailbox = config
            .from_address
            .parse()
            .map_err(|e| format!("Invalid from_address '{}': {}", config.from_address, e))?;

        let to: Vec<lettre::message::Mailbox> = config
            .to_addresses
            .iter()
            .map(|addr| {
                addr.parse()
                    .map_err(|e| format!("Invalid to_address '{}': {}", addr, e))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if to.is_empty() {
            return Err("At least one recipient address is required".to_string());
        }

        let mut builder = if config.smtp_tls {
            AsyncSmtpTransport::<lettre::Tokio1Executor>::starttls_relay(&config.smtp_host)
                .map_err(|e| format!("Failed to create STARTTLS transport: {}", e))?
        } else {
            AsyncSmtpTransport::<lettre::Tokio1Executor>::builder_dangerous(&config.smtp_host)
        };

        builder = builder.port(config.smtp_port);

        if let (Some(username), Some(password)) =
            (config.smtp_username.as_deref(), config.smtp_password.as_deref())
        {
            builder = builder.credentials(Credentials::new(
                username.to_owned(),
                password.to_owned(),
            ));
        }

        let transport = builder.build();

        info!(
            smtp_host = %config.smtp_host,
            smtp_port = config.smtp_port,
            smtp_tls = config.smtp_tls,
            recipients = to.len(),
            "EmailNotificationService initialized"
        );

        Ok(Self {
            transport,
            from,
            to,
            enabled: config.enabled,
        })
    }

    // -- HTML template helpers ------------------------------------------------

    /// Wrap body content in the shared HTML email scaffold.
    fn html_email(banner_colour: &str, title: &str, body_html: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body style="margin:0;padding:0;font-family:Arial,Helvetica,sans-serif;background:#f4f4f4;">
  <table width="100%" cellpadding="0" cellspacing="0" style="background:#f4f4f4;padding:20px 0;">
    <tr><td align="center">
      <table width="600" cellpadding="0" cellspacing="0" style="background:#ffffff;border-radius:8px;overflow:hidden;">
        <tr>
          <td style="background:{banner_colour};padding:16px 24px;">
            <span style="color:#ffffff;font-size:20px;font-weight:bold;">{title}</span>
          </td>
        </tr>
        <tr>
          <td style="padding:24px;">
            {body_html}
          </td>
        </tr>
        <tr>
          <td style="padding:12px 24px;background:#f8f9fa;color:#6c757d;font-size:12px;">
            Sent by FlowCatalyst Notification Service
          </td>
        </tr>
      </table>
    </td></tr>
  </table>
</body>
</html>"#,
            banner_colour = banner_colour,
            title = title,
            body_html = body_html,
        )
    }

    /// Build HTML for a warning notification.
    fn build_warning_html(warning: &Warning) -> String {
        let colour = Self::severity_colour(&warning.severity);
        let severity_str = format!("{:?}", warning.severity);
        let category_str = format!("{:?}", warning.category);
        let timestamp = warning.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string();

        let body = format!(
            r#"<table style="width:100%;border-collapse:collapse;">
  <tr><td style="padding:4px 8px;font-weight:bold;width:100px;">Severity:</td><td style="padding:4px 8px;color:{colour};">{severity}</td></tr>
  <tr><td style="padding:4px 8px;font-weight:bold;">Category:</td><td style="padding:4px 8px;">{category}</td></tr>
  <tr><td style="padding:4px 8px;font-weight:bold;">Source:</td><td style="padding:4px 8px;">{source}</td></tr>
  <tr><td style="padding:4px 8px;font-weight:bold;">Time:</td><td style="padding:4px 8px;">{timestamp}</td></tr>
</table>
<hr style="border:none;border-top:1px solid #dee2e6;margin:16px 0;">
<p style="margin:0;">{message}</p>"#,
            colour = colour,
            severity = severity_str,
            category = category_str,
            source = warning.source,
            timestamp = timestamp,
            message = warning.message,
        );

        let title = format!("FlowCatalyst Alert - {}", severity_str);
        Self::html_email(colour, &title, &body)
    }

    /// Build HTML for a critical error notification.
    fn build_critical_error_html(message: &str, source: &str) -> String {
        let body = format!(
            r#"<table style="width:100%;border-collapse:collapse;">
  <tr><td style="padding:4px 8px;font-weight:bold;width:100px;">Source:</td><td style="padding:4px 8px;">{source}</td></tr>
</table>
<hr style="border:none;border-top:1px solid #dee2e6;margin:16px 0;">
<p style="margin:0;">{message}</p>
<p style="margin:16px 0 0;font-weight:bold;color:#dc3545;">Immediate action required.</p>"#,
            source = source,
            message = message,
        );
        Self::html_email("#dc3545", "CRITICAL ERROR", &body)
    }

    /// Build HTML for a system event notification.
    fn build_system_event_html(event_type: &str, message: &str) -> String {
        let body = format!(
            r#"<table style="width:100%;border-collapse:collapse;">
  <tr><td style="padding:4px 8px;font-weight:bold;width:100px;">Event:</td><td style="padding:4px 8px;">{event_type}</td></tr>
</table>
<hr style="border:none;border-top:1px solid #dee2e6;margin:16px 0;">
<p style="margin:0;">{message}</p>"#,
            event_type = event_type,
            message = message,
        );
        Self::html_email("#17a2b8", &format!("System Event: {}", event_type), &body)
    }

    /// Map severity to an HTML colour.
    fn severity_colour(severity: &WarningSeverity) -> &'static str {
        match severity {
            WarningSeverity::Critical => "#dc3545",
            WarningSeverity::Error => "#fd7e14",
            WarningSeverity::Warn => "#ffc107",
            WarningSeverity::Info => "#17a2b8",
        }
    }

    /// Subject line for a warning email.
    fn warning_subject(warning: &Warning) -> String {
        format!(
            "[FlowCatalyst] {:?} - {:?}: {}",
            warning.severity,
            warning.category,
            truncate(&warning.message, 80),
        )
    }

    /// Send an HTML email to all configured recipients.
    async fn send_email(&self, subject: &str, html_body: &str) {
        use lettre::message::header::ContentType;
        use lettre::AsyncTransport;

        for recipient in &self.to {
            let message = match lettre::Message::builder()
                .from(self.from.clone())
                .to(recipient.clone())
                .subject(subject)
                .header(ContentType::TEXT_HTML)
                .body(html_body.to_owned())
            {
                Ok(msg) => msg,
                Err(e) => {
                    error!(
                        error = %e,
                        recipient = %recipient,
                        "Failed to build email message"
                    );
                    continue;
                }
            };

            match self.transport.send(message).await {
                Ok(_) => {
                    debug!(recipient = %recipient, subject = %subject, "Email sent");
                }
                Err(e) => {
                    error!(
                        error = %e,
                        recipient = %recipient,
                        "Failed to send email"
                    );
                }
            }
        }
    }
}

#[cfg(feature = "email")]
#[async_trait]
impl NotificationService for EmailNotificationService {
    async fn notify_warning(&self, warning: &Warning) {
        if !self.enabled {
            return;
        }
        let subject = Self::warning_subject(warning);
        let html = Self::build_warning_html(warning);
        self.send_email(&subject, &html).await;
    }

    async fn notify_critical_error(&self, message: &str, source: &str) {
        if !self.enabled {
            return;
        }
        let subject = format!(
            "[FlowCatalyst] CRITICAL ERROR - {}",
            truncate(source, 60),
        );
        let html = Self::build_critical_error_html(message, source);
        self.send_email(&subject, &html).await;
    }

    async fn notify_system_event(&self, event_type: &str, message: &str) {
        if !self.enabled {
            return;
        }
        let subject = format!(
            "[FlowCatalyst] System Event: {}",
            truncate(event_type, 60),
        );
        let html = Self::build_system_event_html(event_type, message);
        self.send_email(&subject, &html).await;
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
#[cfg(feature = "email")]
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        let mut end = max_len.saturating_sub(3);
        // Ensure we don't split a multi-byte character
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Batching notification service that collects warnings and sends summaries
pub struct BatchingNotificationService {
    delegates: Vec<Arc<dyn NotificationService>>,
    min_severity: WarningSeverity,
    warning_batch: Mutex<Vec<Warning>>,
    batch_start_time: Mutex<DateTime<Utc>>,
}

impl BatchingNotificationService {
    pub fn new(delegates: Vec<Arc<dyn NotificationService>>, min_severity: WarningSeverity) -> Self {
        info!(
            delegate_count = delegates.len(),
            min_severity = ?min_severity,
            "BatchingNotificationService initialized"
        );
        Self {
            delegates,
            min_severity,
            warning_batch: Mutex::new(Vec::new()),
            batch_start_time: Mutex::new(Utc::now()),
        }
    }

    /// Check if severity meets minimum threshold
    fn meets_min_severity(&self, severity: &WarningSeverity) -> bool {
        let severity_order = [
            WarningSeverity::Info,
            WarningSeverity::Warn,
            WarningSeverity::Error,
            WarningSeverity::Critical,
        ];

        let min_idx = severity_order
            .iter()
            .position(|s| *s == self.min_severity)
            .unwrap_or(0);
        let severity_idx = severity_order
            .iter()
            .position(|s| s == severity)
            .unwrap_or(0);

        severity_idx >= min_idx
    }

    /// Get the highest severity from a list of warnings
    fn get_highest_severity(&self, warnings: &[Warning]) -> WarningSeverity {
        warnings
            .iter()
            .map(|w| &w.severity)
            .max_by_key(|s| match s {
                WarningSeverity::Info => 0,
                WarningSeverity::Warn => 1,
                WarningSeverity::Error => 2,
                WarningSeverity::Critical => 3,
            })
            .cloned()
            .unwrap_or(WarningSeverity::Info)
    }

    /// Send batched notifications
    pub async fn send_batch(&self) {
        let warnings = {
            let mut batch = self.warning_batch.lock();
            if batch.is_empty() {
                debug!("No warnings to send in this batch period");
                return;
            }
            std::mem::take(&mut *batch)
        };

        let batch_start = {
            let mut start = self.batch_start_time.lock();
            let old = *start;
            *start = Utc::now();
            old
        };
        let batch_end = Utc::now();

        info!(
            warning_count = warnings.len(),
            batch_start = %batch_start,
            batch_end = %batch_end,
            "Sending batched notification"
        );

        // Group warnings by severity
        let mut by_severity: HashMap<WarningSeverity, Vec<&Warning>> = HashMap::new();
        for warning in &warnings {
            by_severity
                .entry(warning.severity.clone())
                .or_default()
                .push(warning);
        }

        // Build summary message
        let mut summary = format!(
            "FlowCatalyst Warning Summary ({} to {})\n\n",
            batch_start.format("%Y-%m-%d %H:%M:%S"),
            batch_end.format("%Y-%m-%d %H:%M:%S")
        );

        for severity in [
            WarningSeverity::Critical,
            WarningSeverity::Error,
            WarningSeverity::Warn,
            WarningSeverity::Info,
        ] {
            if let Some(warnings_for_severity) = by_severity.get(&severity) {
                summary.push_str(&format!(
                    "{:?} Issues ({}):\n",
                    severity,
                    warnings_for_severity.len()
                ));

                // Group by category
                let mut by_category: HashMap<String, Vec<&&Warning>> = HashMap::new();
                for w in warnings_for_severity {
                    by_category
                        .entry(format!("{:?}", w.category))
                        .or_default()
                        .push(w);
                }

                for (category, category_warnings) in by_category {
                    if category_warnings.len() == 1 {
                        summary.push_str(&format!(
                            "  - {}: {}\n",
                            category, category_warnings[0].message
                        ));
                    } else {
                        summary.push_str(&format!(
                            "  - {}: {} occurrences\n",
                            category,
                            category_warnings.len()
                        ));
                        summary.push_str(&format!(
                            "    Example: {}\n",
                            category_warnings[0].message
                        ));
                    }
                }
                summary.push('\n');
            }
        }

        summary.push_str(&format!("Total Warnings: {}\n", warnings.len()));

        // Create summary warning
        let highest_severity = self.get_highest_severity(&warnings);
        let summary_warning = Warning::new(
            fc_common::WarningCategory::Processing,
            highest_severity,
            summary,
            "BatchingNotificationService".to_string(),
        );

        // Send to all delegates
        for delegate in &self.delegates {
            delegate.notify_warning(&summary_warning).await;
        }
    }

    /// Get the number of pending warnings in the batch
    pub fn pending_count(&self) -> usize {
        self.warning_batch.lock().len()
    }
}

#[async_trait]
impl NotificationService for BatchingNotificationService {
    async fn notify_warning(&self, warning: &Warning) {
        if self.meets_min_severity(&warning.severity) {
            self.warning_batch.lock().push(warning.clone());
        }
    }

    async fn notify_critical_error(&self, message: &str, source: &str) {
        // Critical errors bypass batching
        for delegate in &self.delegates {
            delegate.notify_critical_error(message, source).await;
        }
    }

    async fn notify_system_event(&self, event_type: &str, message: &str) {
        // System events bypass batching
        for delegate in &self.delegates {
            delegate.notify_system_event(event_type, message).await;
        }
    }

    fn is_enabled(&self) -> bool {
        self.delegates.iter().any(|d| d.is_enabled())
    }
}

/// Create notification service based on configuration
pub fn create_notification_service(config: &NotificationConfig) -> Arc<dyn NotificationService> {
    let mut delegates: Vec<Arc<dyn NotificationService>> = Vec::new();

    if config.teams_enabled {
        if let Some(ref webhook_url) = config.teams_webhook_url {
            if !webhook_url.is_empty() {
                let teams_service = TeamsWebhookNotificationService::new(webhook_url.clone(), true);
                delegates.push(Arc::new(teams_service));
                info!("Teams webhook notifications enabled");
            } else {
                warn!("Teams notifications enabled but webhook URL is empty - skipping");
            }
        } else {
            warn!("Teams notifications enabled but webhook URL not configured - skipping");
        }
    }

    #[cfg(feature = "email")]
    if let Some(ref email_config) = config.email_config {
        if email_config.enabled {
            match EmailNotificationService::new(email_config.clone()) {
                Ok(email_service) => {
                    delegates.push(Arc::new(email_service));
                    info!("Email notifications enabled");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to initialize email notifications");
                }
            }
        }
    }

    if delegates.is_empty() {
        info!("No notification channels configured - using NoOpNotificationService");
        return Arc::new(NoOpNotificationService);
    }

    if config.batch_interval_seconds > 0 {
        Arc::new(BatchingNotificationService::new(
            delegates,
            config.min_severity.clone(),
        ))
    } else {
        // If only one delegate and no batching, return it directly
        if delegates.len() == 1 {
            delegates.remove(0)
        } else {
            // Multiple delegates but no batching - wrap anyway
            Arc::new(BatchingNotificationService::new(
                delegates,
                config.min_severity.clone(),
            ))
        }
    }
}

/// Result of creating a notification service with scheduler
pub struct NotificationServiceWithScheduler {
    /// The notification service
    pub service: Arc<BatchingNotificationService>,
    /// Task handle for the batch scheduler (if batching is enabled)
    pub scheduler_handle: Option<tokio::task::JoinHandle<()>>,
}

/// Create notification service with batch scheduler
/// Returns the batching service and spawns the batch scheduler task
pub fn create_notification_service_with_scheduler(
    config: &NotificationConfig,
) -> Option<NotificationServiceWithScheduler> {
    let mut delegates: Vec<Arc<dyn NotificationService>> = Vec::new();

    if config.teams_enabled {
        if let Some(ref webhook_url) = config.teams_webhook_url {
            if !webhook_url.is_empty() {
                let teams_service = TeamsWebhookNotificationService::new(webhook_url.clone(), true);
                delegates.push(Arc::new(teams_service));
                info!("Teams webhook notifications enabled");
            } else {
                warn!("Teams notifications enabled but webhook URL is empty - skipping");
            }
        } else {
            warn!("Teams notifications enabled but webhook URL not configured - skipping");
        }
    }

    #[cfg(feature = "email")]
    if let Some(ref email_config) = config.email_config {
        if email_config.enabled {
            match EmailNotificationService::new(email_config.clone()) {
                Ok(email_service) => {
                    delegates.push(Arc::new(email_service));
                    info!("Email notifications enabled");
                }
                Err(e) => {
                    warn!(error = %e, "Failed to initialize email notifications");
                }
            }
        }
    }

    if delegates.is_empty() {
        info!("No notification channels configured");
        return None;
    }

    let service = Arc::new(BatchingNotificationService::new(
        delegates,
        config.min_severity.clone(),
    ));

    let scheduler_handle = if config.batch_interval_seconds > 0 {
        let service_for_scheduler = service.clone();
        let interval = std::time::Duration::from_secs(config.batch_interval_seconds);

        Some(tokio::spawn(async move {
            info!(
                interval_secs = interval.as_secs(),
                "Starting notification batch scheduler"
            );
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                interval_timer.tick().await;
                service_for_scheduler.send_batch().await;
            }
        }))
    } else {
        None
    };

    Some(NotificationServiceWithScheduler {
        service,
        scheduler_handle,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::WarningCategory;

    #[test]
    fn test_severity_ordering() {
        let service = BatchingNotificationService::new(vec![], WarningSeverity::Warn);

        assert!(!service.meets_min_severity(&WarningSeverity::Info));
        assert!(service.meets_min_severity(&WarningSeverity::Warn));
        assert!(service.meets_min_severity(&WarningSeverity::Error));
        assert!(service.meets_min_severity(&WarningSeverity::Critical));
    }

    #[test]
    fn test_no_op_service() {
        let service = NoOpNotificationService;
        assert!(!service.is_enabled());
    }

    #[test]
    fn test_teams_service_disabled() {
        let service = TeamsWebhookNotificationService::new(
            "https://example.com/webhook".to_string(),
            false,
        );
        assert!(!service.is_enabled());
    }

    #[test]
    fn test_create_notification_service_disabled() {
        let config = NotificationConfig::default();
        let service = create_notification_service(&config);
        assert!(!service.is_enabled());
    }

    #[test]
    fn test_create_notification_service_teams_enabled() {
        let config = NotificationConfig {
            teams_enabled: true,
            teams_webhook_url: Some("https://example.com/webhook".to_string()),
            ..Default::default()
        };
        let service = create_notification_service(&config);
        assert!(service.is_enabled());
    }
}
