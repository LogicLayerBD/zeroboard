<script lang="ts">
	interface Props {
		/** Hides the wordmark, leaving only the square mark. */
		compact?: boolean;
		/** Light wordmark for dark backgrounds. */
		inverted?: boolean;
	}

	let { compact = false, inverted = false }: Props = $props();
</script>

<span class="inline-flex items-center gap-2">
	<span
		class="flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-indigo-500 via-violet-500 to-fuchsia-500 shadow-md shadow-indigo-500/30"
	>
		<svg class="h-4 w-4 text-white" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
			<rect x="3" y="4" width="5" height="14" rx="1.5" />
			<rect x="9.5" y="4" width="5" height="9" rx="1.5" />
			<rect x="16" y="4" width="5" height="17" rx="1.5" />
		</svg>
	</span>
	{#if !compact}
		<span class="text-lg font-bold tracking-tight {inverted ? 'text-white' : 'text-slate-900 dark:text-white'}"
			>Zero<span class="bg-gradient-to-r from-indigo-500 to-violet-500 bg-clip-text text-transparent"
				>Board</span
			></span
		>
	{/if}
</span>
