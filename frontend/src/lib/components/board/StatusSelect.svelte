<script lang="ts">
	import { columnAccent } from '$lib/colors';
	import Icon from '$lib/components/ui/Icon.svelte';

	interface Props {
		/** Board columns in display order; position decides the colour. */
		lists: { id: string; name: string }[];
		value: string;
		editable: boolean;
		/** Stretches the pill to its container (card modal sidebar). */
		block?: boolean;
		onchange?: (select: HTMLSelectElement) => void;
	}

	let { lists, value, editable, block = false, onchange }: Props = $props();

	const index = $derived(lists.findIndex((l) => l.id === value));
	const accent = $derived(columnAccent(Math.max(index, 0)));
	const name = $derived(lists[index]?.name ?? '');
</script>

{#if editable}
	<div class="relative {block ? 'flex w-full' : 'inline-flex'}">
		<span
			class="pointer-events-none absolute left-3 top-1/2 h-2 w-2 -translate-y-1/2 rounded-full"
			style:background-color={accent}
		></span>
		<select
			class="w-full cursor-pointer appearance-none rounded-full border-0 bg-white dark:bg-slate-900 py-1 pl-7 pr-8 text-xs font-semibold text-slate-700 dark:text-slate-200 shadow-sm ring-1 ring-inset ring-slate-200 dark:ring-slate-700 transition hover:ring-slate-300 dark:hover:ring-slate-600 focus:outline-none focus:ring-2 focus:ring-indigo-500 {block
				? 'py-2 text-sm'
				: ''}"
			aria-label="Status"
			{value}
			onchange={(e) => onchange?.(e.currentTarget)}
		>
			{#each lists as list (list.id)}
				<option value={list.id}>{list.name}</option>
			{/each}
		</select>
		<Icon
			name="chevronDown"
			class="pointer-events-none absolute right-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-400 dark:text-slate-500"
		/>
	</div>
{:else}
	<span
		class="inline-flex items-center gap-1.5 rounded-full bg-white dark:bg-slate-900 px-2.5 py-1 text-xs font-semibold text-slate-700 dark:text-slate-200 ring-1 ring-inset ring-slate-200 dark:ring-slate-700"
	>
		<span class="h-2 w-2 rounded-full" style:background-color={accent}></span>
		{name}
	</span>
{/if}
