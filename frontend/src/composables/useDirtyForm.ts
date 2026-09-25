import { computed, ref } from "vue";

/**
 * Tracks whether a form's current values differ from the snapshot taken at
 * the last `markClean()` call. Comparison is structural (JSON.stringify) —
 * fine for the primitive/nullable-number/boolean/string-array field shapes
 * edit forms use across this app; order-sensitive for arrays.
 *
 * `dirty` is false until the first `markClean()` — a form that hasn't
 * started editing yet has nothing to be dirty against.
 */
export function useDirtyForm<T>(snapshot: () => T) {
	const baseline = ref<string | null>(null);

	const dirty = computed(
		() => baseline.value !== null && JSON.stringify(snapshot()) !== baseline.value,
	);

	function markClean() {
		baseline.value = JSON.stringify(snapshot());
	}

	function reset() {
		baseline.value = null;
	}

	return { dirty, markClean, reset };
}
