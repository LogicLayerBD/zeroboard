import { SHADOW_ITEM_MARKER_PROPERTY_NAME } from 'svelte-dnd-action';

/** Shared by the dnd zones and `animate:flip` so their animations stay in sync. */
export const FLIP_DURATION_MS = 150;

/** True for the placeholder svelte-dnd-action renders where a dragged item would land. */
export function isShadowItem(item: object): boolean {
	return (item as Record<string, unknown>)[SHADOW_ITEM_MARKER_PROPERTY_NAME] === true;
}

/** Id of the item directly before `id`, or null when it is first (the API's `after_id`). */
export function previousId(items: { id: string }[], id: string): string | null {
	const index = items.findIndex((item) => item.id === id);
	return index > 0 ? items[index - 1].id : null;
}

/** True if `position` sorts strictly between the neighbours of `id` in `items`. */
export function fitsBetweenNeighbours(
	items: { id: string; position: number }[],
	id: string,
	position: number
): boolean {
	const index = items.findIndex((item) => item.id === id);
	if (index < 0) return false;
	const before = items[index - 1];
	const after = items[index + 1];
	return (!before || before.position < position) && (!after || position < after.position);
}
