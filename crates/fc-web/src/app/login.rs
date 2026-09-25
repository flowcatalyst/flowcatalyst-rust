//! Sign-in, as the SPA's `LoginPage.vue`: email first, then the password
//! (or a redirect to the federated IdP, or a passkey), then a second factor
//! when one is owed. Plain HTML forms; the browser script covers what only
//! the platform's own JSON endpoints do (the WebAuthn ceremony, the 2FA
//! verify and email-code steps, the password-setup email).
//!
//! Reuses the API's own logic: `resolve_auth_method` (what `/auth/check-domain`
//! answers) with the same password-setup hint, `password_login` (what
//! `/auth/login` runs, including its second-factor gate), and the same
//! `fc_session` cookie.
//!
//! - `mfa_required`: the challenge step (TwoFactorChallenge.vue) posts the
//!   code to `POST /auth/2fa/verify`, which sets the session cookie.
//! - `enrollment_required`: enrolment (TOTP QR, email confirm) is left to
//!   the SPA's sign-in, which the step links to.
//! - `passwordSetupRequired`: the step emails the set-password link through
//!   `POST /auth/password-setup/request`.

use axum_extra::extract::cookie::CookieJar;
use fc_platform::auth::auth_api::{PasswordLogin, password_login};
use fc_platform::auth::oidc_login_api::{AuthMethod, AuthMethodError, resolve_auth_method};
use fc_platform::mfa::login_api::SecondFactor;
use fc_platform::shared::middleware::extract_trusted_client_ip;
use fc_platform::shared::public_api::{LoginThemeResponse, load_login_theme};
use serde::Deserialize;
use topcoat::{
    Result,
    asset::asset,
    context::Cx,
    cookie::{Cookies, cookies},
    icon::{icon, iconify::iconify_icon},
    router::{
        Method, StatusCode,
        content::Form,
        error::see_other,
        page,
        request::{headers, method},
    },
    runtime::{Event, signal},
    view::{Length, View, component, view},
};

use crate::auth::authenticate;
use crate::ui::{TrustedHtml, default_logo};

#[derive(Deserialize, Default)]
struct LoginForm {
    email: Option<String>,
    password: Option<String>,
    next: Option<String>,
}

/// Where to go after signing in. Only a path inside the new UI is honoured,
/// so the parameter can't be used as an open redirect.
fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if n.starts_with("/ui") && !n.starts_with("//") && !n.contains('\\') => {
            n.to_owned()
        }
        _ => "/ui".to_owned(),
    }
}

/// The steps of the form (the SPA's `step`).
enum Step {
    Email,
    Password {
        email: String,
    },
    /// An internal user who has never set a password.
    Setup {
        email: String,
    },
    /// `mfa_required`.
    TwoFactor {
        mfa_token: String,
        methods: Vec<String>,
        remember_device_allowed: bool,
    },
    /// `enrollment_required`.
    Enroll {
        email: String,
    },
}

