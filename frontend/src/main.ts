import { createApp } from "vue";
import { createPinia } from "pinia";
import PrimeVue from "primevue/config";
import { definePreset } from "@primeuix/themes";
import Nora from "@primeuix/themes/nora";
import ConfirmationService from "primevue/confirmationservice";
import Tooltip from "primevue/tooltip";

import App from "./App.vue";
import router from "./router";

import "primeicons/primeicons.css";
import "./styles/main.css";

const app = createApp(App);

app.use(createPinia());
app.use(router);

// Tighter density preset on top of Nora. Nora's defaults are designed for
// content-light marketing layouts; this admin UI is data-dense, so we shrink
// the semantic form-field padding (which Button/InputText/Select/MultiSelect/
// Textarea all read) and the DataTable header/body cell padding. Combined
// with the 12.6px base font in main.css this scales the whole UI tighter
// without per-component `size="small"` annotations.
const FlowCatalystPreset = definePreset(Nora, {
	semantic: {
		formField: {
			paddingX: "0.625rem",
			paddingY: "0.4rem",
		},
	},
	components: {
		datatable: {
			headerCell: { padding: "0.5rem 0.75rem" },
			bodyCell: { padding: "0.4rem 0.75rem" },
			footerCell: { padding: "0.5rem 0.75rem" },
		},
	},
});

app.use(PrimeVue, {
	theme: {
		preset: FlowCatalystPreset,
		options: {
			darkModeSelector: ".dark-mode",
		},
	},
});
app.use(ConfirmationService);
app.directive("tooltip", Tooltip);

app.mount("#app");
