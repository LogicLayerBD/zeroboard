<script lang="ts">
	import * as api from '$lib/api';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import DatePicker from '$lib/components/ui/DatePicker.svelte';
	import Icon, { type IconName } from '$lib/components/ui/Icon.svelte';
	import Modal from '$lib/components/ui/Modal.svelte';
	import Spinner from '$lib/components/ui/Spinner.svelte';
	import { formatDateTime } from '$lib/format';
	import {
		addLabelLocally,
		applyCardDeleted,
		applyCardUpdated,
		board,
		labels,
		moveCardToList,
		removeLabelLocally,
		setCardRelations
	} from '$lib/stores/board.store';
	import { toastError } from '$lib/stores/toast.store';
	import { members } from '$lib/stores/workspace.store';
	import type { CardChanges, CardDetails, Label } from '$lib/types';
	import StatusSelect from '$lib/components/board/StatusSelect.svelte';
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

	async function changeStatus(select: HTMLSelectElement) {
		if (!details || select.value === details.list_id) return;
		const moved = await moveCardToList(details.id, select.value);
		if (moved && details) {
			details = { ...details, ...moved };
		} else if (details) {
			select.value = details.list_id;
		}
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

{#snippet sidebarTitle(icon: IconName, title: string)}
	<h3 class="section-title mb-2 flex items-center gap-1.5"><Icon name={icon} class="h-3.5 w-3.5" />{title}</h3>
{/snippet}

<Modal {onclose} label="Card details">
	{#if !details}
		<div class="p-10"><Spinner /></div>
	{:else}
		<div class="flex items-start gap-3 border-b border-slate-100 px-6 pb-5 pt-6">
			<div class="min-w-0 flex-1">
				<div class="mb-2 flex items-center gap-2 px-1.5 text-xs text-slate-500">
					<StatusSelect
						lists={$board?.lists ?? []}
						value={boardCard?.list_id ?? details.list_id}
						editable={false}
					/>
					<span>Created {formatDateTime(details.created_at)}</span>
				</div>
				{#if canEdit}
					<input
						class="input-inline w-full text-2xl font-bold tracking-tight text-slate-900"
						maxlength={MAX_TITLE_CHARS}
						aria-label="Card title"
						bind:value={titleDraft}
						onblur={saveTitle}
						onkeydown={(e) => {
							if (e.key === 'Enter') e.currentTarget.blur();
						}}
					/>
				{:else}
					<h2 class="px-1.5 text-2xl font-bold tracking-tight text-slate-900">{details.title}</h2>
				{/if}
			</div>
			<button type="button" class="icon-btn" aria-label="Close" onclick={onclose}
				><Icon name="x" class="h-5 w-5" /></button
			>
		</div>

		<div class="grid md:grid-cols-[1fr_17rem]">
			<div class="space-y-8 p-6">
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

			<aside class="space-y-6 border-t border-slate-100 bg-slate-50/70 p-6 md:border-l md:border-t-0">
				<section>
					{@render sidebarTitle('status', 'Status')}
					<StatusSelect
						lists={$board?.lists ?? []}
						value={boardCard?.list_id ?? details.list_id}
						editable={canEdit}
						block
						onchange={changeStatus}
					/>
				</section>

				<section>
					{@render sidebarTitle('users', 'Assignees')}
					<ul class="space-y-0.5">
						{#each $members as member (member.user_id)}
							{@const assigned = details.assignees.some((a) => a.user_id === member.user_id)}
							{#if canEdit || assigned}
								<li>
									<label
										class="flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm transition {canEdit
											? 'cursor-pointer hover:bg-white'
											: ''} {assigned ? 'font-medium text-slate-900' : 'text-slate-600'}"
									>
										{#if canEdit}
											<input
												type="checkbox"
												class="h-4 w-4 rounded border-slate-300 accent-indigo-600"
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
						<p class="px-2 text-sm text-slate-400">Nobody assigned.</p>
					{/if}
				</section>

				<section>
					{@render sidebarTitle('calendar', 'Due date')}
					<DatePicker
						value={details.due_date}
						disabled={!canEdit}
						onchange={(due_date) => update({ due_date }).catch(() => {})}
					/>
				</section>

				<section>
					{@render sidebarTitle('tag', 'Labels')}
					<ul class="space-y-0.5">
						{#each $labels as label (label.id)}
							{@const attached = details.labels.some((l) => l.id === label.id)}
							{#if canEdit || attached}
								<li class="group flex items-center gap-2 rounded-lg px-2 py-1 transition hover:bg-white">
									{#if canEdit}
										<input
											type="checkbox"
											class="h-4 w-4 cursor-pointer rounded border-slate-300 accent-indigo-600"
											checked={attached}
											aria-label="Toggle {label.name}"
											onchange={() => toggleLabel(label, attached)}
										/>
									{/if}
									<Badge color={label.color}>{label.name}</Badge>
									{#if canEdit}
										<button
											type="button"
											class="icon-btn ml-auto p-1 opacity-0 hover:bg-red-50 hover:text-red-600 focus-visible:opacity-100 group-hover:opacity-100"
											aria-label="Delete label {label.name}"
											onclick={() => deleteLabel(label)}><Icon name="x" class="h-3.5 w-3.5" /></button
										>
									{/if}
								</li>
							{/if}
						{/each}
					</ul>
					{#if canEdit}
						<form class="mt-2 flex items-center gap-1.5" onsubmit={createLabel}>
							<input
								type="color"
								class="h-8 w-8 shrink-0 cursor-pointer rounded-lg border-0 bg-white p-1 shadow-sm ring-1 ring-inset ring-slate-200"
								aria-label="Label color"
								bind:value={newLabelColor}
							/>
							<input
								class="input min-w-0 flex-1 px-2.5 py-1.5"
								placeholder="New label, then Enter"
								maxlength={MAX_LABEL_NAME_CHARS}
								bind:value={newLabelName}
							/>
						</form>
					{/if}
				</section>

				{#if canEdit}
					<button type="button" class="btn-danger btn-sm w-full" onclick={deleteCard}
						><Icon name="trash" class="h-3.5 w-3.5" />Delete card</button
					>
				{/if}
			</aside>
		</div>
	{/if}
</Modal>
