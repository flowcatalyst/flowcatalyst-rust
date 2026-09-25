//! FlowCatalyst's UI kit: the Vue app's look (PrimeVue Nora + the
//! FlowCatalyst chrome, see `styles.css`) as server-rendered components.
//!
//! | here                  | Vue app                                                |
//! |-----------------------|--------------------------------------------------------|
//! | [`page_header`]       | `page-header` / `page-title` / `page-subtitle`         |
//! | [`filter_select`]     | `filter-group` + `Select`, URL-synced (`useListState`) |
//! | [`search_input`]      | `IconField` + `InputIcon` + `InputText`                |
//! | [`cursor_pager`]      | `useCursorPagination` Newer/Older                      |
//! | [`tag`]               | `Tag` with a severity                                  |
//! | [`code_chips`]        | `.code-display` / `.code-segment` event type codes     |
//! | [`confirm_dialog`]    | `useConfirm` + `ConfirmDialog`                         |
//! | [`json_block`]        | pretty-printed JSON `<pre>`                            |
//! | [`local_time`]        | `new Date(iso).toLocaleString()`                       |
//! | [`flash`]             | `errorBus` + `NotificationBannerStack`                 |
//!
//! Dialogs are native `<dialog>` elements opened with invoker commands
//! (`commandfor` + `command="show-modal"`): real modals (focus trap,
//! Escape, backdrop) with no script. Menus use `popover`, collapsible nav
//! uses `<details>`.

pub mod flash;

use chrono::{DateTime, Utc};
use topcoat::{
    Result,
    context::Cx,
    icon::{IconData, icon, iconify::iconify_icon},
    view::{
        AttributeValueViewParts, Child, Length, NodeViewParts, PartsWriter, View, attributes,
        component, view,
    },
};

pub use flash::{FlashKind, flash_banner, set_flash};

/// Trusted markup rendered as-is. Only for admin-configured content the Vue
/// app also renders with `v-html` (the theme's `logoSvg`).
pub struct TrustedHtml(pub String);

impl NodeViewParts for TrustedHtml {
    fn into_view_parts(self, _cx: &Cx, parts: &mut PartsWriter<'_>) {
        parts.push_string_unescaped(self.0);
    }
}

/// FlowCatalyst's default logo: the bolt the Vue app draws when the theme
/// has no `logoUrl`/`logoSvg` (`AppSidebar.vue`, `LoginPage.vue`). Sized
/// by its container, coloured by `currentColor`.
pub fn default_logo() -> TrustedHtml {
    TrustedHtml(
        r#"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" width="100%" height="100%" aria-label="FlowCatalyst"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M13 10V3L4 14h7v7l9-11h-7z"/></svg>"#
            .to_owned(),
    )
}

/// A PrimeVue button look, as a class for `<button>` or `<a>`:
/// `<a class=(Btn::Primary) href=...>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Btn {
    Primary,
    Secondary,
    Outline,
    WarnOutline,
    DangerOutline,
    Danger,
    Text,
    Link,
}

impl Btn {
    pub fn class(self) -> &'static str {
        match self {
            Btn::Primary => "fc-btn fc-btn-primary",
            Btn::Secondary => "fc-btn fc-btn-secondary",
            Btn::Outline => "fc-btn fc-btn-outline",
            Btn::WarnOutline => "fc-btn fc-btn-warn-outline",
            Btn::DangerOutline => "fc-btn fc-btn-danger-outline",
            Btn::Danger => "fc-btn fc-btn-danger",
            Btn::Text => "fc-btn fc-btn-text",
            Btn::Link => "fc-btn fc-btn-link",
        }
    }
}

impl AttributeValueViewParts for Btn {
    fn attribute_present(&self) -> bool {
        true
    }
    fn into_view_parts(self, _cx: &Cx, parts: &mut PartsWriter<'_>) {
        parts.push_static_str(self.class());
    }
}

/// The title block every page starts with. Children are the page's
/// actions, right-aligned.
#[component]
pub async fn page_header(
    #[into] title: String,
    #[into] subtitle: String,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <header class="fc-page-header">
            <div>
                <h1 class="fc-page-title">(title)</h1>
                <p class="fc-page-subtitle">(subtitle)</p>
            </div>
            <div class="flex items-center gap-2">(child)</div>
        </header>
    })
}

/// PrimeVue `Tag` severities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Severity {
    Success,
    Info,
    Warn,
    Danger,
    Secondary,
}

#[component]
pub async fn tag(#[into] label: String, severity: Severity) -> Result<impl View> {
    let class = match severity {
        Severity::Success => "fc-tag fc-tag-success",
        Severity::Info => "fc-tag fc-tag-info",
        Severity::Warn => "fc-tag fc-tag-warn",
        Severity::Danger => "fc-tag fc-tag-danger",
        Severity::Secondary => "fc-tag fc-tag-secondary",
    };
    Ok(view! { <span class=(class)>(label)</span> })
}

/// A labelled `<select>` in a filter row that submits its GET form on
/// change, so the URL is the list state (bookmarkable, survives refresh).
/// `options` are `(value, label)`; the empty option clears the filter.
#[component]
pub async fn filter_select(
    name: &'static str,
    #[into] label: String,
    #[into] placeholder: String,
    options: Vec<(String, String)>,
    selected: Option<String>,
) -> Result<impl View> {
    let id = format!("filter-{name}");
    Ok(view! {
        <div class="fc-filter-group">
            <label class="fc-label" for=(&id)>(label)</label>
            <select id=(&id) name=(name) class="fc-select" onchange="this.form.requestSubmit()">
                <option value="" selected=(selected.is_none())>(placeholder)</option>
                for (value, label) in options {
                    <option value=(&value) selected=(selected.as_deref() == Some(value.as_str()))>(label)</option>
                }
            </select>
        </div>
    })
}

