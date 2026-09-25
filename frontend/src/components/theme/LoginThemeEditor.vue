<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { LoginTheme } from "@/api/config";

/**
 * The login-theme form plus its live preview, shared by the platform-wide
 * Theme Settings page and the per-client Login Branding section on the
 * client drawer. The parent owns the theme object, loading, and saving —
 * this component only edits and previews it.
 */
const props = withDefaults(
	defineProps<{
		/** Side-by-side on wide pages; stacked inside the narrow client drawer. */
		layout?: "split" | "stacked";
		disabled?: boolean;
	}>(),
	{ layout: "split", disabled: false },
);

const theme = defineModel<LoginTheme>({ required: true });

const previewBackground = computed(
	() => theme.value.backgroundGradient || theme.value.backgroundColor,
);

// PrimeVue's ColorPicker works in bare hex (no leading #), so each colour
// is mirrored into a picker-shaped ref. The watcher keeps the pickers in
// step when the theme is replaced wholesale — loaded from the server,
// reset to defaults, or seeded from the platform theme.
const primaryColorPicker = ref("");
const accentColorPicker = ref("");
const backgroundColorPicker = ref("");

watch(
	theme,
	(value) => {
		primaryColorPicker.value = stripHash(value.primaryColor);
		accentColorPicker.value = stripHash(value.accentColor);
		backgroundColorPicker.value = stripHash(value.backgroundColor);
	},
	{ immediate: true },
);

function stripHash(color: string | undefined): string {
	return (color || "").replace("#", "");
}

function onPrimaryColorChange(color: string) {
	theme.value.primaryColor = "#" + color;
	updateGradient();
}

function onAccentColorChange(color: string) {
	theme.value.accentColor = "#" + color;
}

function onBackgroundColorChange(color: string) {
	theme.value.backgroundColor = "#" + color;
	updateGradient();
}

function updateGradient() {
	// Auto-generate gradient from primary and background colors
	theme.value.backgroundGradient = `linear-gradient(135deg, ${theme.value.primaryColor} 0%, ${theme.value.backgroundColor} 100%)`;
}
</script>

<template>
  <div class="theme-editor" :class="`theme-editor--${props.layout}`">
    <!-- Form Section -->
    <div class="fc-form">
      <FcFormSection title="Branding" :flat="props.layout === 'stacked'">
        <div class="fc-form-grid">
          <FcFormField label="Brand Name" help="Displayed as the main heading on the login page">
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" v-model="theme.brandName" :disabled="props.disabled" />
            </template>
          </FcFormField>

          <FcFormField label="Brand Subtitle" help="Displayed below the brand name">
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" v-model="theme.brandSubtitle" :disabled="props.disabled" />
            </template>
          </FcFormField>

          <FcFormField label="Footer Text" span help="Displayed at the bottom of the login form">
            <template #default="{ id: fieldId }">
              <InputText :id="fieldId" v-model="theme.footerText" :disabled="props.disabled" />
            </template>
          </FcFormField>
        </div>
      </FcFormSection>

      <FcFormSection title="Logo" :flat="props.layout === 'stacked'">
        <div class="fc-form-grid">
          <FcFormField label="Logo URL" span help="URL to an image file (PNG, SVG, etc.)">
            <template #default="{ id: fieldId }">
              <InputText
                :id="fieldId"
                v-model="theme.logoUrl"
                :disabled="props.disabled"
                placeholder="https://example.com/logo.png"
              />
            </template>
          </FcFormField>

          <FcFormField
            label="Logo SVG (inline)"
            span
            help="Paste inline SVG markup. Takes precedence over Logo URL if both are set."
          >
            <template #default="{ id: fieldId }">
              <Textarea
                :id="fieldId"
                v-model="theme.logoSvg"
                :disabled="props.disabled"
                rows="4"
                placeholder="<svg>...</svg>"
              />
            </template>
          </FcFormField>

          <FcFormField
            label="Logo Height (px)"
            help="Height of the logo in pixels (default: 40, max: 120)"
          >
            <template #default="{ id: fieldId }">
              <InputText
                :id="fieldId"
                :model-value="theme.logoHeight != null ? String(theme.logoHeight) : ''"
                @update:model-value="
                  (v: string | undefined) => (theme.logoHeight = v ? Number(v) : undefined)
                "
                :disabled="props.disabled"
                type="number"
                class="w-small"
                min="20"
                max="120"
              />
            </template>
          </FcFormField>
        </div>
      </FcFormSection>

      <FcFormSection title="Colors" :flat="props.layout === 'stacked'">
        <div class="fc-form-grid">
          <FcFormField label="Primary Color" help="Main brand color for headings">
            <template #default="{ id: fieldId }">
              <div class="color-input">
                <ColorPicker
                  v-model="primaryColorPicker"
                  :disabled="props.disabled"
                  @update:modelValue="onPrimaryColorChange"
                />
                <InputText
                  :id="fieldId"
                  v-model="theme.primaryColor"
                  :disabled="props.disabled"
                  class="color-text"
                />
              </div>
            </template>
          </FcFormField>

          <FcFormField label="Accent Color" help="Button and link color">
            <template #default="{ id: fieldId }">
              <div class="color-input">
                <ColorPicker
                  v-model="accentColorPicker"
                  :disabled="props.disabled"
                  @update:modelValue="onAccentColorChange"
                />
                <InputText
                  :id="fieldId"
                  v-model="theme.accentColor"
                  :disabled="props.disabled"
                  class="color-text"
                />
              </div>
            </template>
          </FcFormField>

          <FcFormField label="Background Color" help="Page background color">
            <template #default="{ id: fieldId }">
              <div class="color-input">
                <ColorPicker
                  v-model="backgroundColorPicker"
                  :disabled="props.disabled"
                  @update:modelValue="onBackgroundColorChange"
                />
                <InputText
                  :id="fieldId"
                  v-model="theme.backgroundColor"
                  :disabled="props.disabled"
                  class="color-text"
                />
              </div>
            </template>
          </FcFormField>

          <FcFormField
            label="Background Gradient"
            span
            help="CSS gradient (overrides background color if set)"
          >
            <template #default="{ id: fieldId }">
              <InputText
                :id="fieldId"
                v-model="theme.backgroundGradient"
                :disabled="props.disabled"
              />
            </template>
          </FcFormField>
        </div>
      </FcFormSection>

      <FcFormSection title="Advanced" :flat="props.layout === 'stacked'">
        <div class="fc-form-grid">
          <FcFormField
            label="Custom CSS"
            span
            help="Additional CSS rules to inject on the login page"
          >
            <template #default="{ id: fieldId }">
              <Textarea
                :id="fieldId"
                v-model="theme.customCss"
                :disabled="props.disabled"
                rows="4"
                placeholder=".login-card { ... }"
              />
            </template>
          </FcFormField>
        </div>

        <slot name="actions" />
      </FcFormSection>
    </div>

    <!-- Preview Section -->
    <div class="preview-section">
      <h3 class="preview-title">Preview</h3>
      <div class="preview-container" :style="{ background: previewBackground }">
        <div class="preview-content">
          <!-- Logo preview -->
          <img
            v-if="theme.logoUrl && !theme.logoSvg"
            :src="theme.logoUrl"
            class="preview-logo-img"
            alt="Logo"
          />
          <div v-else-if="theme.logoSvg" class="preview-logo-svg" v-html="theme.logoSvg" />
          <div v-else class="preview-logo-default">
            <svg fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="1.5"
                d="M13 10V3L4 14h7v7l9-11h-7z"
              />
            </svg>
          </div>

          <h1 class="preview-brand">{{ theme.brandName }}</h1>
          <p class="preview-subtitle">{{ theme.brandSubtitle }}</p>

          <div class="preview-card">
            <h2 class="preview-card-title">Sign in</h2>
            <div class="preview-input"></div>
            <button class="preview-button" :style="{ backgroundColor: theme.accentColor }">
              Continue
            </button>
          </div>

          <p class="preview-footer">{{ theme.footerText }}</p>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.theme-editor {
  display: grid;
  gap: 24px;
}

