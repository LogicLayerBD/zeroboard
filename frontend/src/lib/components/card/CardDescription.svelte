<script lang="ts">
	import { renderMarkdown } from '$lib/markdown';

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
	<div class="mb-2 flex items-center justify-between">
		<h3 class="text-sm font-semibold text-slate-700">Description</h3>
		{#if canEdit && !editing}
			<button type="button" class="text-xs text-indigo-600 hover:underline" onclick={startEdit}
				>Edit</button
			>
		{/if}
	</div>
	{#if editing}
		<textarea
			class="h-48 w-full rounded-md border border-slate-300 p-2 font-mono text-sm focus:border-indigo-500 focus:outline-none"
			placeholder="Supports **bold**, *italic*, `code`, lists, # headings and [links](https://…)"
			maxlength={MAX_DESCRIPTION_CHARS}
			bind:value={draft}
		></textarea>
		<div class="mt-2 flex gap-2">
			<button
				type="button"
				class="rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
				disabled={saving}
				onclick={save}>Save</button
			>
			<button
				type="button"
				class="rounded-md px-3 py-1.5 text-sm text-slate-600 hover:bg-slate-100"
				onclick={() => (editing = false)}>Cancel</button
			>
		</div>
	{:else if html}
		<div class="break-words text-sm text-slate-700">{@html html}</div>
	{:else}
		<p class="text-sm text-slate-400">No description.</p>
	{/if}
</section>
