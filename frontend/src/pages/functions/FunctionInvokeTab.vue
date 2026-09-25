<script setup lang="ts">
// Invoke: a developer aid that makes NO network call. The SPA talks to the
// platform, and the function host is a separate origin it can't reach
// reliably, so this renders ready-made curl lines for the host's private
// entry (`/functions/<address>[:<version>]<path>`) with copy buttons.
import { computed, ref, watch } from "vue";
import { toast } from "@/utils/errorBus";
import type { VersionResponse } from "@/api/functions";

const props = defineProps<{
	address: string;
	versions: VersionResponse[];
	liveVersion?: number;
}>();

const path = ref("/hello");
const selectedVersion = ref<number | null>(null);

const versionOptions = computed(() =>
	props.versions.map((v) => ({ label: `v${v.version} (${v.state})`, value: v.version })),
);

// Default to live, else the newest version, once the list arrives.
watch(
	() => [props.versions, props.liveVersion] as const,
	() => {
		if (selectedVersion.value !== null) return;
		selectedVersion.value = props.liveVersion ?? props.versions[0]?.version ?? null;
	},
	{ immediate: true },
);

const normalizedPath = computed(() => {
	const p = path.value.trim();
	if (!p) return "/";
	return p.startsWith("/") ? p : `/${p}`;
});

const liveCurl = computed(
	() => `curl "$FC_FN_HOST_URL/functions/${props.address}${normalizedPath.value}"`,
);

const versionCurl = computed(() => {
	if (selectedVersion.value === null) return "";
	return (
		`curl -H "Authorization: Bearer $TOKEN" ` +
		`"$FC_FN_HOST_URL/functions/${props.address}:${selectedVersion.value}${normalizedPath.value}"`
	);
});

function copy(text: string) {
	if (!text) return;
	void navigator.clipboard.writeText(text);
	toast.info("Copied", "Command copied to clipboard");
}
</script>

<template>
  <div class="invoke-tab">
    <Message severity="info" :closable="false" class="invoke-note">
      Nothing here calls the platform. Run these yourself against a function host of this
      function's pool (<code>$FC_FN_HOST_URL</code>).
    </Message>

    <div class="section-card">
      <div class="card-header"><h3>Request</h3></div>
      <div class="card-content">
        <div class="form-field">
          <label for="invoke-path">Path</label>
          <InputText id="invoke-path" v-model="path" placeholder="/hello" class="path-input" />
        </div>
      </div>
    </div>

    <div class="section-card">
      <div class="card-header"><h3>Private entry: live version</h3></div>
      <div class="card-content">
        <div class="command-row">
          <pre data-testid="invoke-live-curl">{{ liveCurl }}</pre>
          <Button icon="pi pi-copy" text size="small" v-tooltip="'Copy'" @click="copy(liveCurl)" />
        </div>
        <small class="hint">
          An endpoint with <code>auth: platform</code> also needs
          <code>-H "Authorization: Bearer $TOKEN"</code>.
        </small>
      </div>
    </div>

    <div class="section-card">
      <div class="card-header"><h3>Private entry: a specific version</h3></div>
      <div class="card-content">
        <p class="hint section-intro">
          Needs <code>platform:function:version:invoke</code> and a bearer token. The host serves a
          version it has loaded: the live one, or a published candidate.
        </p>
        <div class="form-field">
          <label for="invoke-version">Version</label>
          <Select
            v-model="selectedVersion"
            inputId="invoke-version"
            :options="versionOptions"
            optionLabel="label"
            optionValue="value"
            placeholder="No versions"
            class="version-select"
          />
        </div>
        <div v-if="versionCurl" class="command-row">
          <pre data-testid="invoke-version-curl">{{ versionCurl }}</pre>
          <Button icon="pi pi-copy" text size="small" v-tooltip="'Copy'" @click="copy(versionCurl)" />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.invoke-note {
  margin-bottom: 16px;
}

.section-card {
  margin-bottom: 16px;
  background: var(--surface-card, white);
  border-radius: 8px;
  border: 1px solid var(--surface-border);
  overflow: hidden;
}

.card-header {
  padding: 12px 20px;
  border-bottom: 1px solid var(--surface-border);
}

.card-header h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}

.card-content {
  padding: 20px;
}

.form-field label {
  display: block;
  margin-bottom: 6px;
  font-weight: 500;
}

.form-field {
  margin-bottom: 12px;
}

.path-input,
.version-select {
  min-width: 280px;
}

.section-intro {
  margin: 0 0 12px;
}

.hint {
  font-size: 12px;
  color: var(--text-color-secondary);
}

.command-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.command-row pre {
  flex: 1;
  margin: 0;
  padding: 8px 12px;
  background: #0f172a;
  color: #e2e8f0;
  border-radius: 6px;
  font-size: 12px;
  overflow-x: auto;
}
</style>
