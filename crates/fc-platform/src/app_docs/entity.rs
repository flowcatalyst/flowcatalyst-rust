//! Application documentation pages.

use chrono::{DateTime, Utc};

use crate::usecase::unit_of_work::HasId;

/// One synced page (Go `appdocs.Doc`).
#[derive(Debug, Clone, PartialEq)]
pub struct AppDoc {
    pub id: String,
    pub application_id: String,
    pub slug: String,
    pub title: String,
    pub content: String,
    pub position: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An application's full replacement of its pages: every listed page (in
/// order) is written, every other page of the application is removed.
#[derive(Debug, Clone)]
pub struct AppDocsReplacement {
    pub application_id: String,
    pub docs: Vec<AppDoc>,
    pub removed_slugs: Vec<String>,
}

impl HasId for AppDocsReplacement {
    fn id(&self) -> &str {
        &self.application_id
    }
}

/// Go `docTitle`: the explicit title, else the first `# ` heading, else the
/// slug.
pub fn doc_title(slug: &str, title: Option<&str>, content: &str) -> String {
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    content
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("# "))
        .map(|h| h.trim().to_string())
        .unwrap_or_else(|| slug.to_string())
}

#[cfg(test)]
mod tests {
    use super::doc_title;

    #[test]
    fn titles_fall_back_to_the_heading_then_the_slug() {
        assert_eq!(doc_title("s", Some(" T "), "# H"), "T");
        assert_eq!(doc_title("s", None, "intro\n  # Heading \nbody"), "Heading");
        assert_eq!(doc_title("s", Some(" "), "no heading"), "s");
    }
}
