<script lang="ts">
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import { accentFor } from '$lib/colors';
	import Icon from '$lib/components/ui/Icon.svelte';
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

<div class="w-full overflow-y-auto">
	<div class="relative overflow-hidden border-b border-slate-200 bg-white">
		<div
			class="pointer-events-none absolute -right-24 -top-24 h-72 w-72 rounded-full bg-gradient-to-br from-indigo-200 to-fuchsia-200 opacity-60 blur-3xl"
		></div>
		<div class="relative mx-auto max-w-5xl px-8 py-10">
			<p class="text-sm font-medium text-indigo-600">Your workspaces</p>
			<h1 class="mt-1 text-3xl font-bold tracking-tight text-slate-900">
				Welcome back, {$currentUser?.name}
			</h1>
			<p class="mt-2 text-slate-500">Pick up where you left off, or start something new.</p>
		</div>
	</div>

	<div class="mx-auto max-w-5xl px-8 py-8">
		<ul class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
			{#each $workspaces as workspace (workspace.id)}
				<li>
					<a
						href="/{encodeURIComponent(workspace.id)}"
						class="panel group flex items-center gap-4 p-5 transition hover:-translate-y-0.5 hover:shadow-lift"
					>
						<span
							class="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl text-lg font-bold text-white shadow-md"
							style:background-color={accentFor(workspace.id)}
						>
							{workspace.name.charAt(0).toUpperCase()}
						</span>
						<span class="min-w-0 flex-1">
							<span class="block truncate font-semibold text-slate-900">{workspace.name}</span>
							<span class="block text-xs text-slate-500">Open workspace</span>
						</span>
						<Icon
							name="open"
							class="h-4 w-4 text-slate-300 transition group-hover:text-indigo-500"
						/>
					</a>
				</li>
			{:else}
				<li
					class="rounded-2xl border-2 border-dashed border-slate-200 p-8 text-center text-sm text-slate-500 sm:col-span-2 lg:col-span-3"
				>
					<Icon name="grid" class="mx-auto mb-3 h-8 w-8 text-slate-300" />
					You're not in any workspace yet. Create one below, or ask a teammate to invite you using
					the email address you registered with.
				</li>
			{/each}
		</ul>

		<form class="panel mt-8 flex flex-col gap-3 p-5 sm:flex-row sm:items-center" onsubmit={create}>
			<div class="flex-1">
				<p class="text-sm font-semibold text-slate-900">Create a workspace</p>
				<p class="text-xs text-slate-500">A shared home for your team's boards.</p>
			</div>
			<input
				class="input sm:w-72"
				placeholder="Workspace name"
				maxlength={MAX_NAME_CHARS}
				required
				bind:value={name}
			/>
			<button type="submit" class="btn-primary" disabled={creating}
				><Icon name="plus" />Create</button
			>
		</form>
	</div>
</div>
