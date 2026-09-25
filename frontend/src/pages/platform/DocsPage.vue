<script setup lang="ts">
import { ref, computed, watch, nextTick } from "vue";
import { useRoute, useRouter } from "vue-router";
import { marked } from "marked";
import DOMPurify from "dompurify";
import { docsApi, type DocListResponse } from "@/api/docs";
import { getErrorMessage } from "@/utils/errors";

const route = useRoute();
const router = useRouter();

const index = ref<DocListResponse | null>(null);
const listLoading = ref(true);
const listError = ref<string | null>(null);
const filter = ref("");

const docLoading = ref(false);
const docError = ref<string | null>(null);
const docHtml = ref("");
const contentEl = ref<HTMLElement | null>(null);

// Source is "platform" or an application code; together with the slug it
// addresses one page: /platform/docs/{source}/{slug}.
const activeSource = computed(() => (route.params["source"] as string) || null);
const activeSlug = computed(() => (route.params["slug"] as string) || null);

function matches(title: string, slug: string): boolean {
	const q = filter.value.trim().toLowerCase();
	if (!q) return true;
	return title.toLowerCase().includes(q) || slug.toLowerCase().includes(q);
}

const filteredPlatform = computed(
	() => index.value?.platform.filter((d) => matches(d.title, d.slug)) ?? [],
);
const filteredApplications = computed(
	() =>
		index.value?.applications
			.map((g) => ({
				...g,
				docs: g.docs.filter((d) => matches(d.title, d.slug)),
			}))
			.filter((g) => g.docs.length > 0) ?? [],
);

async function loadList() {
	listLoading.value = true;
	listError.value = null;
	try {
		index.value = await docsApi.list();
		const first = index.value.platform[0];
		if (!activeSlug.value && first) {
			void router.replace(`/platform/docs/platform/${first.slug}`);
		}
	} catch (e) {
		listError.value = getErrorMessage(e, "Failed to load documentation");
	} finally {
		listLoading.value = false;
	}
}

async function loadDoc(source: string, slug: string) {
	docLoading.value = true;
	docError.value = null;
	docHtml.value = "";
	try {
		const doc =
			source === "platform"
				? await docsApi.getPlatform(slug)
				: await docsApi.getApplication(source, slug);
		// Render markdown → sanitize → inject; mermaid fences are upgraded to
		// SVG afterwards (the sanitizer would mangle inline SVG, so diagrams
		// render post-sanitize from the fence text).
		const raw = marked.parse(doc.content, { async: false }) as string;
		docHtml.value = DOMPurify.sanitize(raw);
	} catch (e) {
		docError.value = getErrorMessage(e, "Failed to load document");
	} finally {
		// The article is v-else-gated on docLoading: it must MOUNT before the
		// mermaid pass can find the fences, so clear loading first, wait a
		// tick, then upgrade. (Running the pass before this point silently
		// did nothing — the classic reason diagrams show as raw text.)
		docLoading.value = false;
	}
	if (!docError.value) {
		await nextTick();
		await renderMermaid();
	}
}

// Upgrade ```mermaid fences (rendered as <pre><code class="language-mermaid">)
// into SVG diagrams. Mermaid is lazy-loaded so the docs page costs nothing
// unless a diagram is actually on screen (same pattern as the process pages).
async function renderMermaid() {
	const host = contentEl.value;
	if (!host) return;
	const fences = host.querySelectorAll("pre code.language-mermaid");
	if (fences.length === 0) return;
	const mermaid = (await import("mermaid")).default;
	mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: "neutral" });
	let i = 0;
	for (const fence of Array.from(fences)) {
		const pre = fence.closest("pre");
		if (!pre?.parentElement) continue;
		const code = fence.textContent ?? "";
		try {
			const { svg } = await mermaid.render(`doc-diagram-${i++}`, code);
			const wrap = document.createElement("div");
			wrap.className = "doc-diagram";
			wrap.title = "Click to enlarge";
			wrap.innerHTML = svg;
			// Click-to-zoom: the inline diagram is constrained to the prose
			// column, so a click opens the same SVG in a near-fullscreen
			// dialog where it can breathe.
			wrap.addEventListener("click", () => openDiagramZoom(svg));
			pre.replaceWith(wrap);
		} catch {
			// Leave the fence as a code block if it doesn't parse.
		}
	}
}

