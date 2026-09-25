<script setup lang="ts">
import { ref } from "vue";
import {
	enrollEmailBegin,
	enrollEmailConfirm,
	enrollTotpBegin,
	enrollTotpConfirm,
	redirectAfterLogin,
	selfEnrollEmailBegin,
	selfEnrollEmailConfirm,
	selfEnrollTotpBegin,
	selfEnrollTotpConfirm,
	type TotpEnrollment,
	type TwoFactorMethod,
} from "@/api/twofactor";
import { getErrorMessage } from "@/utils/errors";

const props = defineProps<{
	// When set, enrollment runs in the post-login "enroll token" mode and the
	// session is completed on confirm. When absent, it runs in session mode
	// (Profile) for an already-authenticated user.
	enrollToken?: string;
	allowedMethods: TwoFactorMethod[];
}>();

const emit = defineEmits<{ (e: "done"): void }>();

type Stage = "choose" | "totp" | "email" | "recovery";
const stage = ref<Stage>("choose");
const busy = ref(false);
const error = ref<string | null>(null);

const totp = ref<TotpEnrollment | null>(null);
const code = ref("");
const recoveryCodes = ref<string[]>([]);

const isTokenMode = () => !!props.enrollToken;

async function chooseTotp() {
	error.value = null;
	busy.value = true;
	try {
		totp.value = isTokenMode()
			? await enrollTotpBegin(props.enrollToken!)
			: await selfEnrollTotpBegin();
		code.value = "";
		stage.value = "totp";
	} catch (e) {
		error.value = getErrorMessage(e, "Could not start setup");
	} finally {
		busy.value = false;
	}
}

async function chooseEmail() {
	error.value = null;
	busy.value = true;
	try {
		if (isTokenMode()) await enrollEmailBegin(props.enrollToken!);
		else await selfEnrollEmailBegin();
		code.value = "";
		stage.value = "email";
	} catch (e) {
		error.value = getErrorMessage(e, "Could not send code");
	} finally {
		busy.value = false;
	}
}

async function confirmTotp() {
	if (busy.value || !code.value) return;
	error.value = null;
	busy.value = true;
	try {
		const codes = isTokenMode()
			? await enrollTotpConfirm(props.enrollToken!, code.value.trim())
			: (await selfEnrollTotpConfirm(code.value.trim())).recoveryCodes;
		finishOrShowCodes(codes);
	} catch (e) {
		error.value = getErrorMessage(e, "That code didn't match");
	} finally {
		busy.value = false;
	}
}

async function confirmEmail() {
	if (busy.value || !code.value) return;
	error.value = null;
	busy.value = true;
	try {
		const codes = isTokenMode()
			? await enrollEmailConfirm(props.enrollToken!, code.value.trim())
			: (await selfEnrollEmailConfirm(code.value.trim())).recoveryCodes;
		finishOrShowCodes(codes);
	} catch (e) {
		error.value = getErrorMessage(e, "That code didn't match");
	} finally {
		busy.value = false;
	}
}

function finishOrShowCodes(codes: string[]) {
	if (codes.length > 0) {
		recoveryCodes.value = codes;
		stage.value = "recovery";
		return;
	}
	finish();
}

function finish() {
	if (isTokenMode()) redirectAfterLogin();
	else emit("done");
}

function copyCodes() {
	void navigator.clipboard?.writeText(recoveryCodes.value.join("\n"));
}
</script>

