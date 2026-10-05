import { get, writable } from 'svelte/store';

export type ThemePreference = 'light' | 'dark' | 'system';

const STORAGE_KEY = 'zeroboard.theme';
const DARK_QUERY = '(prefers-color-scheme: dark)';

function readPreference(): ThemePreference {
	try {
		const stored = localStorage.getItem(STORAGE_KEY);
		return stored === 'light' || stored === 'dark' ? stored : 'system';
	} catch {
		// Storage can be unavailable (e.g. privacy modes); fall back to the OS setting.
		return 'system';
	}
}

export const themePreference = writable<ThemePreference>(readPreference());

export function setTheme(preference: ThemePreference): void {
	themePreference.set(preference);
	try {
		if (preference === 'system') localStorage.removeItem(STORAGE_KEY);
		else localStorage.setItem(STORAGE_KEY, preference);
	} catch {
		// Not persisting is fine; the choice still applies for this session.
	}
}

/** Keeps the `dark` class on <html> in sync with the preference and the OS setting. */
export function startThemeSync(): () => void {
	const media = window.matchMedia(DARK_QUERY);
	const apply = () => {
		const preference = get(themePreference);
		const dark = preference === 'dark' || (preference === 'system' && media.matches);
		document.documentElement.classList.toggle('dark', dark);
	};
	const unsubscribe = themePreference.subscribe(apply);
	media.addEventListener('change', apply);
	return () => {
		unsubscribe();
		media.removeEventListener('change', apply);
	};
}
