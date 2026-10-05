<script lang="ts">
	import * as api from '$lib/api';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import DatePicker from '$lib/components/ui/DatePicker.svelte';
	import Modal from '$lib/components/ui/Modal.svelte';
	import Spinner from '$lib/components/ui/Spinner.svelte';
	import { formatDateTime } from '$lib/format';
	import {
		addLabelLocally,
		applyCardDeleted,
		applyCardUpdated,
		board,
		labels,
		removeLabelLocally,
		setCardRelations
	} from '$lib/stores/board.store';
	import { toastError } from '$lib/stores/toast.store';
	import { members } from '$lib/stores/workspace.store';
	import type { CardChanges, CardDetails, Label } from '$lib/types';
	import CardActivity from './CardActivity.svelte';
	import CardAttachments from './CardAttachments.svelte';
	import CardDescription from './CardDescription.svelte';
	import CardTimeTracking from './CardTimeTracking.svelte';

	const MAX_TITLE_CHARS = 500;
	const MAX_LABEL_NAME_CHARS = 100;
	const DEFAULT_LABEL_COLOR = '#6366f1';

	interface Props {
		cardId: string;
		canEdit: boolean;
		onclose: () => void;
	}

	let { cardId, canEdit, onclose }: Props = $props();

	let details = $state<CardDetails | null>(null);
	let titleDraft = $state('');
	let newLabelName = $state('');
	let newLabelColor = $state(DEFAULT_LABEL_COLOR);

	const boardCard = $derived(
		$board?.lists.flatMap((l) => l.cards).find((c) => c.id === cardId) ?? null
	);
	const listName = $derived($board?.lists.find((l) => l.id === boardCard?.list_id)?.name ?? '');

	$effect(() => {
		const id = cardId;
		details = null;
		api
			.getCard(id)
			.then((card) => {
				if (id !== cardId) return;
				details = card;
				titleDraft = card.title;
			})
			.catch((err: unknown) => {
				toastError(err);
				onclose();
			});
	});

	// Keep the open card in sync with live edits from other users.
	$effect(() => {
		if (!details || !$board) return;
		if (!boardCard) {
			toastError(new api.ApiError(404, ''));
			onclose();
			return;
		}
		if (boardCard.updated_at > details.updated_at) {
			const { title, description, due_date, list_id, position, updated_at } = boardCard;
			details = { ...details, title, description, due_date, list_id, position, updated_at };
			titleDraft = title;
		}
	});

	async function update(changes: CardChanges) {
		if (!details) return;
		try {
			const card = await api.updateCard(details.id, changes);
			details = { ...details, ...card };
			titleDraft = card.title;
			applyCardUpdated(card);
		} catch (err) {
			toastError(err);
			throw err;
		}
	}

	function saveTitle() {
		const title = titleDraft.trim();
		if (!details || !title || title === details.title) {
			titleDraft = details?.title ?? '';
			return;
		}
		void update({ title }).catch(() => (titleDraft = details?.title ?? ''));
	}

	async function toggleAssignee(userId: string, assigned: boolean) {
		if (!details) return;
		try {
			if (assigned) {
				await api.removeAssignee(details.id, userId);
				details.assignees = details.assignees.filter((a) => a.user_id !== userId);
			} else {
				details.assignees = [...details.assignees, await api.addAssignee(details.id, userId)];
			}
			setCardRelations(details.id, { assignee_ids: details.assignees.map((a) => a.user_id) });
		} catch (err) {
			toastError(err);
		}
	}

	async function toggleLabel(label: Label, attached: boolean) {
		if (!details) return;
		try {
			if (attached) {
				await api.removeCardLabel(details.id, label.id);
				details.labels = details.labels.filter((l) => l.id !== label.id);
			} else {
				await api.addCardLabel(details.id, label.id);
				details.labels = [...details.labels, label];
			}
			setCardRelations(details.id, { label_ids: details.labels.map((l) => l.id) });
		} catch (err) {
			toastError(err);
		}
	}

	async function createLabel(event: SubmitEvent) {
		event.preventDefault();
		const name = newLabelName.trim();
		if (!details || !name) return;
		try {
			const label = await api.createLabel(details.board_id, name, newLabelColor);
			addLabelLocally(label);
			newLabelName = '';
			await toggleLabel(label, false);
		} catch (err) {
			toastError(err);
		}
	}

	async function deleteLabel(label: Label) {
		if (!confirm(`Delete the "${label.name}" label from every card on this board?`)) return;
		try {
			await api.deleteLabel(label.id);
			removeLabelLocally(label.id);
			if (details) details.labels = details.labels.filter((l) => l.id !== label.id);
		} catch (err) {
			toastError(err);
		}
	}

	async function deleteCard() {
		if (!details || !confirm(`Delete "${details.title}"? This cannot be undone.`)) return;
		try {
			await api.deleteCard(details.id);
			applyCardDeleted(details.id);
			onclose();
		} catch (err) {
			toastError(err);
		}
	}
