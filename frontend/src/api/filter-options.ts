// Stays hand-rolled: BFF endpoint — deliberately stripped from the OpenAPI
// spec (StripBFFPaths), so no generated types exist for it.
import { bffFetch } from "./client";

export interface FilterOption {
	value: string;
	label: string;
}

interface ClientFilterOptions {
	clients: FilterOption[];
}

export const filterOptionsApi = {
	clients(): Promise<ClientFilterOptions> {
		return bffFetch("/filter-options/clients");
	},
};
