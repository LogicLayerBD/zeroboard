import { writable } from 'svelte/store';

export type CardDensity = 'compact' | 'detailed';

const CARD_DENSITY_KEY = 'zeroboard.cardDensity';

/** Whether the workspace sidebar drawer is open on small screens (always visible from `md`). */
export const sidebarOpen = writable(false);

function readCardDensity(): CardDensity {
	try {
		return localStorage.getItem(CARD_DENSITY_KEY) === 'detailed' ? 'detailed' : 'compact';
	} catch {
		return 'compact';
	}
}

/** How much of each card the Kanban board shows; remembered per browser. */
export const cardDensity = writable<CardDensity>(readCardDensity());

export function setCardDensity(density: CardDensity): void {
	cardDensity.set(density);
	try {
		localStorage.setItem(CARD_DENSITY_KEY, density);
	} catch {
		// Not persisting is fine; the choice still applies for this session.
	}
}
