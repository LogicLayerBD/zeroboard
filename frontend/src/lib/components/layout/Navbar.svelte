<script lang="ts">
	import { goto } from '$app/navigation';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
	import Logo from '$lib/components/ui/Logo.svelte';
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

<header
	class="relative z-20 flex h-14 shrink-0 items-center gap-4 border-b border-slate-200/80 bg-white/80 px-4 backdrop-blur"
>
	<a href="/" aria-label="ZeroBoard home"><Logo /></a>

	{#if $workspaces.length > 0}
		<span class="h-6 w-px bg-slate-200"></span>
		<div class="relative">
			<select
				class="input appearance-none py-1.5 pl-3 pr-8 font-medium"
				aria-label="Switch workspace"
				value={$workspaceId ?? ''}
				onchange={(e) => switchWorkspace(e.currentTarget.value)}
			>
				<option value="">All workspaces</option>
				{#each $workspaces as workspace (workspace.id)}
					<option value={workspace.id}>{workspace.name}</option>
				{/each}
			</select>
			<Icon
				name="chevronDown"
				class="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400"
			/>
		</div>
	{/if}

	{#if $connectionState === 'reconnecting'}
		<span
			class="inline-flex items-center gap-1.5 rounded-full bg-amber-50 px-2.5 py-1 text-xs font-medium text-amber-700 ring-1 ring-inset ring-amber-200"
		>
			<span class="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500"></span>
			Reconnecting…
		</span>
	{/if}

	<div class="ml-auto flex items-center gap-1">
		{#if $currentUser?.role === 'admin'}
			<a href="/admin" class="btn-ghost btn-sm"><Icon name="shield" class="h-4 w-4" />Admin</a>
		{/if}
		<NotificationBell />
		{#if $currentUser}
			<span class="mx-1 h-6 w-px bg-slate-200"></span>
			<div class="flex items-center gap-2 pl-1">
				<Avatar name={$currentUser.name} color={$currentUser.avatar_color} />
				<span class="hidden text-sm font-medium text-slate-700 sm:inline">{$currentUser.name}</span>
			</div>
			<button
				type="button"
				class="icon-btn ml-1"
				aria-label="Sign out"
				title="Sign out"
				onclick={handleSignOut}><Icon name="logout" class="h-4 w-4" /></button
			>
		{/if}
	</div>
</header>
