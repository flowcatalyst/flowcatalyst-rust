//! List-page pieces: the table toolbar (`FcTableToolbar.vue`), the
//! paginator (PrimeVue `Paginator`), and URL helpers.
//!
//! The SPA keeps list state in the URL (`useListState`); here the URL *is*
//! the state: the toolbar is a GET form, each filter submits it on change,
//! and pages and rows-per-page are links and a select bound to the same
//! form. Leaving `page` out of the form resets it to 1 on any filter change,
//! as the SPA does.

use topcoat::{
    Result,
    icon::{IconData, icon, iconify::iconify_icon},
    view::{Child, Length, View, component, view},
};

use super::Btn;

/// `path?k=v&…`, skipping empty values.
pub fn list_query(path: &str, pairs: &[(&str, &str)]) -> String {
    let mut ser = form_urlencoded::Serializer::new(String::new());
    let mut any = false;
    for (k, v) in pairs {
        if !v.is_empty() {
            ser.append_pair(k, v);
            any = true;
        }
    }
    if any {
        format!("{path}?{}", ser.finish())
    } else {
        path.to_owned()
    }
}

/// The SPA's `:rowsPerPageOptions="[50, 100, 250, 500]"`, default 100.
pub const ROWS_OPTIONS: [usize; 4] = [50, 100, 250, 500];
pub const DEFAULT_ROWS: usize = 100;

/// Offset pagination over an in-memory list (the SPA's client-side
/// `paginator`), or over a server total.
#[derive(Clone, Debug)]
pub struct Pager {
    pub total: usize,
    /// 1-based.
    pub page: usize,
    pub rows: usize,
}

impl Pager {
    /// From the `page` / `rows` query values; anything unexpected falls
    /// back to page 1 and the default size.
    pub fn new(total: usize, page: Option<usize>, rows: Option<usize>) -> Self {
        let rows = rows
            .filter(|r| ROWS_OPTIONS.contains(r))
            .unwrap_or(DEFAULT_ROWS);
        let pages = total.div_ceil(rows).max(1);
        let page = page.unwrap_or(1).clamp(1, pages);
        Self { total, page, rows }
    }

    pub fn pages(&self) -> usize {
        self.total.div_ceil(self.rows).max(1)
    }

    /// This page's slice of `items`.
    pub fn slice<T>(&self, items: Vec<T>) -> Vec<T> {
        items
            .into_iter()
            .skip((self.page - 1) * self.rows)
            .take(self.rows)
            .collect()
    }

    /// "Showing 1 to 100 of 250 subscriptions".
    pub fn report(&self, noun: &str) -> String {
        if self.total == 0 {
            return format!("Showing 0 to 0 of 0 {noun}");
        }
        let first = (self.page - 1) * self.rows + 1;
        let last = (self.page * self.rows).min(self.total);
        format!("Showing {first} to {last} of {} {noun}", self.total)
    }
}

/// One paginator control: its label or icon, its link (none when
/// disabled), and whether it is the current page.
#[derive(Clone)]
pub struct PageLink {
    label: String,
    icon: Option<IconData>,
    href: Option<String>,
    current: bool,
}

