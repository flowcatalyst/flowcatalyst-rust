//! One-shot messages across a Post/Redirect/Get: the handler sets a flash
//! cookie before redirecting, the layout shows it once and clears it.

use topcoat::{
    Result,
    context::Cx,
    cookie::{Cookies, cookie, cookies},
    view::{View, component, view},
};


const COOKIE: &str = "fc_web_flash";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashKind {
    Success,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Flash {
    pub kind: FlashKind,
    pub message: String,
}

/// Queue a message for the next page the browser loads.
pub fn set_flash(cx: &Cx, kind: FlashKind, message: impl AsRef<str>) {
    let kind = match kind {
        FlashKind::Success => "success",
        FlashKind::Error => "error",
    };
    let value = form_urlencoded::Serializer::new(String::new())
        .append_pair("k", kind)
        .append_pair("m", message.as_ref())
        .finish();
    cookies(cx).add(cookie! {
        COOKIE = value;
        Path = "/ui";
        HttpOnly;
        SameSite = Lax
    });
}

/// Read and clear the pending message. Call before the view starts
/// streaming (cookie writes after that point panic).
pub fn take_flash(cx: &Cx) -> Option<Flash> {
    let raw = cookies(cx).get(COOKIE)?.value().to_owned();
    cookies(cx).remove(cookie!(COOKIE = ""; Path = "/ui"));
    let mut kind = None;
    let mut message = None;
    for (k, v) in form_urlencoded::parse(raw.as_bytes()) {
        match k.as_ref() {
            "k" => kind = Some(v.into_owned()),
            "m" => message = Some(v.into_owned()),
            _ => {}
        }
    }
    let kind = match kind.as_deref() {
        Some("error") => FlashKind::Error,
        _ => FlashKind::Success,
    };
    message.map(|message| Flash { kind, message })
}

#[component]
pub async fn flash_banner(flash: Option<Flash>) -> Result<impl View> {
    Ok(view! {
        if let Some(flash) = flash {
            <div
                role="status"
                class=(match flash.kind {
                    FlashKind::Success => "fc-banner fc-banner-success mb-4",
                    FlashKind::Error => "fc-banner fc-banner-error mb-4",
                })
            >
                (flash.message)
            </div>
        }
    })
}
