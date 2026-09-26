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
    if (Number.isNaN(at.getTime())) continue;
    // data-local="date": `toLocaleDateString()`, as the SPA's list columns.
    el.textContent = el.dataset.local === "date" ? at.toLocaleDateString() : at.toLocaleString();
  }
  // Date-only cells (`toLocaleDateString()`): the server renders YYYY-MM-DD.
  for (const el of document.querySelectorAll("time[data-local-date]")) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(el.textContent)) continue;
    const at = new Date(el.getAttribute("datetime"));
    if (!Number.isNaN(at.getTime())) el.textContent = at.toLocaleDateString();
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

// A dialog the server rendered to be seen at once (credentials shown a
// single time after provisioning) opens as a modal on load.
// Like the SPA's (closable=false), only its own button dismisses it.
for (const d of document.querySelectorAll("dialog[data-open-on-load]")) {
  d.addEventListener("cancel", (e) => e.preventDefault());
  d.showModal();
}
// A dialog the server reopened with an error in it (a refused form):
// closable as usual.
for (const d of document.querySelectorAll("dialog[data-show-on-load]")) d.showModal();

// Topcoat UI's dialogs fill the viewport with their own mask, so the
// browser never sees a backdrop click: a click on the mask itself (not the
// panel) closes a `data-light-dismiss` dialog.
document.addEventListener("click", (e) => {
  const d = e.target;
  if (d instanceof HTMLDialogElement && d.open && d.matches("[data-light-dismiss]")) d.close();
});

// Escape closes the record drawer (EntityDrawer), unless a dialog or a
// popover is open: those close first, natively.
// The drawer can itself be a (non-modal) <dialog>: Topcoat UI's sheet.
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape" || e.defaultPrevented) return;
  if (document.querySelector("dialog[open]:not([data-drawer]), [popover]:popover-open")) return;
  const drawer = [...document.querySelectorAll("[data-drawer]")].find(
    (d) => !d.hidden && (!(d instanceof HTMLDialogElement) || d.open),
  );
  drawer?.querySelector("[data-drawer-close]")?.click();
});

// Dual-pane pickers (`data-mirror` forms): the right pane shows what is
// ticked on the left, with a count. Server-rendered right for the initial
// state; this keeps it in step.
function mirror(form) {
  let count = 0;
  for (const el of form.querySelectorAll("[data-mirror-of]")) {
    const box = document.getElementById(el.dataset.mirrorOf);
    const on = !!box?.checked;
    el.hidden = !on;
    if (on) count++;
  }
  for (const el of form.querySelectorAll("[data-mirror-count]")) el.textContent = String(count);
  for (const el of form.querySelectorAll("[data-mirror-empty]")) el.hidden = count > 0;
}
document.addEventListener("change", (e) => {
  const form = e.target instanceof Element ? e.target.closest("form[data-mirror]") : null;
  if (form) mirror(form);
});
document.addEventListener("reset", (e) => {
  if (e.target instanceof HTMLFormElement && e.target.matches("[data-mirror]")) {
    setTimeout(() => mirror(e.target));
  }
});

// Filter boxes (`data-filter-list="<list id>"`): hide the list's rows whose
// `data-filter-text` doesn't contain the query.
document.addEventListener("input", (e) => {
  const box = e.target;
  if (!(box instanceof HTMLInputElement) || !box.dataset.filterList) return;
  const list = document.getElementById(box.dataset.filterList);
  if (!list) return;
  const q = box.value.trim().toLowerCase();
  let shown = 0;
  for (const row of list.querySelectorAll("[data-filter-text]")) {
    const hit = !q || row.dataset.filterText.includes(q);
    row.hidden = !hit;
    if (hit) shown++;
  }
  for (const el of list.querySelectorAll("[data-filter-empty]")) el.hidden = shown > 0;
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