/// The paginator under a table. `href(page)` builds a page's URL (with the
/// list's filters); `form_id` is the toolbar form the rows select submits.
#[component]
pub async fn paginator(
    pager: Pager,
    #[into] noun: String,
    href: std::sync::Arc<dyn Fn(usize) -> String + Send + Sync>,
    #[into] form_id: String,
) -> Result<impl View> {
    let pages = pager.pages();
    let p = pager.page;
    // PrimeVue shows five page links around the current one.
    let start = p
        .saturating_sub(2)
        .max(1)
        .min(pages.saturating_sub(4).max(1));
    let end = (start + 4).min(pages);
    let link = |label: String, glyph: Option<IconData>, to: usize, enabled: bool, current: bool| {
        PageLink {
            label,
            icon: glyph,
            href: (enabled && !current).then(|| href(to)),
            current,
        }
    };
    let mut links = vec![
        link(
            "First".into(),
            Some(iconify_icon!("lucide:chevrons-left")),
            1,
            p > 1,
            false,
        ),
        link(
            "Previous".into(),
            Some(iconify_icon!("lucide:chevron-left")),
            p.saturating_sub(1).max(1),
            p > 1,
            false,
        ),
    ];
    for n in start..=end {
        links.push(link(n.to_string(), None, n, true, n == p));
    }
    links.push(link(
        "Next".into(),
        Some(iconify_icon!("lucide:chevron-right")),
        (p + 1).min(pages),
        p < pages,
        false,
    ));
    links.push(link(
        "Last".into(),
        Some(iconify_icon!("lucide:chevrons-right")),
        pages,
        p < pages,
        false,
    ));
    let report = pager.report(&noun);
    let rows = pager.rows;

    Ok(view! {
        <nav class="fc-paginator" aria-label="Pagination">
            for l in links {
                match (l.href, l.icon) {
                    (Some(href), Some(icon_data)) => <a class="fc-page-link" href=(href) aria-label=(l.label)>icon(data: icon_data, size: Length::rem(1.0))</a>,
                    (None, Some(icon_data)) => <span class="fc-page-link" aria-disabled="true" aria-label=(l.label)>icon(data: icon_data, size: Length::rem(1.0))</span>,
                    (Some(href), None) => <a class="fc-page-link" href=(href)>(l.label)</a>,
                    (None, None) => <span class="fc-page-link" aria-current=(l.current.then_some("page"))>(l.label)</span>,
                }
            }
            <select name="rows" form=(&form_id) class="fc-select ml-2" aria-label="Rows per page" onchange="this.form.requestSubmit()">
                for option in ROWS_OPTIONS {
                    <option value=(option.to_string()) selected=(option == rows)>(option.to_string())</option>
                }
            </select>
            <span class="fc-page-report">(report)</span>
        </nav>
    })
}

/// `FcTableToolbar`: the quick search on the left; Clear All and the
/// Filters button (with its active-count badge) on the right. The toolbar
/// is the list's GET form; `child` is the popover's filter fields, which
/// submit it on change. `hidden` carries state the form must keep (the
/// page size).
#[component]
pub async fn table_toolbar(
    #[into] form_id: String,
    #[into] action: String,
    #[into] placeholder: String,
    search: Option<String>,
    #[default] active_filter_count: usize,
    #[default] has_active_filters: bool,
    #[default] show_filters: bool,
    #[default] show_search: bool,
    #[default] hidden: Vec<(String, String)>,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    let popover_id = format!("{form_id}-filters");
    let clear_href = action.clone();
    Ok(view! {
        <form id=(&form_id) method="get" action=(action) class="fc-table-toolbar">
            for (name, value) in hidden {
                <input type="hidden" name=(name) value=(value)>
            }
            <div class="fc-toolbar-start">
                if show_search {
                    <div class="fc-input-icon fc-toolbar-search">
                        icon(data: iconify_icon!("lucide:search"), size: Length::rem(1.0))
                        <input
                            type="search"
                            name="q"
                            class="fc-input"
                            placeholder=(&placeholder)
                            aria-label=(&placeholder)
                            value=(search.unwrap_or_default())
                        >
                    </div>
                }
            </div>
            <div class="fc-toolbar-end">
                if has_active_filters {
                    <a href=(clear_href) class=(Btn::Text)>
                        icon(data: iconify_icon!("lucide:funnel-x"), size: Length::rem(1.0))
                        "Clear All"
                    </a>
                }
                if show_filters {
                    <button type="button" class=(Btn::PrimaryOutline) popovertarget=(&popover_id)>
                        icon(data: iconify_icon!("lucide:funnel"), size: Length::rem(1.0))
                        "Filters"
                        if active_filter_count > 0 {
                            <span class="fc-badge">(active_filter_count.to_string())</span>
                        }
                    </button>
                    <div id=(&popover_id) popover="" class="fc-filter-popover" data-anchor-popover="">
                        <div class="fc-filter-popover-body">(child)</div>
                    </div>
                }
            </div>
        </form>
    })
}
