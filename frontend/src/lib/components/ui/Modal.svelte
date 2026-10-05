<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fade, fly } from 'svelte/transition';

	const BACKDROP_FADE_MS = 120;
	const DIALOG_ENTER_OFFSET_PX = 12;
	const DIALOG_ENTER_MS = 180;

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

<div
	class="fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-slate-900/40 p-4 backdrop-blur-sm sm:p-10"
	transition:fade={{ duration: BACKDROP_FADE_MS }}
>
	<button type="button" class="fixed inset-0 cursor-default" aria-label="Close" onclick={onclose}
	></button>
	<div
		class="relative w-full max-w-4xl overflow-hidden rounded-2xl bg-white shadow-2xl ring-1 ring-slate-900/10"
		role="dialog"
		aria-modal="true"
		aria-label={label}
		in:fly={{ y: DIALOG_ENTER_OFFSET_PX, duration: DIALOG_ENTER_MS }}
	>
		{@render children()}
	</div>
</div>
