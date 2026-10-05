<script lang="ts">
	import { dismissToast, toasts } from '$lib/stores/toast.store';
</script>

<div class="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2" aria-live="polite">
	{#each $toasts as toast (toast.id)}
		<div
			class="pointer-events-auto flex items-start gap-3 rounded-lg px-4 py-3 text-sm text-white shadow-lg {toast.kind ===
			'error'
				? 'bg-red-600'
				: 'bg-emerald-600'}"
			role={toast.kind === 'error' ? 'alert' : 'status'}
		>
			<p class="flex-1">{toast.message}</p>
			<button
				type="button"
				class="text-white/80 hover:text-white"
				aria-label="Dismiss"
				onclick={() => dismissToast(toast.id)}>✕</button
			>
		</div>
	{/each}
</div>
