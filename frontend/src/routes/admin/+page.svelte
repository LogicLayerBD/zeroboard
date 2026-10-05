<script lang="ts">
	import * as api from '$lib/api';
	import Spinner from '$lib/components/ui/Spinner.svelte';
	import { formatBytes } from '$lib/format';
	import { currentUser } from '$lib/stores/auth.store';
	import { toastError } from '$lib/stores/toast.store';
	import type { ServerInfo, StorageUsage } from '$lib/types';

	const SECONDS_PER_MINUTE = 60;
	const SECONDS_PER_HOUR = 60 * SECONDS_PER_MINUTE;
	const SECONDS_PER_DAY = 24 * SECONDS_PER_HOUR;

	let info = $state<ServerInfo | null>(null);
	let storage = $state<StorageUsage | null>(null);

	const isAdmin = $derived($currentUser?.role === 'admin');

	$effect(() => {
		if (!isAdmin) return;
		Promise.all([api.getServerInfo(), api.getStorageUsage()])
			.then(([serverInfo, usage]) => {
				info = serverInfo;
				storage = usage;
			})
			.catch(toastError);
	});

	function formatUptime(totalSeconds: number): string {
		const days = Math.floor(totalSeconds / SECONDS_PER_DAY);
		const hours = Math.floor((totalSeconds % SECONDS_PER_DAY) / SECONDS_PER_HOUR);
		const minutes = Math.floor((totalSeconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE);
		return days > 0 ? `${days}d ${hours}h ${minutes}m` : `${hours}h ${minutes}m`;
	}
</script>

<svelte:head><title>Admin · ZeroBoard</title></svelte:head>

<div class="mx-auto w-full max-w-5xl overflow-y-auto p-4 sm:p-8">
	<p class="text-sm font-medium text-indigo-600 dark:text-indigo-400">Instance</p>
	<h1 class="mt-1 text-3xl font-bold tracking-tight text-slate-900 dark:text-white">Admin</h1>
	{#if !isAdmin}
		<p class="mt-4 text-sm text-slate-500 dark:text-slate-400">Only instance administrators can view this page.</p>
	{:else if !info || !storage}
		<Spinner />
	{:else}
		<section class="mt-8 grid grid-cols-2 gap-4 lg:grid-cols-4">
			{#each [['Version', info.version], ['Uptime', formatUptime(info.uptime_seconds)], ['Database', formatBytes(info.db_size_bytes)], ['Users', String(info.user_count)]] as [label, value] (label)}
				<div class="panel p-5">
					<p class="section-title">{label}</p>
					<p class="mt-2 text-2xl font-bold tracking-tight text-slate-900 dark:text-white">{value}</p>
				</div>
			{/each}
		</section>

		<section class="mt-10">
			<h2 class="section-title mb-3">
				Attachment storage · {formatBytes(storage.total_bytes)} in {storage.attachment_count} files
			</h2>
			<table class="panel w-full border-separate border-spacing-0 overflow-hidden text-sm">
				<thead class="bg-slate-50 dark:bg-slate-800/50 text-left text-xs font-semibold uppercase tracking-wider text-slate-500 dark:text-slate-400">
					<tr>
						<th class="px-5 py-3">Workspace</th>
						<th class="px-5 py-3 text-right">Files</th>
						<th class="px-5 py-3 text-right">Size</th>
					</tr>
				</thead>
				<tbody>
					{#each storage.workspaces as row (row.workspace_id ?? 'archived')}
						<tr class="hover:bg-slate-50 dark:hover:bg-slate-800/60">
							<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3 font-medium text-slate-800 dark:text-slate-100"
								>{row.workspace_name ?? 'Archived boards'}</td
							>
							<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3 text-right tabular-nums"
								>{row.attachment_count}</td
							>
							<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3 text-right tabular-nums"
								>{formatBytes(row.size_bytes)}</td
							>
						</tr>
					{:else}
						<tr><td colspan="3" class="px-4 py-6 text-center text-slate-500 dark:text-slate-400">No attachments yet.</td></tr>
					{/each}
				</tbody>
			</table>
		</section>
	{/if}
</div>
