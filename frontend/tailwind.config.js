import defaultTheme from 'tailwindcss/defaultTheme';

/** @type {import('tailwindcss').Config} */
export default {
	content: ['./src/**/*.{html,js,svelte,ts}'],
	theme: {
		extend: {
			fontFamily: {
				sans: ['"Inter Variable"', ...defaultTheme.fontFamily.sans]
			},
			boxShadow: {
				card: '0 1px 2px rgb(15 23 42 / 0.06), 0 1px 3px rgb(15 23 42 / 0.08)',
				lift: '0 8px 24px -6px rgb(15 23 42 / 0.18), 0 2px 6px rgb(15 23 42 / 0.08)'
			}
		}
	},
	plugins: []
};
