import { derived, get, writable } from 'svelte/store';
import * as api from '$lib/api';
import type { Board, Member, Workspace, WorkspaceRole } from '$lib/types';
import { currentUser } from './auth.store';
import { toastError } from './toast.store';

const ROLE_RANK: Record<WorkspaceRole, number> = { viewer: 0, member: 1, admin: 2 };

export const workspaces = writable<Workspace[]>([]);
export const workspaceId = writable<string | null>(null);
export const members = writable<Member[]>([]);
export const boards = writable<Board[]>([]);
export const workspaceLoading = writable(false);

export const currentWorkspace = derived([workspaces, workspaceId], ([all, id]) =>
	all.find((w) => w.id === id) ?? null
);

/** The signed-in user's role in the current workspace, or null if not a member. */
export const myRole = derived(
	[members, currentUser],
	([all, user]) => all.find((m) => m.user_id === user?.id)?.role ?? null
);

export function hasRole(role: WorkspaceRole | null, required: WorkspaceRole): boolean {
	return role !== null && ROLE_RANK[role] >= ROLE_RANK[required];
}

export async function loadWorkspaces(): Promise<void> {
	try {
		workspaces.set(await api.listWorkspaces());
	} catch (err) {
		toastError(err);
	}
}

/** Loads members and live boards for `id`; stale responses for a previous workspace are dropped. */
export async function openWorkspace(id: string): Promise<void> {
	workspaceId.set(id);
	members.set([]);
	boards.set([]);
	workspaceLoading.set(true);
	try {
		const [memberList, boardList] = await Promise.all([api.listMembers(id), api.listBoards(id)]);
		if (get(workspaceId) !== id) return;
		members.set(memberList);
		boards.set(boardList);
	} catch (err) {
		if (get(workspaceId) === id) toastError(err);
	} finally {
		if (get(workspaceId) === id) workspaceLoading.set(false);
	}
}

export function closeWorkspace(): void {
	workspaceId.set(null);
	members.set([]);
	boards.set([]);
}

export function upsertMember(member: Member): void {
	members.update((all) =>
		all.some((m) => m.user_id === member.user_id)
			? all.map((m) => (m.user_id === member.user_id ? member : m))
			: [...all, member]
	);
}

export function removeMemberLocally(userId: string): void {
	members.update((all) => all.filter((m) => m.user_id !== userId));
}

export function upsertBoard(board: Board): void {
	boards.update((all) =>
		all.some((b) => b.id === board.id)
			? all.map((b) => (b.id === board.id ? board : b))
			: [...all, board]
	);
}

export function removeBoardLocally(boardId: string): void {
	boards.update((all) => all.filter((b) => b.id !== boardId));
}

export function upsertWorkspace(workspace: Workspace): void {
	workspaces.update((all) =>
		all.some((w) => w.id === workspace.id)
			? all.map((w) => (w.id === workspace.id ? workspace : w))
			: [...all, workspace]
	);
}

export function removeWorkspaceLocally(id: string): void {
	workspaces.update((all) => all.filter((w) => w.id !== id));
}
