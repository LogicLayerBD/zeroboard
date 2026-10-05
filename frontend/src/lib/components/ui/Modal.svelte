<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		onclose: () => void;
		label: string;
		children: Snippet;
	}

	let { onclose, label, children }: Props = $props();

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') onclose();
	}
</script>

<svelte:window {onkeydown} />

<div class="fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-slate-900/50 p-4 sm:p-10">
	<button type="button" class="fixed inset-0 cursor-default" aria-label="Close" onclick={onclose}
	></button>
	<div
		class="relative w-full max-w-3xl rounded-xl bg-white shadow-2xl"
		role="dialog"
		aria-modal="true"
		aria-label={label}
	>
		{@render children()}
	</div>
</div>
