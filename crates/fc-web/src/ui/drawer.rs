//! The record drawer, as `components/drawer/EntityDrawer.vue`: a non-modal
//! right panel over the list (no mask, so the list stays scrollable and
//! clickable, and clicking another row switches the drawer), closed with
//! its X or Escape.
//!
//! A section's list component owns a `selected` signal (the open record's
//! id, empty for closed) and renders [`drawer_frame`] around its drawer
//! shard. Rows set the signal; the shard re-renders for the new id; the
//! frame hides itself when the signal is empty. `/ui/<section>/{id}` pages
//! start with the signal set, so writes redirect back to an open drawer
//! and the drawer is linkable.

use topcoat::{
    Result,
    icon::{icon, iconify::iconify_icon},
    runtime::{Event, Signal},
    view::{Child, Length, View, component, view},
};

/// `size` on `EntityDrawer`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub enum DrawerSize {
    #[default]
    Default,
    Wide,
    TwoThirds,
}

impl DrawerSize {
    fn class(self) -> &'static str {
        match self {
            DrawerSize::Default => "fc-drawer",
            DrawerSize::Wide => "fc-drawer fc-drawer-wide",
            DrawerSize::TwoThirds => "fc-drawer fc-drawer-two-thirds",
        }
    }
}

/// The panel around a drawer shard. Hidden while `selected` is empty; the
/// X clears it. `close_href` is where the X goes without the runtime (the
/// list's URL).
#[component]
pub async fn drawer_frame(
    selected: Signal<String>,
    #[into] label: String,
    #[default] size: DrawerSize,
    child: Child<'_>,
) -> Result<impl View> {
    let close = selected.clone();
    Ok(view! {
        <aside
            class=(size.class())
            role="complementary"
            aria-label=(label)
            data-drawer=""
            :hidden=$(selected.get().is_empty())
        >
            <button
                type="button"
                class="fc-icon-btn absolute top-[1.1rem] right-[1.1rem] z-10"
                aria-label="Close"
                data-drawer-close=""
                @click=$(|_e: Event| close.set("".to_owned()))
            >
                icon(data: iconify_icon!("lucide:x"), size: Length::rem(1.2))
            </button>
            (child)
        </aside>
    })
}

/// The drawer's header row: the title with the subtitle underneath, and
/// the extras (status tags) beside them. Leaves room for the frame's close button.
#[component]
pub async fn drawer_header(
    #[into] title: String,
    #[default] subtitle: Option<String>,
    #[default] child: Child<'_>,
) -> Result<impl View> {
    Ok(view! {
        <header class="fc-drawer-header pr-14">
            <div class="fc-drawer-titles">
                <div class="min-w-0">
                    <h2 class="fc-drawer-title">(title)</h2>
                    if let Some(subtitle) = subtitle {
                        <p class="fc-drawer-subtitle">(subtitle)</p>
                    }
                </div>
                <div class="flex shrink-0 items-center gap-2">(child)</div>
            </div>
        </header>
    })
}
