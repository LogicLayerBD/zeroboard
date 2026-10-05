<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
	import Logo from '$lib/components/ui/Logo.svelte';
	import ThemeToggle from '$lib/components/ui/ThemeToggle.svelte';

	interface Props {
		title: string;
		children: Snippet;
	}

	let { title, children }: Props = $props();

	const HIGHLIGHTS = [
		['Kanban & table views', 'Drag cards across columns or edit everything in a sortable table.'],
		['Live collaboration', 'See teammates on the board and their changes as they happen.'],
		['Yours to host', 'One small binary and a SQLite file. No cloud required.']
	] as const;
</script>

<main class="grid min-h-screen lg:grid-cols-2">
	<section
		class="relative hidden overflow-hidden bg-slate-950 p-12 text-white lg:flex lg:flex-col lg:justify-between"
	>
		<div
			class="pointer-events-none absolute -left-32 -top-32 h-96 w-96 rounded-full bg-indigo-600/40 blur-3xl"
		></div>
		<div
			class="pointer-events-none absolute -bottom-40 right-0 h-[28rem] w-[28rem] rounded-full bg-fuchsia-600/30 blur-3xl"
		></div>

		<div class="relative"><Logo inverted /></div>

		<div class="relative max-w-md">
			<h2 class="text-4xl font-bold leading-tight tracking-tight">
				Plan, track and ship —
				<span class="bg-gradient-to-r from-indigo-300 to-fuchsia-300 bg-clip-text text-transparent"
					>together.</span
				>
			</h2>
			<ul class="mt-10 space-y-6">
				{#each HIGHLIGHTS as [heading, body] (heading)}
					<li class="flex gap-4">
						<span
							class="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-white/10 ring-1 ring-white/20"
						>
							<Icon name="check" class="h-3.5 w-3.5" strokeWidth={3} />
						</span>
						<span>
							<span class="block font-semibold">{heading}</span>
							<span class="block text-sm text-slate-400">{body}</span>
						</span>
					</li>
				{/each}
			</ul>
		</div>

		<p class="relative text-xs text-slate-500">Self-hosted project management for small teams.</p>
	</section>

	<section class="relative flex items-center justify-center bg-white p-6 dark:bg-slate-950 sm:p-12">
		<div class="absolute right-4 top-4"><ThemeToggle /></div>
		<div class="w-full max-w-sm">
			<div class="mb-8 lg:hidden"><Logo /></div>
			<h1 class="text-2xl font-bold tracking-tight text-slate-900 dark:text-white">{title}</h1>
			<div class="mt-8">
				{@render children()}
			</div>
		</div>
	</section>
</main>
