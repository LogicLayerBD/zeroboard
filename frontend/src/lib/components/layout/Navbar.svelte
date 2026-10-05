<script lang="ts">
	import { goto } from '$app/navigation';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
	import Logo from '$lib/components/ui/Logo.svelte';
	import ThemeToggle from '$lib/components/ui/ThemeToggle.svelte';
	import NotificationBell from '$lib/components/notifications/NotificationBell.svelte';
	import { currentUser, signOut } from '$lib/stores/auth.store';
	import { sidebarOpen } from '$lib/stores/ui.store';
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
	class="relative z-20 flex h-14 shrink-0 items-center gap-2 border-b border-slate-200/80 dark:border-slate-800/80 bg-white/80 dark:bg-slate-900/80 px-3 backdrop-blur sm:gap-4 sm:px-4"
>
	{#if $workspaceId}
		<button
			type="button"
			class="icon-btn p-2 md:hidden"
			aria-label="Open sidebar"
			aria-expanded={$sidebarOpen}
			onclick={() => sidebarOpen.set(true)}><Icon name="menu" class="h-5 w-5" /></button
		>
	{/if}
	<a href="/" aria-label="ZeroBoard home">
		<span class="sm:hidden"><Logo compact /></span>
		<span class="hidden sm:inline"><Logo /></span>
	</a>

	{#if $workspaces.length > 0}
		<span class="hidden h-6 w-px bg-slate-200 dark:bg-slate-700 sm:block"></span>
		<div class="relative hidden sm:block">
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
				class="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400 dark:text-slate-500"
			/>
		</div>
	{/if}

	{#if $connectionState === 'reconnecting'}
		<span
			class="inline-flex items-center gap-1.5 rounded-full bg-amber-50 dark:bg-amber-500/10 px-2.5 py-1 text-xs font-medium text-amber-700 dark:text-amber-400 ring-1 ring-inset ring-amber-200 dark:ring-amber-500/30"
		>
			<span class="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500"></span>
			<span class="hidden sm:inline">Reconnecting…</span>
		</span>
	{/if}

	<div class="ml-auto flex items-center gap-0.5 sm:gap-1">
		{#if $currentUser?.role === 'admin'}
			<a href="/admin" class="btn-ghost btn-sm" aria-label="Admin"
				><Icon name="shield" class="h-4 w-4" /><span class="hidden sm:inline">Admin</span></a
			>
		{/if}
		<ThemeToggle />
		<NotificationBell />
		{#if $currentUser}
			<span class="mx-1 hidden h-6 w-px bg-slate-200 dark:bg-slate-700 sm:block"></span>
			<div class="flex items-center gap-2 pl-1">
				<Avatar name={$currentUser.name} color={$currentUser.avatar_color} />
				<span class="hidden text-sm font-medium text-slate-700 dark:text-slate-200 sm:inline">{$currentUser.name}</span>
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