#[page([GET, POST] "/ui/login")]
async fn login(cx: &Cx, form: Option<Form<LoginForm>>) -> Result<impl View> {
    let form = form.map(|Form(f)| f).unwrap_or_default();
    let next = safe_next(form.next.as_deref());
    let deps = crate::deps(cx);

    let (step, error, status) = if method(cx) == Method::GET {
        // Already signed in: straight through.
        if authenticate(cx).await?.is_some() {
            return Err(see_other(next).into());
        }
        (Step::Email, None, StatusCode::OK)
    } else if let Some(password) = form.password {
        // Step 2: a password was submitted.
        let email = form.email.unwrap_or_default().trim().to_owned();
        let ip = extract_trusted_client_ip(headers(cx));
        // The trusted-device cookie (remember this device) rides along.
        let jar = CookieJar::from_headers(headers(cx));
        match password_login(&deps.auth_state, &jar, &email, &password, ip.as_deref()).await {
            Ok(PasswordLogin::Session { session_token, .. }) => {
                cookies(cx).add(deps.auth_state.session_cookie.build_cookie(session_token));
                return Err(see_other(next).into());
            }
            Ok(PasswordLogin::SecondFactor(SecondFactor::Challenge {
                mfa_token,
                methods,
                remember_device_allowed,
            })) => (
                Step::TwoFactor {
                    mfa_token,
                    methods,
                    remember_device_allowed,
                },
                None,
                StatusCode::OK,
            ),
            Ok(PasswordLogin::SecondFactor(SecondFactor::Enrollment { .. })) => {
                (Step::Enroll { email }, None, StatusCode::OK)
            }
            Err(e) => {
                let status = e.status_code();
                let message = if status == StatusCode::TOO_MANY_REQUESTS {
                    "Too many attempts. Wait a moment and try again.".to_owned()
                } else if status.is_server_error() {
                    tracing::error!(error = %e, "fc-web: password login failed");
                    "Sign-in is unavailable right now.".to_owned()
                } else {
                    // The API's messages ("Invalid credentials", "Account is
                    // not active") are written for users.
                    e.to_string()
                };
                (Step::Password { email }, Some(message), status)
            }
        }
    } else {
        // Step 1: which way does this address sign in?
        let email = form.email.unwrap_or_default().trim().to_owned();
        match resolve_auth_method(
            &deps.anchor_domain_repo,
            &deps.edm_repo,
            &deps.idp_repo,
            &email,
        )
        .await
        {
            Ok(AuthMethod::Internal) => {
                // The check-domain hint: a user who has never set a password
                // gets the set-password step instead of a password prompt.
                let ip = extract_trusted_client_ip(headers(cx));
                let setup = match &deps.password_setup_hint {
                    Some(hint) => {
                        hint.required_for(&email.to_lowercase(), ip.as_deref())
                            .await
                    }
                    None => false,
                };
                if setup {
                    (Step::Setup { email }, None, StatusCode::OK)
                } else {
                    (Step::Password { email }, None, StatusCode::OK)
                }
            }
            Ok(AuthMethod::External { login_url, .. }) => {
                // `/auth/oidc/login` carries `return_url` through the IdP
                // round trip and lands the browser back here with the cookie set.
                let return_url = form_urlencoded::Serializer::new(String::new())
                    .append_pair("return_url", &next)
                    .finish();
                return Err(see_other(format!("{login_url}&{return_url}")).into());
            }
            Err(AuthMethodError::InvalidEmail) => (
                Step::Email,
                Some("Enter a valid email address.".to_owned()),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            Err(AuthMethodError::Lookup) => (
                Step::Email,
                Some("Sign-in is unavailable right now.".to_owned()),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        }
    };

    let theme = load_login_theme(&deps.platform_config_repo).await;
    Ok(view! {
        login_screen(theme: theme, step: step, next: next, error: error, status: status)
    })
}

#[component]
async fn login_screen(
    cx: &Cx,
    theme: LoginThemeResponse,
    step: Step,
    next: String,
    error: Option<String>,
    status: StatusCode,
) -> Result<impl View> {
    // As the Vue page renders it: the theme API sends unset fields as
    // `null`, which override `stores/loginTheme.ts`'s text defaults, so
    // brand name, subtitle and footer only show when configured.
    let brand = theme.brand_name.clone().filter(|s| !s.is_empty());
    let subtitle = theme.brand_subtitle.clone().filter(|s| !s.is_empty());
    let footer = theme.footer_text.clone().filter(|s| !s.is_empty());
    let background = theme
        .background_gradient
        .clone()
        .or_else(|| theme.background_color.clone())
        .unwrap_or_else(|| "linear-gradient(135deg, #102a43 0%, #0a1929 100%)".to_owned());
    let accent = theme
        .accent_color
        .clone()
        .unwrap_or_else(|| "#0967d2".to_owned());
    let style = format!("background: {background}; --login-accent: {accent};");
    let title = match step {
        Step::Email => "Sign in to your account",
        Step::Password { .. } => "Enter your password",
        Step::Setup { .. } => "Create your password",
        Step::TwoFactor { .. } => "Verify it's you",
        Step::Enroll { .. } => "Set up two-factor authentication",
    };
    let show_password = signal(cx, || false);
    let forgot_href = |email: &str| {
        let q = form_urlencoded::Serializer::new(String::new())
            .append_pair("email", email)
            .finish();
        format!("/auth/forgot-password?{q}")
    };
    let change_href = format!(
        "/ui/login?{}",
        form_urlencoded::Serializer::new(String::new())
            .append_pair("next", &next)
            .finish()
    );

    Ok(view! {
        (status)
        if let Some(css) = theme.custom_css.clone() {
            // Admin-configured, applied as the Vue login page does.
            <style>(TrustedHtml(css))</style>
        }
        <main class="fc-login flex min-h-screen items-center justify-center p-4" style=(style)>
            <div class="w-full max-w-[480px]">
                <div class="mb-8 text-center">
                    if let Some(url) = theme.logo_url.clone() {
                        <img src=(url) alt="Logo" class="mx-auto mb-4 max-h-[72px] max-w-[200px] object-contain">
                    } else if let Some(svg) = theme.logo_svg.clone() {
                        <div class="fc-login-logo-svg mb-4 flex justify-center">(TrustedHtml(svg))</div>
                    } else {
                        <div class="mb-4 inline-flex size-[72px] items-center justify-center rounded-2xl bg-white/10 text-white">
                            <span class="size-10">(default_logo())</span>
                        </div>
                    }
                    if let Some(brand) = brand {
                        <h1 class="m-0 text-[32px] font-bold text-white">(brand)</h1>
                    }
                    if let Some(subtitle) = subtitle {
                        <p class="mt-2 text-base text-[#9fb3c8]">(subtitle)</p>
                    }
                </div>

                <div class="rounded-2xl bg-white p-10 shadow-[0_20px_60px_rgba(0,0,0,0.3)]">
                    <h2 class="mb-6 text-xl font-semibold text-[#102a43]">(title)</h2>

                    if let Some(error) = error {
                        <div class="fc-banner fc-banner-error mb-6" role="alert">(error)</div>
                    }

                    <form method="post" action="/ui/login" class="flex flex-col gap-6">
                        <input type="hidden" name="next" value=(&next)>
                        match step {
                            Step::Email => {
                                <div class="flex flex-col gap-2">
                                    <label for="email" class="text-sm font-medium text-[#334e68]">"Email address"</label>
                                    <input
                                        id="email" name="email" type="email" class="fc-input"
                                        placeholder="you@company.com"
                                        autocomplete="username webauthn" required="" autofocus=""
                                    >
                                    <small class="fc-field-hint">"We'll check if your organization uses single sign-on"</small>
                                </div>
                                <button type="submit" class="fc-btn fc-btn-block fc-login-submit">"Continue"</button>
                            }
                            Step::Password { email } => {
                                <div class="flex items-center justify-between rounded-lg bg-[#f8fafc] px-4 py-3">
                                    <div class="flex min-w-0 items-center gap-3">
                                        <span class="fc-login-avatar">(email.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())</span>
                                        <span class="truncate text-sm text-[#475569]">(&email)</span>
                                    </div>
                                    <a href=(&change_href) class="fc-login-link text-sm font-medium">"Change"</a>
                                </div>
                                <input type="hidden" name="email" value=(&email)>

                                <div class="flex flex-col gap-2">
                                    <label for="password" class="text-sm font-medium text-[#334e68]">"Password"</label>
                                    <div class="relative">
                                        <input
                                            id="password" name="password" class="fc-input pr-10"
                                            placeholder="Enter your password"
                                            autocomplete="current-password" required="" autofocus=""
                                            :type=$(if show_password.get() { "text" } else { "password" })
                                        >
                                        <button
                                            type="button"
                                            class="absolute top-1/2 right-2 flex -translate-y-1/2 items-center p-1 text-[#475569] hover:text-[#1e293b]"
                                            aria-label="Show password"
                                            :aria-pressed=$(if show_password.get() { "true" } else { "false" })
                                            @click=$(|_e: Event| show_password.toggle())
                                        >
                                            <span :hidden=$(show_password.get())>icon(data: iconify_icon!("lucide:eye"), size: Length::rem(1.1))</span>
                                            <span hidden="" :hidden=$(!show_password.get())>icon(data: iconify_icon!("lucide:eye-off"), size: Length::rem(1.1))</span>
                                        </button>
                                    </div>
                                </div>

                                <div class="-mt-2 flex justify-end">
                                    <a href=(forgot_href(&email)) class="fc-login-link text-sm">"Forgot password?"</a>
                                </div>

                                <button type="submit" class="fc-btn fc-btn-block fc-login-submit">"Sign in"</button>

                                <div class="fc-login-divider" data-passkey-only="">"or"</div>
                                <button
                                    type="button"
                                    class="fc-btn fc-btn-outline fc-btn-block"
                                    data-passkey-login=""
                                    data-email=(&email)
                                    data-next=(&next)
                                >
                                    icon(data: iconify_icon!("lucide:key-round"), size: Length::rem(1.0))
                                    "Sign in with a passkey"
                                </button>
                                <p data-passkey-error="" hidden="" class="-mt-3 text-sm text-[#dc2626]"></p>
                            }
                            Step::Setup { email } => {
                                <div class="flex items-center justify-between rounded-lg bg-[#f8fafc] px-4 py-3">
                                    <div class="flex min-w-0 items-center gap-3">
                                        <span class="fc-login-avatar">(email.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default())</span>
                                        <span class="truncate text-sm text-[#475569]">(&email)</span>
                                    </div>
                                    <a href=(&change_href) class="fc-login-link text-sm font-medium">"Use a different email"</a>
                                </div>
                                <div data-setup="" data-email=(&email) class="flex flex-col gap-4">
                                    <p data-setup-ask="" class="text-sm leading-relaxed text-[#475569]">
                                        "This is your first time signing in. We'll email you a link to create your password — this confirms it's really you."
                                    </p>
                                    <button type="button" data-setup-send="" class="fc-btn fc-btn-block fc-login-submit">"Email me a link"</button>
                                    <div data-setup-sent="" hidden="" class="fc-banner fc-banner-success">
                                        <p>"We sent a link to " <strong>(&email)</strong> ". Open it on this device to create your password. The link expires in 72 hours."</p>
                                    </div>
                                    <p data-setup-error="" hidden="" class="text-sm text-[#dc2626]"></p>
                                </div>
                            }
                            Step::TwoFactor { mfa_token, methods, remember_device_allowed } => {
                                let first = methods.first().cloned().unwrap_or_else(|| "RECOVERY_CODE".to_owned());
                                let has_totp = methods.iter().any(|m| m == "TOTP");
                                let has_email = methods.iter().any(|m| m == "EMAIL_PIN");
                                <div data-tfa="" data-token=(&mfa_token) data-active=(&first) data-next=(&next) class="flex flex-col gap-4 text-left">
                                    <div data-tfa-error="" hidden="" class="rounded-md border border-[#fecaca] bg-[#fef2f2] px-3 py-2 text-sm text-[#b91c1c]"></div>
                                    <p data-tfa-panel="TOTP" hidden="" class="text-sm text-[#64748b]">"Enter the 6-digit code from your authenticator app."</p>
                                    <div data-tfa-panel="EMAIL_PIN" hidden="" class="flex flex-col gap-3">
                                        <p data-tfa-email-ask="" class="text-sm text-[#64748b]">"We'll email a one-time code to your address."</p>
                                        <p data-tfa-email-sent="" hidden="" class="text-sm text-[#64748b]">"Enter the code we emailed you."</p>
                                        <button type="button" data-tfa-send="" class="fc-btn fc-btn-block fc-login-submit">
                                            icon(data: iconify_icon!("lucide:mail"), size: Length::rem(1.0))
                                            "Email me a code"
                                        </button>
                                    </div>
                                    <p data-tfa-panel="RECOVERY_CODE" hidden="" class="text-sm text-[#64748b]">"Enter one of your recovery codes."</p>
                                    <div data-tfa-entry="" class="flex flex-col gap-4">
                                        <input data-tfa-code="" class="fc-input" placeholder="123456" inputmode="text" autocomplete="one-time-code" aria-label="Code">
                                        if remember_device_allowed {
                                            <label class="flex items-center gap-2 text-sm text-[#64748b]">
                                                <input type="checkbox" data-tfa-remember="">
                                                "Remember this device for 30 days"
                                            </label>
                                        }
                                        <button type="button" data-tfa-verify="" class="fc-btn fc-btn-block fc-login-submit">"Verify"</button>
                                    </div>
                                    <div class="mt-1 flex flex-col gap-1.5 text-sm">
                                        if has_totp {
                                            <a href="#" data-tfa-switch="TOTP" class="fc-login-link">"Use authenticator app"</a>
                                        }
                                        if has_email {
                                            <a href="#" data-tfa-switch="EMAIL_PIN" class="fc-login-link">"Use an email code"</a>
                                        }
                                        <a href="#" data-tfa-switch="RECOVERY_CODE" class="fc-login-link">"Use a recovery code"</a>
                                    </div>
                                </div>
                            }
                            Step::Enroll { email } => {
                                // Enrolment (authenticator QR or email
                                // confirmation) runs in the SPA's sign-in.
                                <p class="text-sm leading-relaxed text-[#475569]">
                                    "Your organization requires two-factor authentication for "
                                    <strong>(&email)</strong>
                                    ". Set it up to finish signing in."
                                </p>
                                <a href="/auth/login" class="fc-btn fc-btn-block fc-login-submit">"Continue to set up"</a>
                            }
                        }
                    </form>
                </div>

                if let Some(footer) = footer {
                    <p class="mt-6 text-center text-sm text-[#627d98]">(footer)</p>
                }
            </div>
        </main>
        <script type="module" src=(asset!("./passkey.js"))></script>
        <script type="module" src=(asset!("./login_steps.js"))></script>
    })
}
