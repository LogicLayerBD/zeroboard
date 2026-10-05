<script lang="ts">
	import Icon, { type IconName } from '$lib/components/ui/Icon.svelte';
	import { setTheme, themePreference, type ThemePreference } from '$lib/stores/theme.store';

	const NEXT: Record<ThemePreference, ThemePreference> = {
		system: 'light',
		light: 'dark',
		dark: 'system'
	};
	const ICONS: Record<ThemePreference, IconName> = { system: 'monitor', light: 'sun', dark: 'moon' };
	const LABELS: Record<ThemePreference, string> = {
		system: 'Theme: system',
		light: 'Theme: light',
		dark: 'Theme: dark'
	};
</script>

<button
	type="button"
	class="icon-btn p-2"
	aria-label="{LABELS[$themePreference]} (click to change)"
	title={LABELS[$themePreference]}
	onclick={() => setTheme(NEXT[$themePreference])}
>
	<Icon name={ICONS[$themePreference]} class="h-5 w-5" />
</button>
