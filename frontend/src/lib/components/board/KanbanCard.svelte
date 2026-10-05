<script lang="ts">
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
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
	class="group block w-full rounded-xl bg-white p-3 text-left shadow-card ring-1 ring-slate-900/5 transition hover:-translate-y-px hover:shadow-lift hover:ring-indigo-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500"
	onclick={() => onopen(card.id)}
>
	{#if cardLabels.length > 0}
		<div class="mb-2 flex flex-wrap gap-1">
			{#each cardLabels as label (label.id)}
				<Badge color={label.color}>{label.name}</Badge>
			{/each}
		</div>
	{/if}
	<p class="break-words text-sm font-medium leading-snug text-slate-800 group-hover:text-slate-950">
		{card.title}
	</p>
	{#if card.due_date !== null || card.description || assignees.length > 0}
		<div class="mt-3 flex items-center gap-2 text-xs text-slate-500">
			{#if card.due_date !== null}
				<span
					class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 font-medium {overdue
						? 'bg-red-50 text-red-700 ring-1 ring-inset ring-red-200'
						: 'bg-slate-100 text-slate-600'}"
					title={overdue ? 'Overdue' : 'Due date'}
				>
					<Icon name="calendar" class="h-3 w-3" />
					{formatDate(card.due_date)}
				</span>
			{/if}
			{#if card.description}
				<span class="text-slate-400" title="Has description"><Icon name="text" class="h-3.5 w-3.5" /></span>
			{/if}
			<span class="ml-auto flex -space-x-1.5">
				{#each assignees as member (member.user_id)}
					<Avatar name={member.name} color={member.avatar_color} size="sm" />
				{/each}
			</span>
		</div>
	{/if}
</button>
