<script setup lang="ts">
// The live manifest's public routes (the routes promote materialised,
// `GET /api/function-routes?address=`), each joined with whether its hostname
// sits in a zone the function's owner has claimed. A claim is verified by
// being made, so a route is claimed or not claimed; there is no pending state.
import { computed, ref, watch } from "vue";
import {
	functionsApi,
	type DomainResponse,
	type FunctionRouteResponse,
} from "@/api/functions";
import { useAuthStore } from "@/stores/auth";
import { userHasPermission } from "@/stores/permissions";

const props = defineProps<{
	address: string;
	clientId?: string;
	hasLiveVersion: boolean;
}>();

const authStore = useAuthStore();
const canManageDomains = computed(() =>
	userHasPermission(authStore.user, "platform:function:domain:manage"),
);

const routes = ref<FunctionRouteResponse[]>([]);
const loading = ref(true);
const domains = ref<DomainResponse[]>([]);

watch(
	() => [props.address, props.clientId, props.hasLiveVersion] as const,
	async ([addr]) => {
		if (addr) await load(addr);
	},
	{ immediate: true },
);

async function load(addr: string) {
	loading.value = true;
	try {
		const [routeResult, domainResult] = await Promise.all([
			functionsApi.listRoutes({ address: addr }),
			functionsApi.listDomains(props.clientId ?? "platform").catch(() => []),
		]);
		routes.value = routeResult;
		domains.value = domainResult;
	} catch {
		routes.value = [];
	} finally {
		loading.value = false;
	}
}

/** A claim is a zone: it covers its own hostname and every hostname under it. */
function coveringClaim(hostname: string): DomainResponse | undefined {
	return (
		domains.value.find((d) => d.hostname === hostname) ??
		domains.value.find((d) => hostname.endsWith(`.${d.hostname}`))
	);
}

/**
 * An alias prefix derives a hostname by prefixing the route's first label
 * with `<prefix>-` (`qa` on `app.acme.com` is `qa-app.acme.com`). Display only.
 */
function derivedHostname(hostname: string, prefix: string): string {
	return `${prefix}-${hostname}`;
}
</script>

<template>
  <div class="public-routes-tab">
    <p v-if="!hasLiveVersion" class="text-muted text-sm">
      This function has no live version yet; public routes are created on promote.
    </p>
    <ProgressSpinner v-else-if="loading" style="width: 24px; height: 24px" />
    <p v-else-if="routes.length === 0" class="text-muted text-sm">
      The live manifest declares no public routes.
    </p>
    <DataTable v-else :value="routes" size="small">
      <Column header="Hostname">
        <template #body="{ data }">
          <span class="font-mono text-sm">{{ data.hostname }}</span>
        </template>
      </Column>
      <Column header="Path Prefix">
        <template #body="{ data }">
          <span class="font-mono text-sm">{{ data.pathPrefix }}</span>
        </template>
      </Column>
      <Column header="Alias Prefixes">
        <template #body="{ data }">
          <span v-if="data.aliasPrefixes.length === 0" class="text-muted">none</span>
          <ul v-else class="alias-prefixes">
            <li v-for="prefix in data.aliasPrefixes" :key="prefix">
              <Tag :value="prefix" severity="info" />
              <span class="font-mono text-xs text-muted">{{ derivedHostname(data.hostname, prefix) }}</span>
            </li>
          </ul>
        </template>
      </Column>
      <Column header="Claim">
        <template #body="{ data }">
          <Tag
            :value="coveringClaim(data.hostname) ? 'claimed' : 'not claimed'"
            :severity="coveringClaim(data.hostname) ? 'success' : 'danger'"
          />
        </template>
      </Column>
      <Column header="">
        <template #body="{ data }">
          <RouterLink
            v-if="canManageDomains && coveringClaim(data.hostname)"
            :to="`/function-domains/${encodeURIComponent(coveringClaim(data.hostname)!.hostname)}`"
          >
            View domain
          </RouterLink>
        </template>
      </Column>
    </DataTable>
  </div>
</template>

<style scoped>
.alias-prefixes {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.alias-prefixes li {
  display: flex;
  align-items: center;
  gap: 8px;
}

.font-mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.text-sm {
  font-size: 0.875rem;
}

.text-xs {
  font-size: 0.75rem;
}

.text-muted {
  color: var(--text-color-secondary);
}
</style>