<template>
  <div class="tfa-setup">
    <div v-if="error" class="tfa-error">{{ error }}</div>

    <!-- Choose a method -->
    <template v-if="stage === 'choose'">
      <p class="tfa-hint">
        Two-factor authentication adds a second step when you sign in with a
        password. Choose how you'd like to receive your codes.
      </p>
      <div class="tfa-methods">
        <Button
          v-if="allowedMethods.includes('TOTP')"
          label="Use an authenticator app"
          icon="pi pi-mobile"
          class="tfa-full"
          :loading="busy"
          @click="chooseTotp"
        />
        <Button
          v-if="allowedMethods.includes('EMAIL_PIN')"
          label="Use email codes"
          icon="pi pi-envelope"
          severity="secondary"
          outlined
          class="tfa-full"
          :loading="busy"
          @click="chooseEmail"
        />
      </div>
    </template>

    <!-- TOTP -->
    <template v-else-if="stage === 'totp'">
      <p class="tfa-hint">
        Scan this QR code with your authenticator app (Google Authenticator,
        1Password, Authy…), or enter the key manually, then type the 6-digit
        code it shows.
      </p>
      <img v-if="totp?.qr" :src="totp.qr" alt="Authenticator QR code" class="tfa-qr" />
      <div v-if="totp" class="tfa-secret">
        <code>{{ totp.secret }}</code>
        <a :href="totp.uri" class="tfa-uri-link">Open in app</a>
      </div>
      <InputText
        v-model="code"
        class="tfa-input"
        placeholder="123456"
        inputmode="numeric"
        autocomplete="one-time-code"
        @keyup.enter="confirmTotp"
      />
      <Button
        label="Verify"
        class="tfa-full"
        :loading="busy"
        :disabled="!code"
        @click="confirmTotp"
      />
      <Button label="Back" text class="tfa-back" @click="stage = 'choose'" />
    </template>

    <!-- Email PIN -->
    <template v-else-if="stage === 'email'">
      <p class="tfa-hint">We've emailed you a code. Enter it below.</p>
      <InputText
        v-model="code"
        class="tfa-input"
        placeholder="123456"
        inputmode="numeric"
        autocomplete="one-time-code"
        @keyup.enter="confirmEmail"
      />
      <Button
        label="Verify"
        class="tfa-full"
        :loading="busy"
        :disabled="!code"
        @click="confirmEmail"
      />
      <Button label="Back" text class="tfa-back" @click="stage = 'choose'" />
    </template>

    <!-- Recovery codes -->
    <template v-else-if="stage === 'recovery'">
      <p class="tfa-hint">
        Save these recovery codes somewhere safe. Each can be used once if you
        lose access to your second factor.
      </p>
      <ul class="tfa-recovery">
        <li v-for="c in recoveryCodes" :key="c"><code>{{ c }}</code></li>
      </ul>
      <Button label="Copy codes" icon="pi pi-copy" outlined class="tfa-full" @click="copyCodes" />
      <Button label="I've saved them — continue" class="tfa-full" @click="finish" />
    </template>
  </div>
</template>

<style scoped>
.tfa-setup {
	display: flex;
	flex-direction: column;
	gap: 1rem;
	text-align: left;
}
.tfa-hint {
	margin: 0;
	font-size: 0.9rem;
	line-height: 1.45;
	color: var(--text-color-secondary, #64748b);
}
.tfa-error {
	padding: 0.6rem 0.85rem;
	border-radius: 6px;
	background: var(--red-50, #fef2f2);
	color: var(--red-700, #b91c1c);
	border: 1px solid var(--red-200, #fecaca);
	font-size: 0.9rem;
}
.tfa-methods {
	display: flex;
	flex-direction: column;
	gap: 0.6rem;
}
/* Full-width buttons + input (Button/InputText roots carry the class). */
.tfa-full,
.tfa-input {
	width: 100%;
}
.tfa-back {
	align-self: center;
}
.tfa-qr {
	align-self: center;
	width: 200px;
	height: 200px;
	image-rendering: pixelated;
	border: 1px solid var(--surface-200, #e5e7eb);
	border-radius: 8px;
	background: #fff;
	padding: 8px;
}
.tfa-secret {
	display: flex;
	align-items: center;
	justify-content: space-between;
	gap: 1rem;
	padding: 0.75rem 1rem;
	background: var(--surface-100, #f3f4f6);
	border: 1px solid var(--surface-200, #e5e7eb);
	border-radius: 6px;
	font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
	letter-spacing: 1px;
	word-break: break-all;
}
.tfa-uri-link {
	white-space: nowrap;
	font-family: inherit;
}
.tfa-recovery {
	list-style: none;
	padding: 1rem 1.25rem;
	margin: 0;
	background: var(--surface-100, #f3f4f6);
	border: 1px solid var(--surface-200, #e5e7eb);
	border-radius: 6px;
	columns: 2;
	font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
	font-size: 1rem;
}
.tfa-recovery li {
	padding: 0.2rem 0;
}
</style>
