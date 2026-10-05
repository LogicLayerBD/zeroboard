<script lang="ts">
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import { currentUser } from '$lib/stores/auth.store';
	import { toastError } from '$lib/stores/toast.store';
	import { upsertWorkspace, workspaces } from '$lib/stores/workspace.store';

	const MAX_NAME_CHARS = 255;

	let name = $state('');
	let creating = $state(false);

	async function create(event: SubmitEvent) {
		event.preventDefault();
		const trimmed = name.trim();
		if (!trimmed) return;
		creating = true;
		try {
			const workspace = await api.createWorkspace(trimmed);
			upsertWorkspace(workspace);
			name = '';
			await goto(`/${encodeURIComponent(workspace.id)}`);
		} catch (err) {
			toastError(err);
		} finally {
			creating = false;
		}
	}
</script>

<svelte:head><title>Workspaces · ZeroBoard</title></svelte:head>

<div class="mx-auto w-full max-w-3xl overflow-y-auto p-8">
	<h1 class="text-2xl font-semibold">Welcome, {$currentUser?.name}</h1>
	<p class="mt-1 text-sm text-slate-500">Pick a workspace or create a new one.</p>

	<ul class="mt-6 grid gap-3 sm:grid-cols-2">
		{#each $workspaces as workspace (workspace.id)}
			<li>
				<a
					href="/{encodeURIComponent(workspace.id)}"
					class="block rounded-lg border border-slate-200 bg-white p-4 font-medium shadow-sm hover:border-indigo-300"
				>
					{workspace.name}
				</a>
			</li>
		{:else}
			<li class="text-sm text-slate-500 sm:col-span-2">
				You're not in any workspace yet. Create one below, or ask a teammate to invite you using
				the email address you registered with.
			</li>
		{/each}
	</ul>

	<form class="mt-8 flex gap-2" onsubmit={create}>
		<input
			class="flex-1 rounded-md border border-slate-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none"
			placeholder="New workspace name"
			maxlength={MAX_NAME_CHARS}
			required
			bind:value={name}
		/>
		<button
			type="submit"
			class="rounded-md bg-indigo-600 px-4 py-2 text-sm font-semibold text-white hover:bg-indigo-700 disabled:opacity-50"
			disabled={creating}>Create workspace</button
		>
	</form>
</div>