</script>

<Modal {onclose} label="Card details">
	{#if !details}
		<Spinner />
	{:else}
		<div class="flex items-start gap-3 border-b border-slate-100 p-5">
			<div class="flex-1">
				{#if canEdit}
					<input
						class="w-full rounded border border-transparent px-1 text-xl font-semibold hover:border-slate-300 focus:border-indigo-500 focus:outline-none"
						maxlength={MAX_TITLE_CHARS}
						aria-label="Card title"
						bind:value={titleDraft}
						onblur={saveTitle}
						onkeydown={(e) => {
							if (e.key === 'Enter') e.currentTarget.blur();
						}}
					/>
				{:else}
					<h2 class="px-1 text-xl font-semibold">{details.title}</h2>
				{/if}
				<p class="mt-1 px-1 text-xs text-slate-500">
					in <span class="font-medium">{listName}</span> · created {formatDateTime(details.created_at)}
				</p>
			</div>
			<button
				type="button"
				class="rounded p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700"
				aria-label="Close"
				onclick={onclose}>✕</button
			>
		</div>

		<div class="grid gap-6 p-5 md:grid-cols-3">
			<div class="space-y-6 md:col-span-2">
				<CardDescription
					description={details.description}
					{canEdit}
					onsave={(description) => update({ description })}
				/>
				<CardAttachments
					cardId={details.id}
					attachments={details.attachments}
					{canEdit}
					onchange={(attachments) => details && (details.attachments = attachments)}
				/>
				<CardTimeTracking
					cardId={details.id}
					entries={details.time_entries}
					{canEdit}
					onchange={(entries) => details && (details.time_entries = entries)}
				/>
				<CardActivity
					cardId={details.id}
					comments={details.comments}
					{canEdit}
					onchange={(comments) => details && (details.comments = comments)}
				/>
			</div>

			<aside class="space-y-6">
				<section>
					<h3 class="mb-2 text-sm font-semibold text-slate-700">Assignees</h3>
					<ul class="space-y-1">
						{#each $members as member (member.user_id)}
							{@const assigned = details.assignees.some((a) => a.user_id === member.user_id)}
							{#if canEdit || assigned}
								<li>
									<label class="flex items-center gap-2 text-sm">
										{#if canEdit}
											<input
												type="checkbox"
												checked={assigned}
												onchange={() => toggleAssignee(member.user_id, assigned)}
											/>
										{/if}
										<Avatar name={member.name} color={member.avatar_color} size="sm" />
										<span class="truncate">{member.name}</span>
									</label>
								</li>
							{/if}
						{/each}
					</ul>
					{#if !canEdit && details.assignees.length === 0}
						<p class="text-sm text-slate-400">Nobody assigned.</p>
					{/if}
				</section>

				<section>
					<h3 class="mb-2 text-sm font-semibold text-slate-700">Due date</h3>
					<DatePicker
						value={details.due_date}
						disabled={!canEdit}
						onchange={(due_date) => update({ due_date }).catch(() => {})}
					/>
				</section>

				<section>
					<h3 class="mb-2 text-sm font-semibold text-slate-700">Labels</h3>
					<ul class="space-y-1">
						{#each $labels as label (label.id)}
							{@const attached = details.labels.some((l) => l.id === label.id)}
							{#if canEdit || attached}
								<li class="flex items-center gap-2">
									{#if canEdit}
										<input
											type="checkbox"
											checked={attached}
											aria-label="Toggle {label.name}"
											onchange={() => toggleLabel(label, attached)}
										/>
									{/if}
									<Badge color={label.color}>{label.name}</Badge>
									{#if canEdit}
										<button
											type="button"
											class="ml-auto text-xs text-slate-300 hover:text-red-600"
											aria-label="Delete label {label.name}"
											onclick={() => deleteLabel(label)}>✕</button
										>
									{/if}
								</li>
							{/if}
						{/each}
					</ul>
					{#if canEdit}
						<form class="mt-2 flex items-center gap-1" onsubmit={createLabel}>
							<input
								type="color"
								class="h-7 w-8 cursor-pointer rounded border border-slate-300"
								aria-label="Label color"
								bind:value={newLabelColor}
							/>
							<input
								class="min-w-0 flex-1 rounded-md border border-slate-300 px-2 py-1 text-sm focus:border-indigo-500 focus:outline-none"
								placeholder="New label"
								maxlength={MAX_LABEL_NAME_CHARS}
								bind:value={newLabelName}
							/>
						</form>
					{/if}
				</section>

				{#if canEdit}
					<button
						type="button"
						class="w-full rounded-md border border-red-200 px-3 py-1.5 text-sm text-red-600 hover:bg-red-50"
						onclick={deleteCard}>Delete card</button
					>
				{/if}
			</aside>
		</div>
	{/if}
</Modal>
