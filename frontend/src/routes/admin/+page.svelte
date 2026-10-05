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

<div class="mx-auto w-full max-w-3xl overflow-y-auto p-8">
	<h1 class="text-2xl font-semibold">Admin</h1>
	{#if !isAdmin}
		<p class="mt-4 text-sm text-slate-500">Only instance administrators can view this page.</p>
	{:else if !info || !storage}
		<Spinner />
	{:else}
		<section class="mt-6 grid gap-3 sm:grid-cols-4">
			{#each [['Version', info.version], ['Uptime', formatUptime(info.uptime_seconds)], ['Database', formatBytes(info.db_size_bytes)], ['Users', String(info.user_count)]] as [label, value] (label)}
				<div class="rounded-lg border border-slate-200 bg-white p-4">
					<p class="text-xs uppercase tracking-wide text-slate-500">{label}</p>
					<p class="mt-1 text-lg font-semibold">{value}</p>
				</div>
			{/each}
		</section>

		<section class="mt-8">
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-slate-500">
				Attachment storage · {formatBytes(storage.total_bytes)} in {storage.attachment_count} files
			</h2>
			<table class="w-full rounded-lg border border-slate-200 bg-white text-sm">
				<thead class="bg-slate-50 text-left text-xs uppercase tracking-wide text-slate-500">
					<tr>
						<th class="px-4 py-2">Workspace</th>
						<th class="px-4 py-2 text-right">Files</th>
						<th class="px-4 py-2 text-right">Size</th>
					</tr>
				</thead>
				<tbody>
					{#each storage.workspaces as row (row.workspace_id ?? 'archived')}
						<tr class="border-t border-slate-100">
							<td class="px-4 py-2">{row.workspace_name ?? 'Archived boards'}</td>
							<td class="px-4 py-2 text-right">{row.attachment_count}</td>
							<td class="px-4 py-2 text-right">{formatBytes(row.size_bytes)}</td>
						</tr>
					{:else}
						<tr><td colspan="3" class="px-4 py-6 text-center text-slate-500">No attachments yet.</td></tr>
					{/each}
				</tbody>
			</table>
		</section>
	{/if}
</div>
