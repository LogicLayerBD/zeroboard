<script lang="ts">
	import { renderMarkdown } from '$lib/markdown';
	import Icon from '$lib/components/ui/Icon.svelte';

	const MAX_DESCRIPTION_CHARS = 50_000;

	interface Props {
		description: string | null;
		canEdit: boolean;
		onsave: (description: string | null) => Promise<void>;
	}

	let { description, canEdit, onsave }: Props = $props();

	let editing = $state(false);
	let draft = $state('');
	let saving = $state(false);

	// renderMarkdown escapes all input before formatting, so {@html} cannot inject markup.
	const html = $derived(description ? renderMarkdown(description) : '');

	function startEdit() {
		draft = description ?? '';
		editing = true;
	}

	async function save() {
		saving = true;
		try {
			await onsave(draft.trim() ? draft : null);
			editing = false;
		} catch {
			// The caller already reported the error; keep the draft so nothing is lost.
		} finally {
			saving = false;
		}
	}
</script>

<section>
	<div class="mb-3 flex items-center justify-between">
		<h3 class="flex items-center gap-2 text-sm font-semibold text-slate-900 dark:text-white">
			<Icon name="text" class="h-4 w-4 text-slate-400 dark:text-slate-500" />Description
		</h3>
		{#if canEdit && !editing}
			<button type="button" class="btn-ghost btn-sm" onclick={startEdit}
				><Icon name="pencil" class="h-3.5 w-3.5" />Edit</button
			>
		{/if}
	</div>
	{#if editing}
		<textarea
			class="input h-48 w-full font-mono"
			placeholder="Supports **bold**, *italic*, `code`, lists, # headings and [links](https://…)"
			maxlength={MAX_DESCRIPTION_CHARS}
			bind:value={draft}
		></textarea>
		<div class="mt-2 flex gap-2">
			<button
				type="button"
				class="btn-primary btn-sm"
				disabled={saving}
				onclick={save}>Save</button
			>
			<button
				type="button"
				class="btn-ghost btn-sm"
				onclick={() => (editing = false)}>Cancel</button
			>
		</div>
	{:else if html}
		<div class="break-words rounded-xl bg-slate-50/70 dark:bg-slate-800/30 p-4 text-sm leading-relaxed text-slate-700 dark:text-slate-200 ring-1 ring-inset ring-slate-100 dark:ring-slate-800">
			{@html html}
		</div>
	{:else}
		{#if canEdit}
			<button
				type="button"
				class="w-full rounded-xl border-2 border-dashed border-slate-200 dark:border-slate-800 p-4 text-left text-sm text-slate-400 dark:text-slate-500 transition hover:border-indigo-300 dark:hover:border-indigo-500 hover:text-indigo-600 dark:hover:text-indigo-300"
				onclick={startEdit}>Add a more detailed description…</button
			>
		{:else}
			<p class="text-sm text-slate-400 dark:text-slate-500">No description.</p>
		{/if}
	{/if}
</section>
