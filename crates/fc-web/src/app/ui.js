// Progressive enhancements for the server-rendered UI. Every page works
// without this file; it only localises timestamps (the server renders UTC,
// the Vue app shows `toLocaleString()`), including ones a shard patches in.
// The runtime morphs shard content in place, so watch text and attribute
// changes too, and only rewrite text that still reads as the server's UTC.
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