// Diagram lightbox: the clicked diagram's SVG, shown near-fullscreen.
const zoomVisible = ref(false);
const zoomSvg = ref("");

function openDiagramZoom(svg: string) {
	zoomSvg.value = svg;
	zoomVisible.value = true;
}

// Cross-doc links: a markdown link to `some-doc.md` navigates within the
// current source instead of 404ing; external links leave in a new tab.
function onContentClick(event: MouseEvent) {
	const anchor = (event.target as HTMLElement).closest("a");
	if (!anchor) return;
	const href = anchor.getAttribute("href") ?? "";
	const match = href.match(
		/^(?:\.\/)?(?:docs\/)?(?:published\/)?(?:\d+-)?([a-zA-Z0-9_-]+)\.md(?:#.*)?$/,
	);
	if (match?.[1] && activeSource.value) {
		event.preventDefault();
		void router.push(`/platform/docs/${activeSource.value}/${match[1]}`);
		return;
	}
	if (/^https?:\/\//.test(href)) {
		event.preventDefault();
		window.open(href, "_blank", "noopener");
	}
}

watch([activeSource, activeSlug], ([source, slug]) => {
	if (source && slug) void loadDoc(source, slug);
});

void loadList();
if (activeSource.value && activeSlug.value) {
	void loadDoc(activeSource.value, activeSlug.value);
}
</script>

<template>
  <div class="page-container docs-page">
    <header class="page-header">
      <div>
        <h1 class="page-title">Documentation</h1>
        <p class="page-subtitle">
          The platform's published docs plus each application's synced pages.
        </p>
      </div>
    </header>

    <Message v-if="listError" severity="error" class="error-message">{{ listError }}</Message>

    <div v-else class="docs-layout">
      <aside class="docs-nav">
        <InputText v-model="filter" placeholder="Filter docs..." class="docs-filter" />
        <div v-if="listLoading" class="docs-nav-loading">
          <i class="pi pi-spinner pi-spin"></i>
        </div>
        <nav v-else class="docs-list">
          <div class="docs-group-label">Platform</div>
          <router-link
            v-for="d in filteredPlatform"
            :key="`platform:${d.slug}`"
            :to="`/platform/docs/platform/${d.slug}`"
            class="docs-link"
            :class="{ active: activeSource === 'platform' && d.slug === activeSlug }"
          >
            {{ d.title }}
          </router-link>

          <template v-for="g in filteredApplications" :key="g.applicationCode">
            <div class="docs-group-label">{{ g.applicationName }}</div>
            <router-link
              v-for="d in g.docs"
              :key="`${g.applicationCode}:${d.slug}`"
              :to="`/platform/docs/${g.applicationCode}/${d.slug}`"
              class="docs-link"
              :class="{
                active: activeSource === g.applicationCode && d.slug === activeSlug,
              }"
            >
              {{ d.title }}
            </router-link>
          </template>
        </nav>
      </aside>

      <main class="docs-content-pane">
        <div v-if="docLoading" class="docs-content-loading">
          <i class="pi pi-spinner pi-spin"></i> Loading…
        </div>
        <Message v-else-if="docError" severity="error">{{ docError }}</Message>
        <!-- eslint-disable-next-line vue/no-v-html — sanitized above -->
        <article
          v-else
          ref="contentEl"
          class="docs-content"
          @click="onContentClick"
          v-html="docHtml"
        ></article>
      </main>
    </div>

    <!-- Diagram lightbox -->
    <Dialog
      v-model:visible="zoomVisible"
      modal
      dismissable-mask
      :show-header="false"
      class="doc-zoom-dialog"
      :style="{ width: '94vw', maxWidth: '1600px' }"
      @hide="zoomSvg = ''"
    >
      <!-- eslint-disable-next-line vue/no-v-html — mermaid output, rendered locally -->
      <div class="doc-zoom" @click="zoomVisible = false" v-html="zoomSvg"></div>
    </Dialog>
  </div>
</template>

