<script lang="ts">
	import * as api from '$lib/api';
	import { formatDateTime, formatMinutes } from '$lib/format';
	import Icon from '$lib/components/ui/Icon.svelte';
	import { currentUser } from '$lib/stores/auth.store';
	import { toastError } from '$lib/stores/toast.store';
	import { members, myRole } from '$lib/stores/workspace.store';
	import type { TimeEntry } from '$lib/types';

	/** Mirrors the backend's accepted range for one entry (1 minute to 24 hours). */
	const MIN_MINUTES = 1;
	const MAX_MINUTES = 24 * 60;
	const MAX_DESCRIPTION_CHARS = 500;

	interface Props {
		cardId: string;
		entries: TimeEntry[];
		canEdit: boolean;
		onchange: (entries: TimeEntry[]) => void;
	}

	let { cardId, entries, canEdit, onchange }: Props = $props();

	let minutes = $state<number | null>(null);
	let description = $state('');
	let saving = $state(false);

	const total = $derived(entries.reduce((sum, e) => sum + e.minutes, 0));
	const memberName = $derived(new Map($members.map((m) => [m.user_id, m.name])));

	async function log(event: SubmitEvent) {
		event.preventDefault();
		if (minutes === null || saving) return;
		saving = true;
		try {
			const logged = await api.createTimeEntry(cardId, minutes, description.trim() || null);
			onchange([...entries, logged.time_entry]);
			minutes = null;
			description = '';
		} catch (err) {
			toastError(err);
		} finally {
			saving = false;
		}
	}

	async function remove(entry: TimeEntry) {
		try {
			await api.deleteTimeEntry(entry.id);
			onchange(entries.filter((e) => e.id !== entry.id));
		} catch (err) {
			toastError(err);
		}
	}
</script>

<section>
	<div class="mb-3 flex items-center justify-between">
		<h3 class="flex items-center gap-2 text-sm font-semibold text-slate-900 dark:text-white">
			<Icon name="clock" class="h-4 w-4 text-slate-400 dark:text-slate-500" />Time tracking
		</h3>
		<span
			class="rounded-full bg-indigo-50 dark:bg-indigo-500/10 px-2.5 py-0.5 text-xs font-semibold text-indigo-700 dark:text-indigo-300 ring-1 ring-inset ring-indigo-200 dark:ring-indigo-500/30"
			>Total {formatMinutes(total)}</span
		>
	</div>
	{#if canEdit}
		<form class="mb-3 flex gap-2" onsubmit={log}>
			<input
				type="number"
				class="input w-24 px-2.5 py-1.5"
				placeholder="Minutes"
				min={MIN_MINUTES}
				max={MAX_MINUTES}
				required
				bind:value={minutes}
			/>
			<input
				class="input min-w-0 flex-1 px-2.5 py-1.5"
				placeholder="What did you work on? (optional)"
				maxlength={MAX_DESCRIPTION_CHARS}
				bind:value={description}
			/>
			<button
				type="submit"
				class="btn-secondary btn-sm"
				disabled={saving}>Log</button
			>
		</form>
	{/if}
	<ul class="divide-y divide-slate-100 dark:divide-slate-800">
		{#each entries as entry (entry.id)}
			<li class="group flex items-center gap-3 py-2 text-sm">
				<span class="w-14 font-semibold tabular-nums text-slate-900 dark:text-white">{formatMinutes(entry.minutes)}</span>
				<span class="flex-1 truncate text-slate-600 dark:text-slate-300">{entry.description ?? ''}</span>
				<span class="text-xs text-slate-400 dark:text-slate-500"
					>{memberName.get(entry.user_id) ?? 'Former member'} · {formatDateTime(entry.logged_at)}</span
				>
				{#if canEdit && (entry.user_id === $currentUser?.id || $myRole === 'admin')}
					<button
						type="button"
						class="icon-btn p-1 opacity-0 hover:bg-red-50 dark:hover:bg-red-500/10 hover:text-red-600 dark:hover:text-red-400 focus-visible:opacity-100 group-hover:opacity-100"
						aria-label="Delete time entry"
						onclick={() => remove(entry)}><Icon name="x" class="h-3.5 w-3.5" /></button
					>
				{/if}
			</li>
		{/each}
	</ul>
</section>
