<script lang="ts">
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import { formatDate, isOverdue } from '$lib/format';
	import { labels } from '$lib/stores/board.store';
	import { members } from '$lib/stores/workspace.store';
	import type { BoardCard } from '$lib/types';

	interface Props {
		card: BoardCard;
		onopen: (cardId: string) => void;
	}

	let { card, onopen }: Props = $props();

	const cardLabels = $derived($labels.filter((l) => card.label_ids.includes(l.id)));
	const assignees = $derived($members.filter((m) => card.assignee_ids.includes(m.user_id)));
	const overdue = $derived(isOverdue(card.due_date));
</script>

<button
	type="button"
	class="block w-full rounded-lg border border-slate-200 bg-white p-3 text-left shadow-sm hover:border-indigo-300"
	onclick={() => onopen(card.id)}
>
	{#if cardLabels.length > 0}
		<div class="mb-2 flex flex-wrap gap-1">
			{#each cardLabels as label (label.id)}
				<Badge color={label.color}>{label.name}</Badge>
			{/each}
		</div>
	{/if}
	<p class="break-words text-sm font-medium text-slate-800">{card.title}</p>
	{#if card.due_date !== null || card.description || assignees.length > 0}
		<div class="mt-2 flex items-center gap-2 text-xs text-slate-500">
			{#if card.due_date !== null}
				<span
					class="rounded px-1.5 py-0.5 {overdue ? 'bg-red-100 text-red-700' : 'bg-slate-100'}"
					title="Due date"
				>
					{formatDate(card.due_date)}
				</span>
			{/if}
			{#if card.description}
				<span title="Has description">≡</span>
			{/if}
			<span class="ml-auto flex -space-x-1">
				{#each assignees as member (member.user_id)}
					<Avatar name={member.name} color={member.avatar_color} size="sm" />
				{/each}
			</span>
		</div>
	{/if}
</button>