<style scoped>
.docs-layout {
  display: grid;
  grid-template-columns: 260px minmax(0, 1fr);
  gap: 24px;
  align-items: start;
}

.docs-nav {
  position: sticky;
  top: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.docs-filter {
  width: 100%;
}

.docs-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.docs-group-label {
  margin: 12px 0 4px;
  padding: 0 10px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-color-secondary, #64748b);
}

.docs-group-label:first-child {
  margin-top: 0;
}

.docs-link {
  padding: 7px 10px;
  border-radius: 6px;
  color: var(--text-color, #334155);
  text-decoration: none;
  font-size: 13.5px;
  line-height: 1.35;
}

.docs-link:hover {
  background: var(--surface-hover, #f1f5f9);
}

.docs-link.active {
  background: var(--highlight-bg, #eef2ff);
  color: var(--primary-color, #4f46e5);
  font-weight: 600;
}

.docs-nav-loading,
.docs-content-loading {
  color: var(--text-color-secondary, #64748b);
  padding: 16px 0;
}

.docs-content-pane {
  min-width: 0;
  background: var(--surface-card, #ffffff);
  border: 1px solid var(--surface-border, #e2e8f0);
  border-radius: 8px;
  padding: 28px 32px;
}

/* Rendered markdown — prose stays at a readable measure; diagrams open
   full-size via click-to-zoom instead of widening the column. */
.docs-content {
  max-width: 76ch;
  line-height: 1.65;
  font-size: 14.5px;
  color: var(--text-color, #1e293b);
}

.docs-content :deep(h1) {
  font-size: 1.7rem;
  margin: 0 0 0.6em;
}

.docs-content :deep(h2) {
  font-size: 1.25rem;
  margin: 1.6em 0 0.5em;
  padding-top: 0.6em;
  border-top: 1px solid var(--surface-border, #e2e8f0);
}

.docs-content :deep(h3) {
  font-size: 1.05rem;
  margin: 1.3em 0 0.4em;
}

.docs-content :deep(p) {
  margin: 0.7em 0;
}

.docs-content :deep(code) {
  background: var(--surface-ground, #f1f5f9);
  padding: 1px 5px;
  border-radius: 4px;
  font-size: 0.9em;
}

.docs-content :deep(pre) {
  background: var(--surface-ground, #f1f5f9);
  border: 1px solid var(--surface-border, #e2e8f0);
  border-radius: 6px;
  padding: 12px 14px;
  overflow-x: auto;
}

.docs-content :deep(pre code) {
  background: none;
  padding: 0;
}

.docs-content :deep(table) {
  border-collapse: collapse;
  margin: 1em 0;
  display: block;
  overflow-x: auto;
}

.docs-content :deep(th),
.docs-content :deep(td) {
  border: 1px solid var(--surface-border, #e2e8f0);
  padding: 6px 10px;
  text-align: left;
  vertical-align: top;
}

.docs-content :deep(th) {
  background: var(--surface-ground, #f8fafc);
}

.docs-content :deep(blockquote) {
  margin: 1em 0;
  padding: 4px 14px;
  border-left: 3px solid var(--primary-color, #4f46e5);
  color: var(--text-color-secondary, #475569);
}

.docs-content :deep(.doc-diagram) {
  margin: 1.2em 0;
  overflow-x: auto;
  cursor: zoom-in;
  border-radius: 6px;
  transition: background-color 0.15s ease;
}

.docs-content :deep(.doc-diagram:hover) {
  background: var(--surface-hover, #f8fafc);
}

.docs-content :deep(.doc-diagram svg) {
  max-width: 100%;
  height: auto;
}

.doc-zoom {
  cursor: zoom-out;
  display: flex;
  justify-content: center;
  padding: 12px;
}

.doc-zoom :deep(svg) {
  width: 100%;
  height: auto;
  max-height: 86vh;
  /* Mermaid inlines max-width at the diagram's natural size — the whole
     point of the lightbox is to exceed it. */
  max-width: 100% !important;
}

@media (max-width: 900px) {
  .docs-layout {
    grid-template-columns: 1fr;
  }

  .docs-nav {
    position: static;
  }
}
</style>
