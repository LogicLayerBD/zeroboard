import { writable } from 'svelte/store';
import * as api from '$lib/api';
import type { BoardCard, BoardDetails, Card, Label, List, ListWithCards } from '$lib/types';
import { toastError } from './toast.store';

export const board = writable<BoardDetails | null>(null);
export const labels = writable<Label[]>([]);
export const boardLoading = writable(false);

let activeBoardId: string | null = null;

function byPosition<T extends { id: string; position: number }>(a: T, b: T): number {
	return a.position - b.position || a.id.localeCompare(b.id);
}

/** Applies `fn` to the loaded board; events for any other board are ignored. */
function updateBoard(boardId: string, fn: (current: BoardDetails) => BoardDetails): void {
	board.update((current) => (current && current.id === boardId ? fn(current) : current));
}

function mapLists(
	current: BoardDetails,
	fn: (list: ListWithCards) => ListWithCards
): BoardDetails {
	return { ...current, lists: current.lists.map(fn) };
}

function findCard(current: BoardDetails, cardId: string): BoardCard | undefined {
	for (const list of current.lists) {
		const card = list.cards.find((c) => c.id === cardId);
		if (card) return card;
	}
	return undefined;
}

/** Loads the board and its labels. Responses for a board the user already left are dropped. */
export async function loadBoard(boardId: string, { silent = false } = {}): Promise<void> {
	activeBoardId = boardId;
	if (!silent) {
		board.set(null);
		labels.set([]);
		boardLoading.set(true);
	}
	try {
		const [details, labelList] = await Promise.all([
			api.getBoard(boardId),
			api.listLabels(boardId)
		]);
		if (activeBoardId !== boardId) return;
		board.set(details);
		labels.set(labelList);
	} catch (err) {
		if (activeBoardId === boardId) toastError(err);
	} finally {
		if (activeBoardId === boardId) boardLoading.set(false);
	}
}

/** Re-syncs the open board with the server, e.g. after a failed optimistic update. */
export function reloadBoard(): Promise<void> {
	return activeBoardId ? loadBoard(activeBoardId, { silent: true }) : Promise.resolve();
}

export function closeBoard(): void {
	activeBoardId = null;
	board.set(null);
	labels.set([]);
}

export function setLists(lists: ListWithCards[]): void {
	board.update((current) => (current ? { ...current, lists } : current));
}

export function setListCards(listId: string, cards: BoardCard[]): void {
	board.update((current) =>
		current ? mapLists(current, (l) => (l.id === listId ? { ...l, cards } : l)) : current
	);
}

export function applyCardCreated(card: Card): void {
	updateBoard(card.board_id, (current) => {
		if (findCard(current, card.id)) return current;
		const created: BoardCard = { ...card, assignee_ids: [], label_ids: [] };
		return mapLists(current, (l) =>
			l.id === card.list_id ? { ...l, cards: [...l.cards, created].sort(byPosition) } : l
		);
	});
}

/** Merges new card fields; relations (assignees, labels) are kept as they are. */
export function applyCardUpdated(card: Card): void {
	updateBoard(card.board_id, (current) => {
		if (!findCard(current, card.id)) return current;
		return mapLists(current, (l) => ({
			...l,
			cards: l.cards.map((c) => (c.id === card.id ? { ...c, ...card } : c))
		}));
	});
}

export function applyCardDeleted(cardId: string): void {
	board.update((current) =>
		current
			? mapLists(current, (l) => ({ ...l, cards: l.cards.filter((c) => c.id !== cardId) }))
			: current
	);
}

/** Idempotent: works whether or not the card was already moved locally (optimistic drag). */
export function applyCardMoved(cardId: string, toListId: string, position: number): void {
	board.update((current) => {
		if (!current) return current;
		const card = findCard(current, cardId);
		if (!card) return current;
		const moved: BoardCard = { ...card, list_id: toListId, position };
		return mapLists(current, (l) => {
			const others = l.cards.filter((c) => c.id !== cardId);
			return l.id === toListId
				? { ...l, cards: [...others, moved].sort(byPosition) }
				: { ...l, cards: others };
		});
	});
}

export function applyListCreated(list: List): void {
	updateBoard(list.board_id, (current) => {
		if (current.lists.some((l) => l.id === list.id)) return current;
		return { ...current, lists: [...current.lists, { ...list, cards: [] }].sort(byPosition) };
	});
}

export function applyListUpdated(list: List): void {
	updateBoard(list.board_id, (current) => ({
		...current,
		lists: current.lists
			.map((l) => (l.id === list.id ? { ...l, ...list, cards: l.cards } : l))
			.sort(byPosition)
	}));
}

export function applyListDeleted(listId: string): void {
	board.update((current) =>
		current ? { ...current, lists: current.lists.filter((l) => l.id !== listId) } : current
	);
}

export function applyListReordered(listId: string, position: number): void {
	board.update((current) =>
		current
			? {
					...current,
					lists: current.lists
						.map((l) => (l.id === listId ? { ...l, position } : l))
						.sort(byPosition)
				}
			: current
	);
}

export function setCardRelations(
	cardId: string,
	relations: { assignee_ids?: string[]; label_ids?: string[] }
): void {
	board.update((current) =>
		current
			? mapLists(current, (l) => ({
					...l,
					cards: l.cards.map((c) => (c.id === cardId ? { ...c, ...relations } : c))
				}))
			: current
	);
}

export function addLabelLocally(label: Label): void {
	labels.update((all) => [...all, label]);
}

export function removeLabelLocally(labelId: string): void {
	labels.update((all) => all.filter((l) => l.id !== labelId));
	board.update((current) =>
		current
			? mapLists(current, (l) => ({
					...l,
					cards: l.cards.map((c) => ({
						...c,
						label_ids: c.label_ids.filter((id) => id !== labelId)
					}))
				}))
			: current
	);
}