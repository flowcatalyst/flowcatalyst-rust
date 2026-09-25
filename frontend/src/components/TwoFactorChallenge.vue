<script setup lang="ts">
import { computed, ref } from "vue";
import {
	sendEmailChallenge,
	verifyTwoFactor,
	type ChallengeMethod,
	type TwoFactorMethod,
} from "@/api/twofactor";
import { getErrorMessage } from "@/utils/errors";

// The second step of a password sign-in (`/auth/login` answered
// `mfa_required`). On a verified code the session is set and the page
// navigates on; there is nothing to emit.
const props = defineProps<{
	mfaToken: string;
	methods: TwoFactorMethod[];
	rememberDeviceAllowed: boolean;
}>();

const busy = ref(false);
const error = ref<string | null>(null);
const code = ref("");
const rememberDevice = ref(false);
const emailSent = ref(false);

const hasTotp = computed(() => props.methods.includes("TOTP"));
const hasEmail = computed(() => props.methods.includes("EMAIL_PIN"));

// Default to the authenticator if enrolled, else email.
const active = ref<ChallengeMethod>(
	props.methods.includes("TOTP") ? "TOTP" : "EMAIL_PIN",
);

function switchTo(method: ChallengeMethod) {
	active.value = method;
	code.value = "";
	error.value = null;
	emailSent.value = false;
}

async function requestEmail() {
	error.value = null;
	busy.value = true;
	try {
		await sendEmailChallenge(props.mfaToken);
		emailSent.value = true;
	} catch (e) {
		error.value = getErrorMessage(e, "Could not send code");
	} finally {
		busy.value = false;
	}
}

async function verify() {
	if (busy.value || !code.value.trim()) return;
	error.value = null;
	busy.value = true;
	try {
		await verifyTwoFactor({
			mfaToken: props.mfaToken,
			method: active.value,
			code: code.value.trim(),
			rememberDevice: props.rememberDeviceAllowed && rememberDevice.value,
		});
		// verifyTwoFactor has signed in and navigated.
	} catch (e) {
		error.value = getErrorMessage(e, "Invalid or expired code");
	} finally {
		busy.value = false;
	}
}
</script>

<template>
  <div class="tfa-challenge">
    <div v-if="error" class="tfa-error">{{ error }}</div>

    <p v-if="active === 'TOTP'" class="tfa-hint">
      Enter the 6-digit code from your authenticator app.
    </p>
    <template v-else-if="active === 'EMAIL_PIN'">
      <p class="tfa-hint">
        <template v-if="emailSent">Enter the code we emailed you.</template>
        <template v-else>We'll email a one-time code to your address.</template>
      </p>
      <Button
        v-if="!emailSent"
        label="Email me a code"
        icon="pi pi-envelope"
        class="tfa-full"
        :loading="busy"
        @click="requestEmail"
      />
    </template>
    <p v-else class="tfa-hint">Enter one of your recovery codes.</p>

    <form
      v-if="active !== 'EMAIL_PIN' || emailSent"
      class="tfa-form"
      @submit.prevent="verify"
    >
      <InputText
        v-model="code"
        class="tfa-full"
        :placeholder="active === 'RECOVERY_CODE' ? 'XXXXX-XXXXX' : '123456'"
        :inputmode="active === 'RECOVERY_CODE' ? 'text' : 'numeric'"
        autocomplete="one-time-code"
        autofocus
        :disabled="busy"
      />
      <div v-if="rememberDeviceAllowed" class="tfa-remember">
        <Checkbox v-model="rememberDevice" inputId="tfa-remember" binary />
        <label for="tfa-remember">Remember this device</label>
      </div>
      <Button
        type="submit"
        label="Verify"
        class="tfa-full"
        :loading="busy"
        :disabled="!code.trim()"
      />
    </form>

    <div class="tfa-alt">
      <a v-if="hasTotp && active !== 'TOTP'" href="#" @click.prevent="switchTo('TOTP')">
        Use authenticator app
      </a>
      <a v-if="hasEmail && active !== 'EMAIL_PIN'" href="#" @click.prevent="switchTo('EMAIL_PIN')">
        Use an email code
      </a>
      <a v-if="active === 'EMAIL_PIN' && emailSent" href="#" @click.prevent="requestEmail">
        Send a new code
      </a>
      <a v-if="active !== 'RECOVERY_CODE'" href="#" @click.prevent="switchTo('RECOVERY_CODE')">
        Use a recovery code
      </a>
    </div>
  </div>
</template>

<style scoped>
.tfa-challenge,
.tfa-form {
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

.tfa-full {
  width: 100%;
}

.tfa-remember {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  color: #475569;
}

.tfa-alt {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 14px;
}

.tfa-alt a {
  color: var(--login-accent, #0967d2);
  text-decoration: none;
}

.tfa-alt a:hover {
  text-decoration: underline;
}
</style>
