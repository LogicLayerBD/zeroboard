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
		class="rounded-md border border-slate-300 px-2 py-1 text-sm focus:border-indigo-500 focus:outline-none disabled:bg-slate-50"
		value={toDateTimeLocal(value)}
		{disabled}
		onchange={(e) => onchange(fromDateTimeLocal(e.currentTarget.value))}
	/>
	{#if value !== null && !disabled}
		<button
			type="button"
			class="text-xs text-slate-500 hover:text-red-600"
			onclick={() => onchange(null)}>Clear</button
		>
	{/if}
</div>
