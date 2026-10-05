<script lang="ts">
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
	import { formatDate, isOverdue } from '$lib/format';
	import { markdownToPlainText } from '$lib/markdown';
	import { labels } from '$lib/stores/board.store';
	import { members } from '$lib/stores/workspace.store';
	import type { BoardCard } from '$lib/types';

	interface Props {
		card: BoardCard;
		/** Shows a description preview, assignee names and the creation date. */
		detailed?: boolean;
		onopen: (cardId: string) => void;
	}

	let { card, detailed = false, onopen }: Props = $props();

	const cardLabels = $derived($labels.filter((l) => card.label_ids.includes(l.id)));
	const assignees = $derived($members.filter((m) => card.assignee_ids.includes(m.user_id)));
	const overdue = $derived(isOverdue(card.due_date));
	const preview = $derived(detailed && card.description ? markdownToPlainText(card.description) : '');
</script>

<button
	type="button"
	class="group block w-full rounded-xl bg-white dark:bg-slate-800 p-3 text-left shadow-card ring-1 ring-slate-900/5 dark:ring-white/10 transition hover:-translate-y-px hover:shadow-lift hover:ring-indigo-200 dark:hover:ring-indigo-500/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-500"
	onclick={() => onopen(card.id)}
>
	{#if cardLabels.length > 0}
		<div class="mb-2 flex flex-wrap gap-1">
			{#each cardLabels as label (label.id)}
				<Badge color={label.color}>{label.name}</Badge>
			{/each}
		</div>
	{/if}
	<p
		class="break-words leading-snug {detailed
			? 'text-[15px] font-semibold'
			: 'text-sm font-medium'} text-slate-800 dark:text-slate-100 group-hover:text-slate-950 dark:group-hover:text-white">
		{card.title}
	</p>
	{#if preview}
		<p
			class="mt-1.5 line-clamp-4 whitespace-pre-line break-words text-xs leading-relaxed text-slate-500 dark:text-slate-400"
		>
			{preview}
		</p>
	{/if}
	{#if detailed && assignees.length > 0}
		<div class="mt-3 flex flex-wrap gap-1.5">
			{#each assignees as member (member.user_id)}
				<span
					class="inline-flex items-center gap-1.5 rounded-full bg-slate-100 py-0.5 pl-0.5 pr-2 text-xs font-medium text-slate-700 dark:bg-slate-700/60 dark:text-slate-200"
				>
					<Avatar name={member.name} color={member.avatar_color} size="sm" />
					{member.name}
				</span>
			{/each}
		</div>
	{/if}
	{#if detailed || card.due_date !== null || card.description || assignees.length > 0}
		<div class="mt-3 flex items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
			{#if card.due_date !== null}
				<span
					class="inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 font-medium {overdue
						? 'bg-red-50 dark:bg-red-500/10 text-red-700 dark:text-red-400 ring-1 ring-inset ring-red-200 dark:ring-red-500/30'
						: 'bg-slate-100 dark:bg-slate-700/60 text-slate-600 dark:text-slate-300'}"
					title={overdue ? 'Overdue' : 'Due date'}
				>
					<Icon name="calendar" class="h-3 w-3" />
					{detailed ? 'Due ' : ''}{formatDate(card.due_date)}
				</span>
			{/if}
			{#if card.description && !detailed}
				<span class="text-slate-400 dark:text-slate-500" title="Has description"><Icon name="text" class="h-3.5 w-3.5" /></span>
			{/if}
			{#if detailed}
				<span class="ml-auto" title="Created">Created {formatDate(card.created_at)}</span>
			{:else}
				<span class="ml-auto flex -space-x-1.5">
					{#each assignees as member (member.user_id)}
						<Avatar name={member.name} color={member.avatar_color} size="sm" />
					{/each}
				</span>
			{/if}
		</div>
	{/if}
</button>
