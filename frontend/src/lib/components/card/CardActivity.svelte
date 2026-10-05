<script lang="ts">
	import * as api from '$lib/api';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import { formatDateTime } from '$lib/format';
	import { currentUser } from '$lib/stores/auth.store';
	import { toastError } from '$lib/stores/toast.store';
	import { members, myRole } from '$lib/stores/workspace.store';
	import type { Comment } from '$lib/types';

	const MAX_BODY_CHARS = 10_000;
	const FORMER_MEMBER_COLOR = '#94a3b8';

	interface Props {
		cardId: string;
		comments: Comment[];
		canEdit: boolean;
		onchange: (comments: Comment[]) => void;
	}

	let { cardId, comments, canEdit, onchange }: Props = $props();

	let body = $state('');
	let saving = $state(false);
	let editingId = $state<string | null>(null);
	let editDraft = $state('');

	const memberById = $derived(new Map($members.map((m) => [m.user_id, m])));

	async function post(event: SubmitEvent) {
		event.preventDefault();
		const text = body.trim();
		if (!text || saving) return;
		saving = true;
		try {
			const comment = await api.createComment(cardId, text);
			onchange([...comments, comment]);
			body = '';
		} catch (err) {
			toastError(err);
		} finally {
			saving = false;
		}
	}

	function startEdit(comment: Comment) {
		editingId = comment.id;
		editDraft = comment.body;
	}

	async function saveEdit(comment: Comment) {
		const text = editDraft.trim();
		if (!text) return;
		try {
			const updated = await api.updateComment(comment.id, text);
			onchange(comments.map((c) => (c.id === comment.id ? updated : c)));
			editingId = null;
		} catch (err) {
			toastError(err);
		}
	}

	async function remove(comment: Comment) {
		if (!confirm('Delete this comment?')) return;
		try {
			await api.deleteComment(comment.id);
			onchange(comments.filter((c) => c.id !== comment.id));
		} catch (err) {
			toastError(err);
		}
	}
</script>

<section>
	<h3 class="mb-2 text-sm font-semibold text-slate-700">Comments</h3>
	<ul class="space-y-3">
		{#each comments as comment (comment.id)}
			{@const author = memberById.get(comment.user_id)}
			<li class="flex gap-3">
				<Avatar
					name={author?.name ?? '?'}
					color={author?.avatar_color ?? FORMER_MEMBER_COLOR}
					size="sm"
				/>
				<div class="min-w-0 flex-1">
					<div class="flex items-baseline gap-2 text-xs">
						<span class="font-semibold text-slate-700">{author?.name ?? 'Former member'}</span>
						<span class="text-slate-400">{formatDateTime(comment.created_at)}</span>
						{#if comment.updated_at !== comment.created_at}
							<span class="text-slate-400">(edited)</span>
						{/if}
					</div>
					{#if editingId === comment.id}
						<textarea
							class="mt-1 w-full rounded-md border border-slate-300 p-2 text-sm focus:border-indigo-500 focus:outline-none"
							rows="3"
							maxlength={MAX_BODY_CHARS}
							bind:value={editDraft}
						></textarea>
						<div class="mt-1 flex gap-2 text-xs">
							<button
								type="button"
								class="text-indigo-600 hover:underline"
								onclick={() => saveEdit(comment)}>Save</button
							>
							<button
								type="button"
								class="text-slate-500 hover:underline"
								onclick={() => (editingId = null)}>Cancel</button
							>
						</div>
					{:else}
						<p class="mt-0.5 whitespace-pre-wrap break-words text-sm text-slate-700">{comment.body}</p>
						<div class="mt-1 flex gap-3 text-xs">
							{#if canEdit && comment.user_id === $currentUser?.id}
								<button
									type="button"
									class="text-slate-500 hover:underline"
									onclick={() => startEdit(comment)}>Edit</button
								>
							{/if}
							{#if canEdit && (comment.user_id === $currentUser?.id || $myRole === 'admin')}
								<button
									type="button"
									class="text-slate-500 hover:text-red-600 hover:underline"
									onclick={() => remove(comment)}>Delete</button
								>
							{/if}
						</div>
					{/if}
				</div>
			</li>
		{:else}
			<li class="text-sm text-slate-400">No comments yet.</li>
		{/each}
	</ul>
	{#if canEdit}
		<form class="mt-3" onsubmit={post}>
			<textarea
				class="w-full rounded-md border border-slate-300 p-2 text-sm focus:border-indigo-500 focus:outline-none"
				rows="2"
				placeholder="Write a comment…"
				maxlength={MAX_BODY_CHARS}
				bind:value={body}
			></textarea>
			<button
				type="submit"
				class="mt-1 rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
				disabled={saving || !body.trim()}>Comment</button
			>
		</form>
	{/if}
</section>
