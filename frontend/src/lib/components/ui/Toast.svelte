<script lang="ts">
	import { fly } from 'svelte/transition';
	import Icon from '$lib/components/ui/Icon.svelte';
	import { dismissToast, toasts } from '$lib/stores/toast.store';

	const TOAST_ENTER_OFFSET_PX = 16;
	const TOAST_ENTER_MS = 180;
</script>

<div class="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2" aria-live="polite">
	{#each $toasts as toast (toast.id)}
		{@const isError = toast.kind === 'error'}
		<div
			class="pointer-events-auto flex items-start gap-3 rounded-xl bg-white p-3.5 text-sm shadow-lift ring-1 ring-slate-900/10"
			role={isError ? 'alert' : 'status'}
			in:fly={{ y: TOAST_ENTER_OFFSET_PX, duration: TOAST_ENTER_MS }}
		>
			<span
				class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full {isError
					? 'bg-red-100 text-red-600'
					: 'bg-emerald-100 text-emerald-600'}"
			>
				<Icon name={isError ? 'x' : 'check'} class="h-3.5 w-3.5" strokeWidth={3} />
			</span>
			<p class="flex-1 pt-0.5 text-slate-700">{toast.message}</p>
			<button
				type="button"
				class="icon-btn -m-1"
				aria-label="Dismiss"
				onclick={() => dismissToast(toast.id)}><Icon name="x" /></button
			>
		</div>
	{/each}
</div>
