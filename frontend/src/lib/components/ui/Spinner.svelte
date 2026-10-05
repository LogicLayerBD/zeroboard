<script lang="ts">
	interface Props {
		label?: string;
	}

	let { label = 'Loading…' }: Props = $props();
</script>

<div class="flex items-center justify-center gap-2 p-6 text-sm text-slate-500" role="status">
	<span class="h-4 w-4 animate-spin rounded-full border-2 border-slate-300 border-t-indigo-600"></span>
	{label}
</div>
