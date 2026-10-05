<script lang="ts">
	import * as api from '$lib/api';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import DatePicker from '$lib/components/ui/DatePicker.svelte';
	import { formatDate } from '$lib/format';
	import {
		applyCardUpdated,
		labels,
		moveCardToList,
		setCardRelations
	} from '$lib/stores/board.store';
	import { toastError } from '$lib/stores/toast.store';
	import { members } from '$lib/stores/workspace.store';
	import type { BoardCard, CardChanges, ListWithCards } from '$lib/types';

	const MAX_TITLE_CHARS = 500;

	type SortKey = 'board' | 'title' | 'due_date' | 'assignee' | 'label' | 'created_at';

	interface Props {
		lists: ListWithCards[];
		canEdit: boolean;
		onopencard: (cardId: string) => void;
	}

	interface Row {
		card: BoardCard;
		listName: string;
		/** Index in board order (list order, then card position). */
		order: number;
	}

	let { lists, canEdit, onopencard }: Props = $props();

	let sortKey = $state<SortKey>('board');
	let ascending = $state(true);

	const memberName = $derived(new Map($members.map((m) => [m.user_id, m.name])));
	const labelName = $derived(new Map($labels.map((l) => [l.id, l.name])));

	function firstName(ids: string[], names: Map<string, string>): string | null {
		const sorted = ids
			.map((id) => names.get(id))
			.filter((n): n is string => n !== undefined)
			.sort((a, b) => a.localeCompare(b));
		return sorted[0] ?? null;
	}

	function sortValue(row: Row): string | number | null {
		switch (sortKey) {
			case 'board':
				return row.order;
			case 'title':
				return row.card.title.toLowerCase();
			case 'due_date':
				return row.card.due_date;
			case 'assignee':
				return firstName(row.card.assignee_ids, memberName);
			case 'label':
				return firstName(row.card.label_ids, labelName);
			case 'created_at':
				return row.card.created_at;
		}
	}

	const rows = $derived.by(() => {
		const all: Row[] = lists.flatMap((list) =>
			list.cards.map((card) => ({ card, listName: list.name, order: 0 }))
		);
		all.forEach((row, index) => (row.order = index));
		const direction = ascending ? 1 : -1;
		return all.sort((a, b) => {
			const left = sortValue(a);
			const right = sortValue(b);
			// Empty values (no due date, unassigned) always sort last.
			if (left === null || right === null) {
				return left === right ? a.order - b.order : left === null ? 1 : -1;
			}
			if (left < right) return -direction;
			if (left > right) return direction;
			return a.order - b.order;
		});
	});

	function sortBy(key: SortKey) {
		if (sortKey === key) {
			ascending = !ascending;
		} else {
			sortKey = key;
			ascending = true;
		}
	}

	function sortIndicator(key: SortKey): string {
		if (sortKey !== key) return '';
		return ascending ? '▲' : '▼';
	}

	async function update(card: BoardCard, changes: CardChanges) {
		try {
			applyCardUpdated(await api.updateCard(card.id, changes));
		} catch (err) {
			toastError(err);
		}
	}

	function saveTitle(card: BoardCard, value: string) {
		const title = value.trim();
		if (title && title !== card.title) void update(card, { title });
	}

	async function changeStatus(card: BoardCard, select: HTMLSelectElement) {
		if (select.value === card.list_id) return;
		const moved = await moveCardToList(card.id, select.value);
		if (!moved) select.value = card.list_id;
	}

	async function assign(card: BoardCard, userId: string) {
		if (!userId) return;
		try {
			await api.addAssignee(card.id, userId);
			setCardRelations(card.id, { assignee_ids: [...card.assignee_ids, userId] });
		} catch (err) {
			toastError(err);
		}
	}

	async function unassign(card: BoardCard, userId: string) {
		try {
			await api.removeAssignee(card.id, userId);
			setCardRelations(card.id, {
				assignee_ids: card.assignee_ids.filter((id) => id !== userId)
			});
		} catch (err) {
			toastError(err);
		}
	}
</script>

