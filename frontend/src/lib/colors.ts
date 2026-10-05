/** Accent palette for columns and boards; every shade stays readable under white text. */
const ACCENTS = [
	'#6366f1', // indigo
	'#0ea5e9', // sky
	'#f59e0b', // amber
	'#10b981', // emerald
	'#ec4899', // pink
	'#8b5cf6', // violet
	'#14b8a6', // teal
	'#f97316' // orange
] as const;

/** Columns are coloured by position so the board reads left-to-right like a pipeline. */
export function columnAccent(index: number): string {
	return ACCENTS[((index % ACCENTS.length) + ACCENTS.length) % ACCENTS.length];
}

/** Stable accent for an entity (board, workspace) derived from its id. */
export function accentFor(id: string): string {
	let hash = 0;
	for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) | 0;
	return columnAccent(Math.abs(hash));
}
