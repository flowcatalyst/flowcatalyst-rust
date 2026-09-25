<script setup lang="ts">
import { ref } from "vue";
import {
	sendEmailChallenge,
	verifyTwoFactor,
	type ChallengeMethod,
	type TwoFactorMethod,
} from "@/api/twofactor";
import { getErrorMessage } from "@/utils/errors";

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

// Default to the authenticator if available, else email.
const active = ref<ChallengeMethod>(
	props.methods.includes("TOTP") ? "TOTP" : "EMAIL_PIN",
);

const hasTotp = () => props.methods.includes("TOTP");
const hasEmail = () => props.methods.includes("EMAIL_PIN");

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
	if (busy.value || !code.value) return;
	error.value = null;
	busy.value = true;
	try {
		await verifyTwoFactor({
			mfaToken: props.mfaToken,
			method: active.value,
			code: code.value.trim(),
			rememberDevice: rememberDevice.value,
		});
		// verifyTwoFactor redirects on success.
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

    <template v-if="active === 'TOTP'">
      <p class="tfa-hint">Enter the 6-digit code from your authenticator app.</p>
    </template>
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
    <template v-else>
      <p class="tfa-hint">Enter one of your recovery codes.</p>
    </template>

    <template v-if="active !== 'EMAIL_PIN' || emailSent">
      <InputText
        v-model="code"
        class="tfa-input"
        :placeholder="active === 'RECOVERY_CODE' ? 'XXXXX-XXXXX' : '123456'"
        inputmode="text"
        autocomplete="one-time-code"
        @keyup.enter="verify"
      />
      <label v-if="rememberDeviceAllowed" class="tfa-remember">
        <input type="checkbox" v-model="rememberDevice" />
        Remember this device for 30 days
      </label>
      <Button label="Verify" class="tfa-full" :loading="busy" :disabled="!code" @click="verify" />
    </template>

    <div class="tfa-alt">
      <a v-if="hasTotp() && active !== 'TOTP'" href="#" @click.prevent="switchTo('TOTP')">
        Use authenticator app
      </a>
      <a v-if="hasEmail() && active !== 'EMAIL_PIN'" href="#" @click.prevent="switchTo('EMAIL_PIN')">
        Use an email code
      </a>
      <a v-if="active !== 'RECOVERY_CODE'" href="#" @click.prevent="switchTo('RECOVERY_CODE')">
        Use a recovery code
      </a>
    </div>
  </div>
</template>

<style scoped>
.tfa-challenge {
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
.tfa-full,
.tfa-input {
	width: 100%;
}
.tfa-remember {
	display: flex;
	align-items: center;
	gap: 0.5rem;
	font-size: 0.9rem;
	color: var(--text-color-secondary, #64748b);
}
.tfa-alt {
	display: flex;
	flex-direction: column;
	gap: 0.4rem;
	margin-top: 0.25rem;
	font-size: 0.9rem;
}
</style>
