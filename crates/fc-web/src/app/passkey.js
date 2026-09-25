// Passkey sign-in for the server-rendered login page.
//
// The WebAuthn ceremony has to run in the browser (`navigator.credentials`),
// so this is the one piece of hand-written script in the login flow. It
// talks to the same endpoints the Vue app uses (`/auth/webauthn/authenticate/
// {begin,complete}`); on success the server has set the `fc_session` cookie
// and we navigate to the page the user was heading for.
//
// Uses the browser's own JSON helpers (`parseRequestOptionsFromJSON`,
// `credential.toJSON()`) where present, with a small base64url fallback, so
// no @simplewebauthn/browser dependency is needed.

const b64urlToBytes = (s) => {
  const pad = "=".repeat((4 - (s.length % 4)) % 4);
  const bin = atob((s + pad).replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
};

const bytesToB64url = (buf) =>
  btoa(String.fromCharCode(...new Uint8Array(buf)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");

function requestOptions(json) {
  if (typeof PublicKeyCredential.parseRequestOptionsFromJSON === "function") {
    return PublicKeyCredential.parseRequestOptionsFromJSON(json);
  }
  return {
    ...json,
    challenge: b64urlToBytes(json.challenge),
    allowCredentials: (json.allowCredentials || []).map((c) => ({
      ...c,
      id: b64urlToBytes(c.id),
    })),
  };
}

function credentialJson(cred) {
  if (typeof cred.toJSON === "function") return cred.toJSON();
  const r = cred.response;
  return {
    id: cred.id,
    rawId: bytesToB64url(cred.rawId),
    type: cred.type,
    authenticatorAttachment: cred.authenticatorAttachment ?? undefined,
    clientExtensionResults: cred.getClientExtensionResults(),
    response: {
      clientDataJSON: bytesToB64url(r.clientDataJSON),
      authenticatorData: bytesToB64url(r.authenticatorData),
      signature: bytesToB64url(r.signature),
      userHandle: r.userHandle ? bytesToB64url(r.userHandle) : undefined,
    },
  };
}

async function postJson(path, body) {
  const res = await fetch(`/auth/webauthn${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "include",
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.message || err.error || "Passkey sign-in failed.");
  }
  return res.json();
}

async function signIn(button) {
  const errorEl = document.querySelector("[data-passkey-error]");
  if (errorEl) errorEl.hidden = true;
  button.disabled = true;
  try {
    const begin = await postJson("/authenticate/begin", { email: button.dataset.email });
    const credential = await navigator.credentials.get({
      publicKey: requestOptions(begin.options.publicKey),
    });
    await postJson("/authenticate/complete", {
      stateId: begin.stateId,
      credential: credentialJson(credential),
    });
    window.location.assign(button.dataset.next || "/ui");
  } catch (e) {
    if (errorEl) {
      errorEl.textContent =
        e.name === "NotAllowedError" ? "Passkey sign-in was cancelled." : e.message;
      errorEl.hidden = false;
    }
    button.disabled = false;
  }
}

const button = document.querySelector("[data-passkey-login]");
if (button && window.PublicKeyCredential) {
  button.addEventListener("click", () => signIn(button));
} else if (button) {
  // No WebAuthn (old browser, insecure origin): hide the option entirely.
  button.hidden = true;
  for (const el of document.querySelectorAll("[data-passkey-only]")) el.hidden = true;
}
