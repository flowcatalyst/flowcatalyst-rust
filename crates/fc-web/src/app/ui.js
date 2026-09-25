// Progressive enhancements for the server-rendered UI. Every page works
// without this file. Everything is delegated from `document`, because the
// runtime morphs shard content (a drawer's body) in place.

// Timestamps: the server renders UTC; the SPA shows `toLocaleString()`.
// Watch text and attribute changes too, and only rewrite text that still
// reads as the server's UTC.
function localise() {
  for (const el of document.querySelectorAll("time[data-local]")) {
    if (!el.textContent.endsWith(" UTC")) continue;
    const at = new Date(el.getAttribute("datetime"));
    if (!Number.isNaN(at.getTime())) el.textContent = at.toLocaleString();
  }
}

localise();
new MutationObserver(localise).observe(document.body, {
  childList: true,
  subtree: true,
  characterData: true,
  attributes: true,
  attributeFilter: ["datetime"],
});

// Escape closes the record drawer (EntityDrawer), unless a dialog or a
// popover is open: those close first, natively.
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || e.defaultPrevented) return;
  if (document.querySelector("dialog[open], [popover]:popover-open")) return;
  const drawer = [...document.querySelectorAll("[data-drawer]")].find((d) => !d.hidden);
  drawer?.querySelector("[data-drawer-close]")?.click();
});

// The Filters popover opens under its button, right-aligned to it, as
// PrimeVue's Popover does (a native popover centres itself otherwise).
document.addEventListener(
  "toggle",
  (e) => {
    const pop = e.target;
    if (!(pop instanceof HTMLElement) || !pop.matches("[data-anchor-popover]")) return;
    if (e.newState !== "open") return;
    const trigger = document.querySelector(`[popovertarget="${pop.id}"]`);
    if (!trigger) return;
    const r = trigger.getBoundingClientRect();
    const width = pop.offsetWidth;
    pop.style.top = `${r.bottom + 6}px`;
    pop.style.left = `${Math.max(16, r.right - width)}px`;
  },
  true,
);

// Edit forms (useDirtyForm): Save stays disabled and Discard hidden until
// a value differs from what the form was rendered with.
const snapshots = new WeakMap();
const serialise = (form) => new URLSearchParams(new FormData(form)).toString();

function dirtyState(form) {
  if (!snapshots.has(form)) return;
  const dirty = serialise(form) !== snapshots.get(form);
  for (const el of document.querySelectorAll(`[data-dirty-save][form="${form.id}"]`)) {
    el.disabled = !dirty;
  }
  for (const el of document.querySelectorAll(`[data-dirty-discard][form="${form.id}"]`)) {
    el.hidden = !dirty;
  }
}

function watched(target) {
  const form = target instanceof Element ? target.closest("form[data-dirty-form]") : null;
  if (form && !snapshots.has(form)) snapshots.set(form, serialise(form));
  return form;
}

// Snapshot every edit form as it appears (and again when a drawer
// re-renders it for another record), before anyone types in it.
function snapshotForms() {
  for (const form of document.querySelectorAll("form[data-dirty-form]")) {
    const key = form.dataset.dirtyKey ?? "";
    if (!snapshots.has(form) || form.dataset.dirtySeen !== key) {
      form.dataset.dirtySeen = key;
      snapshots.set(form, serialise(form));
      dirtyState(form);
    }
  }
}
snapshotForms();
new MutationObserver(snapshotForms).observe(document.body, {
  childList: true,
  subtree: true,
  attributes: true,
  attributeFilter: ["data-dirty-key"],
});

for (const type of ["input", "change"]) {
  document.addEventListener(type, (e) => {
    const form = watched(e.target);
    if (form) dirtyState(form);
  });
}
document.addEventListener("reset", (e) => {
  const form = e.target;
  if (form instanceof HTMLFormElement && form.matches("[data-dirty-form]")) {
    setTimeout(() => dirtyState(form));
  }
});
