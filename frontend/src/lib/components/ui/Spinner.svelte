<script lang="ts">
	interface Props {
		label?: string;
	}

	let { label = 'Loading…' }: Props = $props();
</script>

<div class="flex items-center justify-center gap-2 p-6 text-sm text-slate-500 dark:text-slate-400" role="status">
	<span class="h-5 w-5 animate-spin rounded-full border-2 border-indigo-100 border-t-indigo-600"></span>
	{label}
</div>
