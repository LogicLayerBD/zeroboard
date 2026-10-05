<script lang="ts">
	import { onDestroy, type Snippet } from 'svelte';
	import { page } from '$app/state';
	import Sidebar from '$lib/components/layout/Sidebar.svelte';
	import { closeWorkspace, openWorkspace } from '$lib/stores/workspace.store';

	interface Props {
		children: Snippet;
	}

	let { children }: Props = $props();

	const workspaceId = $derived(page.params.workspaceId);

	$effect(() => {
		if (workspaceId) void openWorkspace(workspaceId);
	});

	onDestroy(closeWorkspace);
</script>

<Sidebar />
<div class="flex min-w-0 flex-1 flex-col">
	{@render children()}
</div>
