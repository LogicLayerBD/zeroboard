<script lang="ts">
	import * as api from '$lib/api';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
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
	<h3 class="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-900 dark:text-white">
		<Icon name="message" class="h-4 w-4 text-slate-400 dark:text-slate-500" />Comments
	</h3>
	<ul class="space-y-4">
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
						<span class="font-semibold text-slate-700 dark:text-slate-200">{author?.name ?? 'Former member'}</span>
						<span class="text-slate-400 dark:text-slate-500">{formatDateTime(comment.created_at)}</span>
						{#if comment.updated_at !== comment.created_at}
							<span class="text-slate-400 dark:text-slate-500">(edited)</span>
						{/if}
					</div>
					{#if editingId === comment.id}
						<textarea
							class="input mt-1 w-full"
							rows="3"
							maxlength={MAX_BODY_CHARS}
							bind:value={editDraft}
						></textarea>
						<div class="mt-2 flex gap-2">
							<button
								type="button"
								class="btn-primary btn-sm"
								onclick={() => saveEdit(comment)}>Save</button
							>
							<button
								type="button"
								class="btn-ghost btn-sm"
								onclick={() => (editingId = null)}>Cancel</button
							>
						</div>
					{:else}
						<p
							class="mt-1 whitespace-pre-wrap break-words rounded-xl rounded-tl-sm bg-slate-50 dark:bg-slate-800/50 px-3.5 py-2.5 text-sm text-slate-700 dark:text-slate-200 ring-1 ring-inset ring-slate-100 dark:ring-slate-800"
						>
							{comment.body}
						</p>
						<div class="mt-1 flex gap-3 px-1 text-xs font-medium">
							{#if canEdit && comment.user_id === $currentUser?.id}
								<button
									type="button"
									class="text-slate-400 dark:text-slate-500 transition hover:text-slate-700 dark:hover:text-slate-100"
									onclick={() => startEdit(comment)}>Edit</button
								>
							{/if}
							{#if canEdit && (comment.user_id === $currentUser?.id || $myRole === 'admin')}
								<button
									type="button"
									class="text-slate-400 dark:text-slate-500 transition hover:text-red-600 dark:hover:text-red-400"
									onclick={() => remove(comment)}>Delete</button
								>
							{/if}
						</div>
					{/if}
				</div>
			</li>
		{:else}
			<li class="text-sm text-slate-400 dark:text-slate-500">No comments yet.</li>
		{/each}
	</ul>
	{#if canEdit}
		<form class="mt-4 flex gap-3" onsubmit={post}>
			{#if $currentUser}
				<Avatar name={$currentUser.name} color={$currentUser.avatar_color} size="sm" />
			{/if}
			<div class="flex-1">
				<textarea
					class="input w-full"
					rows="2"
					placeholder="Write a comment…"
					maxlength={MAX_BODY_CHARS}
					bind:value={body}
				></textarea>
				<button
					type="submit"
					class="btn-primary btn-sm mt-2"
					disabled={saving || !body.trim()}>Comment</button
				>
			</div>
		</form>
	{/if}
</section>
