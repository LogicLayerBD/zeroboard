<script lang="ts">
	import * as api from '$lib/api';
	import { formatBytes, formatDateTime } from '$lib/format';
	import Icon from '$lib/components/ui/Icon.svelte';
	import { toastError } from '$lib/stores/toast.store';
	import type { Attachment } from '$lib/types';

	interface Props {
		cardId: string;
		attachments: Attachment[];
		canEdit: boolean;
		onchange: (attachments: Attachment[]) => void;
	}

	let { cardId, attachments, canEdit, onchange }: Props = $props();

	let uploading = $state(false);

	async function upload(event: Event & { currentTarget: HTMLInputElement }) {
		const input = event.currentTarget;
		const file = input.files?.[0];
		if (!file) return;
		uploading = true;
		try {
			const attachment = await api.uploadAttachment(cardId, file);
			onchange([...attachments, attachment]);
		} catch (err) {
			toastError(err);
		} finally {
			uploading = false;
			input.value = '';
		}
	}

	async function download(attachment: Attachment) {
		try {
			await api.downloadAttachment(attachment);
		} catch (err) {
			toastError(err);
		}
	}

	async function remove(attachment: Attachment) {
		if (!confirm(`Delete ${attachment.filename}?`)) return;
		try {
			await api.deleteAttachment(attachment.id);
			onchange(attachments.filter((a) => a.id !== attachment.id));
		} catch (err) {
			toastError(err);
		}
	}
</script>

<section>
	<div class="mb-3 flex items-center justify-between">
		<h3 class="flex items-center gap-2 text-sm font-semibold text-slate-900">
			<Icon name="paperclip" class="h-4 w-4 text-slate-400" />Attachments
		</h3>
		{#if canEdit}
			<label class="btn-ghost btn-sm cursor-pointer">
				<Icon name="upload" class="h-3.5 w-3.5" />{uploading ? 'Uploading…' : 'Upload'}
				<input type="file" class="hidden" disabled={uploading} onchange={upload} />
			</label>
		{/if}
	</div>
	<ul class="space-y-1.5">
		{#each attachments as attachment (attachment.id)}
			<li
				class="group flex items-center gap-3 rounded-xl bg-white px-3 py-2.5 text-sm ring-1 ring-inset ring-slate-200 transition hover:ring-indigo-200"
			>
				<span class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-indigo-50 text-indigo-500"
					><Icon name="file" /></span
				>
				<button
					type="button"
					class="flex-1 truncate text-left font-medium text-slate-800 hover:text-indigo-600"
					onclick={() => download(attachment)}>{attachment.filename}</button
				>
				<span class="text-xs text-slate-400">{formatBytes(attachment.size_bytes)}</span>
				<span class="hidden text-xs text-slate-400 sm:inline"
					>{formatDateTime(attachment.uploaded_at)}</span
				>
				{#if canEdit}
					<button
						type="button"
						class="icon-btn p-1 opacity-0 hover:bg-red-50 hover:text-red-600 focus-visible:opacity-100 group-hover:opacity-100"
						aria-label="Delete {attachment.filename}"
						onclick={() => remove(attachment)}><Icon name="x" class="h-3.5 w-3.5" /></button
					>
				{/if}
			</li>
		{:else}
			<li class="text-sm text-slate-400">No attachments.</li>
		{/each}
	</ul>
</section>