.theme-editor--split {
  grid-template-columns: 1fr 400px;
}

.theme-editor--stacked {
  grid-template-columns: 1fr;
}

@media (max-width: 1200px) {
  .theme-editor--split {
    grid-template-columns: 1fr;
  }
}

/* Fixed-width inputs inside form fields (beat the kit's global width: 100%) */
.fc-form-field .w-small {
  width: 120px;
}

.color-input {
  display: flex;
  align-items: center;
  gap: 8px;
}

.fc-form-field .color-text {
  width: 100px;
  font-family: monospace;
}

/* Preview styles */
.theme-editor--split .preview-section {
  position: sticky;
  top: 24px;
  align-self: start;
}

.preview-title {
  font-size: 14px;
  font-weight: 600;
  color: #64748b;
  margin: 0 0 12px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.preview-container {
  border-radius: 12px;
  padding: 32px 24px;
  min-height: 500px;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.15);
}

.theme-editor--stacked .preview-container {
  min-height: 380px;
}

.preview-content {
  text-align: center;
  width: 100%;
  max-width: 280px;
}

.preview-logo-img {
  max-width: 120px;
  max-height: 48px;
  object-fit: contain;
  margin-bottom: 12px;
}

.preview-logo-svg {
  margin-bottom: 12px;
}

.preview-logo-svg :deep(svg) {
  max-width: 120px;
  max-height: 48px;
}

.preview-logo-default {
  width: 48px;
  height: 48px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  margin-bottom: 12px;
}

.preview-logo-default svg {
  width: 28px;
  height: 28px;
  color: white;
}

.preview-brand {
  font-size: 22px;
  font-weight: 700;
  margin: 0;
  color: white;
}

.preview-subtitle {
  color: #9fb3c8;
  font-size: 13px;
  margin: 4px 0 20px;
}

.preview-card {
  background: white;
  border-radius: 10px;
  padding: 24px 20px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
}

.preview-card-title {
  font-size: 15px;
  font-weight: 600;
  color: #1e293b;
  margin: 0 0 16px;
  text-align: left;
}

.preview-input {
  height: 36px;
  background: #f1f5f9;
  border-radius: 6px;
  margin-bottom: 12px;
}

.preview-button {
  width: 100%;
  height: 36px;
  border: none;
  border-radius: 6px;
  color: white;
  font-weight: 500;
  font-size: 13px;
  cursor: default;
}

.preview-footer {
  color: #627d98;
  font-size: 11px;
  margin: 16px 0 0;
}
</style>
