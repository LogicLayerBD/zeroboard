<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		title: string;
		children: Snippet;
	}

	let { title, children }: Props = $props();
</script>

<main class="flex min-h-screen items-center justify-center p-4">
	<div class="w-full max-w-sm rounded-xl border border-slate-200 bg-white p-8 shadow-sm">
		<p class="mb-1 text-center text-2xl font-bold text-indigo-600">ZeroBoard</p>
		<h1 class="mb-6 text-center text-sm text-slate-500">{title}</h1>
		{@render children()}
	</div>
</main>
