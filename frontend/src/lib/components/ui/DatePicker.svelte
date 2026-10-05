<script lang="ts">
	import { fromDateTimeLocal, toDateTimeLocal } from '$lib/format';

	interface Props {
		value: number | null;
		disabled?: boolean;
		onchange: (value: number | null) => void;
	}

	let { value, disabled = false, onchange }: Props = $props();
</script>

<div class="flex items-center gap-2">
	<input
		type="datetime-local"
		class="input min-w-0 flex-1 px-2.5 py-1.5"
		value={toDateTimeLocal(value)}
		{disabled}
		onchange={(e) => onchange(fromDateTimeLocal(e.currentTarget.value))}
	/>
	{#if value !== null && !disabled}
		<button
			type="button"
			class="text-xs font-medium text-slate-400 transition hover:text-red-600"
			onclick={() => onchange(null)}>Clear</button
		>
	{/if}
</div>
