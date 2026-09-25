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
	// Set: the post-login / post-reset "enroll token" mode — confirming
	// completes the sign-in, then the page navigates on. Absent: the Profile
	// mode for a signed-in user, which emits `done`.
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
const copied = ref(false);

async function chooseTotp() {
	error.value = null;
	busy.value = true;
	try {
		totp.value = props.enrollToken
			? await enrollTotpBegin(props.enrollToken)
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
		if (props.enrollToken) await enrollEmailBegin(props.enrollToken);
		else await selfEnrollEmailBegin();
		code.value = "";
		stage.value = "email";
	} catch (e) {
		error.value = getErrorMessage(e, "Could not send code");
	} finally {
		busy.value = false;
	}
}

async function confirm(kind: "totp" | "email") {
	const value = code.value.trim();
	if (busy.value || !value) return;
	error.value = null;
	busy.value = true;
	try {
		let codes: string[];
		if (props.enrollToken) {
			codes =
				kind === "totp"
					? await enrollTotpConfirm(props.enrollToken, value)
					: await enrollEmailConfirm(props.enrollToken, value);
		} else {
			codes = (
				kind === "totp"
					? await selfEnrollTotpConfirm(value)
					: await selfEnrollEmailConfirm(value)
			).recoveryCodes;
		}
		if (codes.length > 0) {
			recoveryCodes.value = codes;
			stage.value = "recovery";
		} else {
			await finish();
		}
	} catch (e) {
		error.value = getErrorMessage(e, "That code didn't match");
	} finally {
		busy.value = false;
	}
}

async function finish() {
	if (props.enrollToken) await redirectAfterLogin();
	else emit("done");
}

async function copyCodes() {
	try {
		await navigator.clipboard?.writeText(recoveryCodes.value.join("\n"));
		copied.value = true;
	} catch {
		// Clipboard blocked: the codes stay on screen to copy by hand.
	}
}

function back() {
	error.value = null;
	stage.value = "choose";
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

    <!-- Authenticator app -->
    <form v-else-if="stage === 'totp'" class="tfa-setup" @submit.prevent="confirm('totp')">
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
        class="tfa-full"
        placeholder="123456"
        inputmode="numeric"
        autocomplete="one-time-code"
        :disabled="busy"
      />
      <Button type="submit" label="Verify" class="tfa-full" :loading="busy" :disabled="!code.trim()" />
      <Button type="button" label="Back" text class="tfa-back" @click="back" />
    </form>

    <!-- Email PIN -->
    <form v-else-if="stage === 'email'" class="tfa-setup" @submit.prevent="confirm('email')">
      <p class="tfa-hint">We've emailed you a code. Enter it below.</p>
      <InputText
        v-model="code"
        class="tfa-full"
        placeholder="123456"
        inputmode="numeric"
        autocomplete="one-time-code"
        :disabled="busy"
      />
      <Button type="submit" label="Verify" class="tfa-full" :loading="busy" :disabled="!code.trim()" />
      <Button type="button" label="Back" text class="tfa-back" @click="back" />
    </form>

    <!-- Recovery codes, shown once -->
    <template v-else-if="stage === 'recovery'">
      <p class="tfa-hint">
        Save these recovery codes somewhere safe. Each can be used once if you
        lose access to your second factor. They won't be shown again.
      </p>
      <ul class="tfa-recovery">
        <li v-for="c in recoveryCodes" :key="c"><code>{{ c }}</code></li>
      </ul>
      <Button
        :label="copied ? 'Copied' : 'Copy codes'"
        :icon="copied ? 'pi pi-check' : 'pi pi-copy'"
        severity="secondary"
        outlined
        class="tfa-full"
        @click="copyCodes"
      />
      <Button label="I've saved them — continue" class="tfa-full" @click="finish" />
    </template>
  </div>
</template>

<style scoped>
.tfa-setup {
  display: flex;
  flex-direction: column;
  gap: 16px;
  text-align: left;
}

.tfa-hint {
  margin: 0;
  font-size: 14px;
  line-height: 1.5;
  color: #627d98;
}

.tfa-error {
  padding: 12px 16px;
  border-radius: 8px;
  background: #fef2f2;
  color: #dc2626;
  border: 1px solid #fecaca;
  font-size: 14px;
}

.tfa-methods {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tfa-full {
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
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  background: #fff;
  padding: 8px;
}

.tfa-secret {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 16px;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  letter-spacing: 1px;
  word-break: break-all;
}

.tfa-uri-link {
  white-space: nowrap;
  font-family: inherit;
  letter-spacing: normal;
}

.tfa-recovery {
  list-style: none;
  padding: 16px 20px;
  margin: 0;
  background: #f8fafc;
  border: 1px solid #e2e8f0;
  border-radius: 8px;
  columns: 2;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 15px;
}

.tfa-recovery li {
  padding: 3px 0;
}
</style>