{#snippet header(key: SortKey, label: string)}
	<th class="px-3 py-2 text-left">
		<button
			type="button"
			class="flex items-center gap-1 text-xs font-semibold uppercase tracking-wide text-slate-500 hover:text-slate-800"
			onclick={() => sortBy(key)}
		>
			{label}
			<span class="text-xs">{sortIndicator(key)}</span>
		</button>
	</th>
{/snippet}

<div class="h-full overflow-auto p-4">
	<table class="w-full min-w-max border-separate border-spacing-0 rounded-lg bg-white text-sm shadow-sm">
		<thead class="sticky top-0 bg-slate-50">
			<tr>
				{@render header('title', 'Title')}
				{@render header('board', 'Status')}
				{@render header('assignee', 'Assignees')}
				{@render header('label', 'Labels')}
				{@render header('due_date', 'Due')}
				{@render header('created_at', 'Created')}
			</tr>
		</thead>
		<tbody>
			{#each rows as { card, listName } (card.id)}
				{@const assigned = $members.filter((m) => card.assignee_ids.includes(m.user_id))}
				{@const unassigned = $members.filter((m) => !card.assignee_ids.includes(m.user_id))}
				<tr class="border-t border-slate-100 hover:bg-slate-50">
					<td class="border-t border-slate-100 px-3 py-2">
						<div class="flex items-center gap-2">
							{#if canEdit}
								<input
									class="w-full rounded border border-transparent px-1 py-0.5 hover:border-slate-300 focus:border-indigo-500 focus:outline-none"
									value={card.title}
									maxlength={MAX_TITLE_CHARS}
									aria-label="Title"
									onchange={(e) => saveTitle(card, e.currentTarget.value)}
									onkeydown={(e) => {
										if (e.key === 'Enter') e.currentTarget.blur();
									}}
								/>
							{:else}
								<span class="px-1">{card.title}</span>
							{/if}
							<button
								type="button"
								class="shrink-0 text-xs text-indigo-600 hover:underline"
								onclick={() => onopencard(card.id)}>Open</button
							>
						</div>
					</td>
					<td class="border-t border-slate-100 px-3 py-2 text-slate-600">
						{#if canEdit}
							<select
								class="rounded border border-slate-200 bg-white px-1 py-0.5 text-sm"
								aria-label="Status"
								value={card.list_id}
								onchange={(e) => changeStatus(card, e.currentTarget)}
							>
								{#each lists as list (list.id)}
									<option value={list.id}>{list.name}</option>
								{/each}
							</select>
						{:else}
							{listName}
						{/if}
					</td>
					<td class="border-t border-slate-100 px-3 py-2">
						<div class="flex items-center gap-1">
							{#each assigned as member (member.user_id)}
								{#if canEdit}
									<button
										type="button"
										title="Unassign {member.name}"
										onclick={() => unassign(card, member.user_id)}
									>
										<Avatar name={member.name} color={member.avatar_color} size="sm" />
									</button>
								{:else}
									<Avatar name={member.name} color={member.avatar_color} size="sm" />
								{/if}
							{/each}
							{#if canEdit && unassigned.length > 0}
								<select
									class="rounded border border-slate-200 bg-white px-1 py-0.5 text-xs text-slate-500"
									aria-label="Assign member"
									value=""
									onchange={(e) => {
										void assign(card, e.currentTarget.value);
										e.currentTarget.value = '';
									}}
								>
									<option value="">+ Assign</option>
									{#each unassigned as member (member.user_id)}
										<option value={member.user_id}>{member.name}</option>
									{/each}
								</select>
							{/if}
						</div>
					</td>
					<td class="border-t border-slate-100 px-3 py-2">
						<div class="flex flex-wrap gap-1">
							{#each $labels.filter((l) => card.label_ids.includes(l.id)) as label (label.id)}
								<Badge color={label.color}>{label.name}</Badge>
							{/each}
						</div>
					</td>
					<td class="border-t border-slate-100 px-3 py-2">
						<DatePicker
							value={card.due_date}
							disabled={!canEdit}
							onchange={(due_date) => update(card, { due_date })}
						/>
					</td>
					<td class="border-t border-slate-100 px-3 py-2 text-slate-500">
						{formatDate(card.created_at)}
					</td>
				</tr>
			{:else}
				<tr>
					<td colspan="6" class="px-3 py-8 text-center text-slate-500">No cards on this board yet.</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>
