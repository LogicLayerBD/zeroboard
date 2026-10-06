<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import Icon from './Icon.svelte';

	interface Props extends Omit<HTMLInputAttributes, 'type' | 'value'> {
		value: string;
	}

	let { value = $bindable(''), class: className = 'input w-full py-2.5', ...rest }: Props = $props();

	let revealed = $state(false);
</script>

<div class="relative">
	<input {...rest} type={revealed ? 'text' : 'password'} class="{className} pr-10" bind:value />
	<button
		type="button"
		class="absolute inset-y-0 right-0 flex items-center px-3 text-slate-400 hover:text-slate-600 dark:text-slate-500 dark:hover:text-slate-300"
		aria-label={revealed ? 'Hide password' : 'Show password'}
		aria-pressed={revealed}
		title={revealed ? 'Hide password' : 'Show password'}
		onclick={() => (revealed = !revealed)}
	>
		<Icon name={revealed ? 'eyeOff' : 'eye'} />
	</button>
</div>
