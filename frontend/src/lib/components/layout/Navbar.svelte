<script lang="ts">
	import { goto } from '$app/navigation';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import NotificationBell from '$lib/components/notifications/NotificationBell.svelte';
	import { currentUser, signOut } from '$lib/stores/auth.store';
	import { workspaceId, workspaces } from '$lib/stores/workspace.store';
	import { connectionState } from '$lib/stores/ws.store';

	async function switchWorkspace(id: string) {
		await goto(id ? `/${encodeURIComponent(id)}` : '/');
	}

	async function handleSignOut() {
		await signOut();
		await goto('/login');
	}
</script>

<header class="flex h-14 shrink-0 items-center gap-4 border-b border-slate-200 bg-white px-4">
	<a href="/" class="text-lg font-bold text-indigo-600">ZeroBoard</a>

	{#if $workspaces.length > 0}
		<select
			class="rounded-md border border-slate-300 bg-white px-2 py-1 text-sm focus:border-indigo-500 focus:outline-none"
			aria-label="Switch workspace"
			value={$workspaceId ?? ''}
			onchange={(e) => switchWorkspace(e.currentTarget.value)}
		>
			<option value="">All workspaces</option>
			{#each $workspaces as workspace (workspace.id)}
				<option value={workspace.id}>{workspace.name}</option>
			{/each}
		</select>
	{/if}

	{#if $connectionState === 'reconnecting'}
		<span class="rounded bg-amber-100 px-2 py-0.5 text-xs text-amber-800">Reconnecting…</span>
	{/if}

	<div class="ml-auto flex items-center gap-3">
		{#if $currentUser?.role === 'admin'}
			<a href="/admin" class="text-sm text-slate-600 hover:text-indigo-600">Admin</a>
		{/if}
		<NotificationBell />
		{#if $currentUser}
			<div class="flex items-center gap-2">
				<Avatar name={$currentUser.name} color={$currentUser.avatar_color} />
				<span class="hidden text-sm font-medium sm:inline">{$currentUser.name}</span>
			</div>
			<button
				type="button"
				class="rounded-md px-2 py-1 text-sm text-slate-600 hover:bg-slate-100"
				onclick={handleSignOut}>Sign out</button
			>
		{/if}
	</div>
</header>
