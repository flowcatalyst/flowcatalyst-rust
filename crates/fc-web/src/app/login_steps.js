// The login steps that only the platform's JSON endpoints can finish:
// the second-factor challenge (TwoFactorChallenge.vue: POST /auth/2fa/verify,
// POST /auth/2fa/challenge/email) and the first-time password setup email
// (POST /auth/password-setup/request). Same requests the SPA makes; the
// verify response sets the session cookie.

async function postJson(path, body) {
  const res = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.message || data.error || "Something went wrong. Try again.");
  return data;
}

const tfa = document.querySelector("[data-tfa]");
if (tfa) {
  const token = tfa.dataset.token;
  const error = tfa.querySelector("[data-tfa-error]");
  const entry = tfa.querySelector("[data-tfa-entry]");
  const code = tfa.querySelector("[data-tfa-code]");
  const send = tfa.querySelector("[data-tfa-send]");
  let active = tfa.dataset.active;
  let emailSent = false;

  const show = (message) => {
    error.textContent = message;
    error.hidden = !message;
  };
  const render = () => {
    for (const p of tfa.querySelectorAll("[data-tfa-panel]")) p.hidden = p.dataset.tfaPanel !== active;
    for (const a of tfa.querySelectorAll("[data-tfa-switch]")) a.hidden = a.dataset.tfaSwitch === active;
    tfa.querySelector("[data-tfa-email-ask]").hidden = emailSent;
    tfa.querySelector("[data-tfa-email-sent]").hidden = !emailSent;
    send.hidden = emailSent;
    entry.hidden = active === "EMAIL_PIN" && !emailSent;
    code.placeholder = active === "RECOVERY_CODE" ? "XXXXX-XXXXX" : "123456";
  };
  for (const a of tfa.querySelectorAll("[data-tfa-switch]")) {
    a.addEventListener("click", (e) => {
      e.preventDefault();
      active = a.dataset.tfaSwitch;
      code.value = "";
      show("");
      render();
    });
  }
  send.addEventListener("click", async () => {
    send.disabled = true;
    try {
      await postJson("/auth/2fa/challenge/email", { mfaToken: token });
      emailSent = true;
      show("");
      render();
    } catch (e) {
      show(e.message);
    } finally {
      send.disabled = false;
    }
  });
  const verify = async () => {
    if (!code.value.trim()) return;
    const button = tfa.querySelector("[data-tfa-verify]");
    button.disabled = true;
    try {
      await postJson("/auth/2fa/verify", {
        mfaToken: token,
        method: active,
        code: code.value.trim(),
        rememberDevice: !!tfa.querySelector("[data-tfa-remember]")?.checked,
      });
      window.location.assign(tfa.dataset.next || "/ui");
    } catch (e) {
      show(e.message);
      button.disabled = false;
    }
  };
  tfa.querySelector("[data-tfa-verify]").addEventListener("click", verify);
  code.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      verify();
    }
  });
  render();
}

const setup = document.querySelector("[data-setup]");
if (setup) {
  const button = setup.querySelector("[data-setup-send]");
  const error = setup.querySelector("[data-setup-error]");
  button.addEventListener("click", async () => {
    button.disabled = true;
    error.hidden = true;
    try {
      await postJson("/auth/password-setup/request", { email: setup.dataset.email });
      button.hidden = true;
      setup.querySelector("[data-setup-ask]").hidden = true;
      setup.querySelector("[data-setup-sent]").hidden = false;
    } catch (e) {
      error.textContent = e.message || "Could not send the email — please try again.";
      error.hidden = false;
      button.disabled = false;
    }
  });
}
