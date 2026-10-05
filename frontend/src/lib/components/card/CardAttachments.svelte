<script lang="ts">
	import * as api from '$lib/api';
	import { formatBytes, formatDateTime } from '$lib/format';
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
	<div class="mb-2 flex items-center justify-between">
		<h3 class="text-sm font-semibold text-slate-700">Attachments</h3>
		{#if canEdit}
			<label class="cursor-pointer text-xs text-indigo-600 hover:underline">
				{uploading ? 'Uploading…' : 'Upload'}
				<input type="file" class="hidden" disabled={uploading} onchange={upload} />
			</label>
		{/if}
	</div>
	<ul class="space-y-1">
		{#each attachments as attachment (attachment.id)}
			<li class="flex items-center gap-2 rounded-md bg-slate-50 px-3 py-2 text-sm">
				<button
					type="button"
					class="flex-1 truncate text-left text-indigo-700 hover:underline"
					onclick={() => download(attachment)}>{attachment.filename}</button
				>
				<span class="text-xs text-slate-400">{formatBytes(attachment.size_bytes)}</span>
				<span class="hidden text-xs text-slate-400 sm:inline"
					>{formatDateTime(attachment.uploaded_at)}</span
				>
				{#if canEdit}
					<button
						type="button"
						class="text-xs text-slate-400 hover:text-red-600"
						aria-label="Delete {attachment.filename}"
						onclick={() => remove(attachment)}>✕</button
					>
				{/if}
			</li>
		{:else}
			<li class="text-sm text-slate-400">No attachments.</li>
		{/each}
	</ul>
</section>
