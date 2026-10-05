<script lang="ts">
	import { initials } from '$lib/format';

	interface Props {
		name: string;
		color: string;
		size?: 'sm' | 'md';
		/** Shows a green dot, e.g. for members currently viewing the board. */
		online?: boolean;
	}

	let { name, color, size = 'md', online = false }: Props = $props();

	const sizeClass = $derived(size === 'sm' ? 'h-6 w-6 text-xs' : 'h-8 w-8 text-sm');
</script>

<!-- The per-user color is data, so it cannot be a static Tailwind class. -->
<span
	class="relative inline-flex shrink-0 select-none items-center justify-center rounded-full font-semibold text-white ring-2 ring-white {sizeClass}"
	style:background-color={color}
	title={name}
>
	{initials(name)}
	{#if online}
		<span class="absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full bg-emerald-500 ring-2 ring-white"></span>
	{/if}
</span>
