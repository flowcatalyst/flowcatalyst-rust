//! The platform's own documentation pages, embedded as Go embeds them
//! (`docs/embed.go`, `docsapi/api.go` index): a page's slug is its file
//! name without `.md` and without its `NN-` ordering prefix; its title is
//! its first `# ` heading, else the slug. Copied from Go; see
//! `docs/published/PROVENANCE.md`.

/// `(file name, content)`, in file-name order.
const FILES: &[(&str, &str)] = &[
    (
        "10-platform-overview.md",
        include_str!("../../../../docs/published/10-platform-overview.md"),
    ),
    (
        "20-messaging-and-delivery.md",
        include_str!("../../../../docs/published/20-messaging-and-delivery.md"),
    ),
    (
        "30-identity-and-access.md",
        include_str!("../../../../docs/published/30-identity-and-access.md"),
    ),
    (
        "40-portal-users.md",
        include_str!("../../../../docs/published/40-portal-users.md"),
    ),
    (
        "50-applications-and-integration.md",
        include_str!("../../../../docs/published/50-applications-and-integration.md"),
    ),
];

/// A platform page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformDoc {
    pub slug: &'static str,
    pub title: &'static str,
    pub content: &'static str,
}

fn slug_of(file: &'static str) -> &'static str {
    let stem = file.strip_suffix(".md").unwrap_or(file);
    match stem.split_once('-') {
        Some((prefix, rest))
            if !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit()) =>
        {
            rest
        }
        _ => stem,
    }
}

fn title_of(slug: &'static str, content: &'static str) -> &'static str {
    content
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("# "))
        .map(str::trim)
        .unwrap_or(slug)
}

/// Every platform page, in order.
pub fn platform_docs() -> Vec<PlatformDoc> {
    FILES
        .iter()
        .map(|(file, content)| {
            let slug = slug_of(file);
            PlatformDoc {
                slug,
                title: title_of(slug, content),
                content,
            }
        })
        .collect()
}

/// The platform page with `slug`.
pub fn platform_doc(slug: &str) -> Option<PlatformDoc> {
    platform_docs().into_iter().find(|d| d.slug == slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_indexed_as_go_indexes_them() {
        let docs = platform_docs();
        let index: Vec<(&str, &str)> = docs.iter().map(|d| (d.slug, d.title)).collect();
        assert_eq!(
            index,
            [
                ("platform-overview", "Platform Overview"),
                ("messaging-and-delivery", "Messaging & Delivery"),
                ("identity-and-access", "Identity & Access"),
                ("portal-users", "Portal Users Architecture"),
                ("applications-and-integration", "Applications & Integration"),
            ]
        );
        assert!(platform_doc("portal-users")
            .unwrap()
            .content
            .starts_with("# Portal Users Architecture"));
        assert!(platform_doc("nope").is_none());
    }
}