/// A search box with a leading icon. Submits with Enter (GET form).
#[component]
pub async fn search_input(
    name: &'static str,
    #[into] placeholder: String,
    value: Option<String>,
) -> Result<impl View> {
    Ok(view! {
        <div class="fc-input-icon w-80 max-w-full">
            icon(data: iconify_icon!("lucide:search"), size: Length::rem(1.0))
            <input
                type="search"
                name=(name)
                class="fc-input"
                placeholder=(&placeholder)
                aria-label=(&placeholder)
                value=(value.unwrap_or_default())
            >
        </div>
    })
}

/// "Newest" / "Older" for keyset-paginated lists. The browser's back button
/// is "newer": each page is its own URL.
#[component]
pub async fn cursor_pager(
    newest_href: Option<String>,
    older_href: Option<String>,
    #[into] summary: String,
) -> Result<impl View> {
    Ok(view! {
        <div class="fc-table-footer">
            <span>(summary)</span>
            <nav class="flex items-center gap-2" aria-label="Pagination">
                pager_link(href: newest_href, label: "Newest", icon_data: iconify_icon!("lucide:chevrons-left"))
                pager_link(href: older_href, label: "Older", icon_data: iconify_icon!("lucide:chevron-right"))
            </nav>
        </div>
    })
}

#[component]
async fn pager_link(
    href: Option<String>,
    label: &'static str,
    icon_data: IconData,
) -> Result<impl View> {
    Ok(view! {
        match href {
            Some(href) => <a class="fc-btn fc-btn-outline fc-btn-sm" href=(href)>icon(data: icon_data) (label)</a>,
            None => <span class="fc-btn fc-btn-outline fc-btn-sm" aria-disabled="true">icon(data: icon_data) (label)</span>,
        }
    })
}

/// An event type code as the Vue app shows it: four coloured segments.
#[component]
pub async fn code_chips(#[into] code: String) -> Result<impl View> {
    const KINDS: [&str; 4] = ["app", "subdomain", "aggregate", "event"];
    let parts: Vec<(usize, String)> = code.split(':').map(str::to_owned).enumerate().collect();
    Ok(view! {
        <span class="fc-code">
            for (i, part) in parts {
                if i > 0 { <span class="fc-code-sep">":"</span> }
                <span class=(format!("fc-code-seg {}", KINDS[i.min(3)]))>(part)</span>
            }
        </span>
    })
}

/// A confirm-then-POST modal. Open it from any button with
/// `commandfor=(id) command="show-modal"`.
#[component]
pub async fn confirm_dialog(
    #[into] id: String,
    /// The route the form posts to.
    #[into]
    action: String,
    #[into] title: String,
    #[into] message: String,
    #[into] confirm_label: String,
    #[default] danger: bool,
    /// Extra hidden fields, as (name, value).
    #[default]
    fields: Vec<(String, String)>,
) -> Result<impl View> {
    let confirm = if danger { Btn::Danger } else { Btn::Primary };
    let icon_class = if danger {
        "mt-0.5 shrink-0 text-red-600"
    } else {
        "mt-0.5 shrink-0 text-amber-600"
    };
    let title_id = format!("{id}-title");
    Ok(view! {
        <dialog id=(&id) class="fc-dialog w-[28rem] max-w-[calc(100vw-2rem)]" closedby="any" aria-labelledby=(&title_id)>
            <form method="post" action=(action)>
                <div class="fc-dialog-header">
                    <span id=(&title_id)>(title)</span>
                </div>
                <div class="fc-dialog-body flex items-start gap-3 text-[#475569]">
                    icon(
                        data: iconify_icon!("lucide:triangle-alert"),
                        size: Length::rem(1.5),
                        attrs: attributes! { class=(icon_class) }
                    )
                    <p>(message)</p>
                </div>
                for (name, value) in fields {
                    <input type="hidden" name=(name) value=(value)>
                }
                <div class="fc-dialog-footer">
                    <button type="button" class=(Btn::Text) commandfor=(&id) command="close">"Cancel"</button>
                    <button type="submit" class=(confirm)>(confirm_label)</button>
                </div>
            </form>
        </dialog>
    })
}

/// Pretty-printed JSON.
#[component]
pub async fn json_block(value: serde_json::Value) -> Result<impl View> {
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    Ok(view! { <pre class="fc-json">(text)</pre> })
}

/// A timestamp. Rendered in UTC; `ui.js` rewrites it to the viewer's
/// locale and time zone, as the Vue app's `toLocaleString()` does.
#[component]
pub async fn local_time(at: DateTime<Utc>) -> Result<impl View> {
    Ok(view! {
        <time datetime=(at.to_rfc3339()) data-local="">(at.format("%Y-%m-%d %H:%M:%S UTC").to_string())</time>
    })
}

/// Shown in place of table rows when there are none.
#[component]
pub async fn empty_state(#[into] message: String) -> Result<impl View> {
    Ok(view! {
        <div class="px-6 py-12 text-center text-[#64748b]">
            icon(data: iconify_icon!("lucide:inbox"), size: Length::rem(3.0), attrs: attributes! { class="mx-auto mb-4 text-[#cbd5e1]" })
            <p>(message)</p>
        </div>
    })
}
