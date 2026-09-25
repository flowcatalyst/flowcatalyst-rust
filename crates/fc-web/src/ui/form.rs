//! The SPA's form kit (`components/form/`): `FcFormField`, `FcDetailField`.
//! `FcFormSection` is plain markup, since it has two slots:
//!
//! ```text
//! <section class="fc-form-section">                 // `flat`, in drawers
//!     <header class="fc-section-header">
//!         <h3 class="fc-section-title">"Details"</h3>
//!         …actions…
//!     </header>
//!     <div class="fc-section-body">…</div>
//! </section>
//! ```
//!
//! Grids: `.fc-form-grid` (two-column form) and `.fc-detail-grid` (read-only
//! values); `span` takes the full width. Both collapse to one column in a
//! narrow drawer.

use topcoat::{
    Result,
    view::{Child, View, component, view},
};

/// A labelled input. `for_id` is the input's `id`.
#[component]
pub async fn form_field(
    #[into] label: String,
    #[into] for_id: String,
    #[default] required: bool,
    #[default] span: bool,
    #[default] help: Option<String>,
    #[default] error: Option<String>,
    child: Child<'_>,
) -> Result<impl View> {
    let class = if span {
        "fc-form-field fc-span-2"
    } else {
        "fc-form-field"
    };
    Ok(view! {
        <div class=(class)>
            <label for=(for_id) class="fc-field-label">
                (label)
                if required {
                    " " <span class="fc-required">"*"</span>
                }
            </label>
            (child)
            if let Some(error) = error {
                <small class="fc-field-error">(error)</small>
            } else if let Some(help) = help {
                <small class="fc-field-help">(help)</small>
            }
        </div>
    })
}

/// A read-only value; an empty or absent one shows "—".
#[component]
pub async fn detail_value(
    #[into] label: String,
    value: Option<String>,
    #[default] span: bool,
) -> Result<impl View> {
    let value = value
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "—".to_owned());
    Ok(view! {
        detail_field(label: label, span: span, (value))
    })
}

/// A read-only field whose value is markup (a tag, a code, a link).
#[component]
pub async fn detail_field(
    #[into] label: String,
    #[default] span: bool,
    child: Child<'_>,
) -> Result<impl View> {
    let class = if span {
        "fc-detail-field fc-span-2"
    } else {
        "fc-detail-field"
    };
    Ok(view! {
        <div class=(class)>
            <span class="fc-field-label">(label)</span>
            <div class="fc-detail-value">(child)</div>
        </div>
    })
}
