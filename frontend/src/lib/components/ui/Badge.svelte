<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		color: string;
		children: Snippet;
	}

	let { color, children }: Props = $props();
</script>

<!-- Label colors are user-chosen hex values, so they cannot be static Tailwind classes. -->
<span
	class="inline-flex max-w-full items-center truncate rounded px-2 py-0.5 text-xs font-medium text-white"
	style:background-color={color}
>
	{@render children()}
</span>
