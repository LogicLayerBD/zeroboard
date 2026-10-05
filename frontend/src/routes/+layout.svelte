<script lang="ts">
	import '../app.css';
	import { onMount, type Snippet } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { setUnauthorizedHandler } from '$lib/api';
	import Navbar from '$lib/components/layout/Navbar.svelte';
	import Spinner from '$lib/components/ui/Spinner.svelte';
	import Toast from '$lib/components/ui/Toast.svelte';
	import { authReady, currentUser, endSession, initAuth } from '$lib/stores/auth.store';
	import { clearNotifications, loadNotifications } from '$lib/stores/notifications.store';
	import { loadWorkspaces, workspaces } from '$lib/stores/workspace.store';
	import { startRealtime, stopRealtime } from '$lib/ws';

	const PUBLIC_PATHS = ['/login', '/register'];

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();

	const isPublic = $derived(PUBLIC_PATHS.includes(page.url.pathname));
	const userId = $derived($currentUser?.id ?? null);

	onMount(() => {
		setUnauthorizedHandler(() => {
			endSession();
			void goto('/login', { replaceState: true });
		});
		void initAuth();
	});

	// Route guard: runs once the initial session check has finished.
	$effect(() => {
		if (!$authReady) return;
		if (!userId && !isPublic) void goto('/login', { replaceState: true });
		else if (userId && isPublic) void goto('/', { replaceState: true });
	});

	// Session-scoped resources follow the signed-in user.
	$effect(() => {
		if (userId) {
			startRealtime();
			void loadNotifications();
			void loadWorkspaces();
		} else {
			stopRealtime();
			clearNotifications();
			workspaces.set([]);
		}
	});
</script>

{#if !$authReady}
	<Spinner />
{:else if userId && !isPublic}
	<div class="flex h-screen flex-col">
		<Navbar />
		<main class="flex min-h-0 flex-1">
			{@render children()}
		</main>
	</div>
{:else if !userId && isPublic}
	{@render children()}
{:else}
	<Spinner label="Redirecting…" />
{/if}

<Toast />
