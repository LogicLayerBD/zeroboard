<script lang="ts">
	import * as api from '$lib/api';
	import { formatDateTime, formatMinutes } from '$lib/format';
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
	<div class="mb-2 flex items-center justify-between">
		<h3 class="text-sm font-semibold text-slate-700">Time tracking</h3>
		<span class="text-xs font-medium text-slate-500">Total: {formatMinutes(total)}</span>
	</div>
	{#if canEdit}
		<form class="mb-2 flex gap-2" onsubmit={log}>
			<input
				type="number"
				class="w-24 rounded-md border border-slate-300 px-2 py-1 text-sm focus:border-indigo-500 focus:outline-none"
				placeholder="Minutes"
				min={MIN_MINUTES}
				max={MAX_MINUTES}
				required
				bind:value={minutes}
			/>
			<input
				class="flex-1 rounded-md border border-slate-300 px-2 py-1 text-sm focus:border-indigo-500 focus:outline-none"
				placeholder="What did you work on? (optional)"
				maxlength={MAX_DESCRIPTION_CHARS}
				bind:value={description}
			/>
			<button
				type="submit"
				class="rounded-md bg-slate-800 px-3 py-1 text-sm text-white hover:bg-slate-900 disabled:opacity-50"
				disabled={saving}>Log</button
			>
		</form>
	{/if}
	<ul class="space-y-1">
		{#each entries as entry (entry.id)}
			<li class="flex items-center gap-2 text-sm">
				<span class="w-14 font-medium">{formatMinutes(entry.minutes)}</span>
				<span class="flex-1 truncate text-slate-600">{entry.description ?? ''}</span>
				<span class="text-xs text-slate-400"
					>{memberName.get(entry.user_id) ?? 'Former member'} · {formatDateTime(entry.logged_at)}</span
				>
				{#if canEdit && (entry.user_id === $currentUser?.id || $myRole === 'admin')}
					<button
						type="button"
						class="text-xs text-slate-400 hover:text-red-600"
						aria-label="Delete time entry"
						onclick={() => remove(entry)}>✕</button
					>
				{/if}
			</li>
		{/each}
	</ul>
</section>
