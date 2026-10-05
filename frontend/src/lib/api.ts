import type {
	Assignee,
	Attachment,
	Board,
	BoardDetails,
	Card,
	CardChanges,
	CardDetails,
	CardTimeEntries,
	Comment,
	Label,
	List,
	LoggedTimeEntry,
	Member,
	Notification,
	ServerInfo,
	Session,
	StorageUsage,
	TokenRefresh,
	User,
	Workspace,
	WorkspaceRole
} from './types';

const AUTH_PREFIX = '/api/auth/';
const MS_PER_SECOND = 1000;
/** Refresh the access token this long before it expires so requests never race the expiry. */
const TOKEN_EXPIRY_MARGIN_MS = 30 * MS_PER_SECOND;
const BLOB_URL_REVOKE_DELAY_MS = 60 * MS_PER_SECOND;

export class ApiError extends Error {
	readonly status: number;

	constructor(status: number, message: string) {
		super(message);
		this.name = 'ApiError';
		this.status = status;
	}
}

/**
 * `refreshed`: a new access token is in memory. `rejected`: the session is gone (log out).
 * `unavailable`: the server could not be reached; the session may still be valid.
 */
export type RefreshResult = 'refreshed' | 'rejected' | 'unavailable';

// The access token lives only in memory; the refresh token is an httpOnly cookie.
let accessToken: string | null = null;
let refreshInFlight: Promise<RefreshResult> | null = null;
let unauthorizedHandler: (() => void) | null = null;

export function getAccessToken(): string | null {
	return accessToken;
}

export function clearAccessToken(): void {
	accessToken = null;
}

/** Called when a request is rejected and the session cannot be refreshed. */
export function setUnauthorizedHandler(handler: () => void): void {
	unauthorizedHandler = handler;
}

export function handleUnauthorized(): void {
	accessToken = null;
	unauthorizedHandler?.();
}

/**
 * Refresh tokens rotate on every use, so concurrent callers must share one request:
 * a second parallel refresh would present an already-rotated token and end the session.
 */
export function refreshAccessToken(): Promise<RefreshResult> {
	refreshInFlight ??= (async (): Promise<RefreshResult> => {
		try {
			const res = await fetch(`${AUTH_PREFIX}refresh`, {
				method: 'POST',
				credentials: 'include'
			});
			if (res.status === 401) {
				accessToken = null;
				return 'rejected';
			}
			if (!res.ok) return 'unavailable';
			const body = (await res.json()) as TokenRefresh;
			accessToken = body.access_token;
			return 'refreshed';
		} catch {
			return 'unavailable';
		} finally {
			refreshInFlight = null;
		}
	})();
	return refreshInFlight;
}

/** True when the in-memory token is missing or about to expire. Reads `exp` without verifying. */
export function accessTokenExpiresSoon(): boolean {
	if (!accessToken) return true;
	try {
		const payload = accessToken.split('.')[1] ?? '';
		const claims: unknown = JSON.parse(atob(payload.replace(/-/g, '+').replace(/_/g, '/')));
		if (claims && typeof claims === 'object' && 'exp' in claims && typeof claims.exp === 'number') {
			return claims.exp * MS_PER_SECOND - Date.now() < TOKEN_EXPIRY_MARGIN_MS;
		}
	} catch {
		// Malformed token: treat as expired.
	}
	return true;
}

async function apiFetch(path: string, init: RequestInit, retry = true): Promise<Response> {
	const headers = new Headers(init.headers);
	if (accessToken) headers.set('Authorization', `Bearer ${accessToken}`);
	const res = await fetch(path, { ...init, headers, credentials: 'include' });
	if (res.status === 401 && retry && !path.startsWith(AUTH_PREFIX)) {
		const refreshed = await refreshAccessToken();
		if (refreshed === 'refreshed') return apiFetch(path, init, false);
		if (refreshed === 'rejected') handleUnauthorized();
	}
	return res;
}

async function toApiError(res: Response): Promise<ApiError> {
	let message = '';
	try {
		const body: unknown = await res.json();
		if (body && typeof body === 'object' && 'error' in body && typeof body.error === 'string') {
			message = body.error;
		}
	} catch {
		// Non-JSON error body; the status code alone is enough.
	}
	return new ApiError(res.status, message);
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
	const init: RequestInit = { method };
	if (body instanceof FormData) {
		init.body = body;
	} else if (body !== undefined) {
		init.body = JSON.stringify(body);
		init.headers = { 'Content-Type': 'application/json' };
	}
	const res = await apiFetch(path, init);
	if (!res.ok) throw await toApiError(res);
	if (res.status === 204) return undefined as T;
	return (await res.json()) as T;
}

const seg = encodeURIComponent;

/** Safe, user-facing text for any error thrown by this module. */
export function errorMessage(err: unknown): string {
	if (!(err instanceof ApiError)) return 'Could not reach the server. Check your connection.';
	switch (err.status) {
		// 400 messages are validation errors written for clients (e.g. "title is required").
		case 400:
			return err.message || 'The request was invalid.';
		case 401:
			return 'Your session has expired. Please sign in again.';
		case 403:
			return "You don't have permission to do that.";
		case 404:
			return 'That item no longer exists.';
		case 413:
			return 'That file is too large.';
		case 429:
			return 'Too many attempts. Please wait and try again.';
		default:
			return 'Something went wrong. Please try again.';
	}
}

// Auth

export function register(email: string, name: string, password: string): Promise<User> {
	return request('POST', `${AUTH_PREFIX}register`, { email, name, password });
}

export async function login(email: string, password: string): Promise<Session> {
	const session = await request<Session>('POST', `${AUTH_PREFIX}login`, { email, password });
	accessToken = session.access_token;
	return session;
}

export async function logout(): Promise<void> {
	try {
		await request<void>('POST', `${AUTH_PREFIX}logout`);
	} finally {
		accessToken = null;
	}
}

export function me(): Promise<User> {
	return request('GET', `${AUTH_PREFIX}me`);
}

// Workspaces

export function listWorkspaces(): Promise<Workspace[]> {
	return request('GET', '/api/workspaces');
}

export function createWorkspace(name: string): Promise<Workspace> {
	return request('POST', '/api/workspaces', { name });
}

export function renameWorkspace(id: string, name: string): Promise<Workspace> {
	return request('PATCH', `/api/workspaces/${seg(id)}`, { name });
}

export function deleteWorkspace(id: string): Promise<void> {
	return request('DELETE', `/api/workspaces/${seg(id)}`);
}

export function listMembers(workspaceId: string): Promise<Member[]> {
	return request('GET', `/api/workspaces/${seg(workspaceId)}/members`);
}

export function inviteMember(
	workspaceId: string,
	email: string,
	role: WorkspaceRole
): Promise<Member> {
	return request('POST', `/api/workspaces/${seg(workspaceId)}/members/invite`, { email, role });
}

export function removeMember(workspaceId: string, userId: string): Promise<void> {
	return request('DELETE', `/api/workspaces/${seg(workspaceId)}/members/${seg(userId)}`);
}

export function changeMemberRole(
	workspaceId: string,
	userId: string,
	role: WorkspaceRole
): Promise<Member> {
	return request('PATCH', `/api/workspaces/${seg(workspaceId)}/members/${seg(userId)}/role`, {
		role
	});
}

// Boards

export function listBoards(workspaceId: string): Promise<Board[]> {
	return request('GET', `/api/workspaces/${seg(workspaceId)}/boards`);
}

export function createBoard(workspaceId: string, name: string): Promise<Board> {
	return request('POST', `/api/workspaces/${seg(workspaceId)}/boards`, { name });
}

export function getBoard(id: string): Promise<BoardDetails> {
	return request('GET', `/api/boards/${seg(id)}`);
}

export function renameBoard(id: string, name: string): Promise<Board> {
	return request('PATCH', `/api/boards/${seg(id)}`, { name });
}

export function archiveBoard(id: string): Promise<void> {
	return request('DELETE', `/api/boards/${seg(id)}`);
}

// Lists

export function createList(boardId: string, name: string): Promise<List> {
	return request('POST', `/api/boards/${seg(boardId)}/lists`, { name });
}

export function renameList(id: string, name: string): Promise<List> {
	return request('PATCH', `/api/lists/${seg(id)}`, { name });
}

export function deleteList(id: string): Promise<void> {
	return request('DELETE', `/api/lists/${seg(id)}`);
}

/** Places the list directly after `afterId`, or first when `afterId` is null. */
export function reorderList(id: string, afterId: string | null): Promise<List> {
	return request('PATCH', `/api/lists/${seg(id)}/position`, { after_id: afterId });
}

// Cards

export function createCard(listId: string, title: string): Promise<Card> {
	return request('POST', `/api/lists/${seg(listId)}/cards`, { title });
}

export function getCard(id: string): Promise<CardDetails> {
	return request('GET', `/api/cards/${seg(id)}`);
}

export function updateCard(id: string, changes: CardChanges): Promise<Card> {
	return request('PATCH', `/api/cards/${seg(id)}`, changes);
}

export function deleteCard(id: string): Promise<void> {
	return request('DELETE', `/api/cards/${seg(id)}`);
}

/** Moves the card into `listId` directly after `afterId`, or first when `afterId` is null. */
export function moveCard(id: string, listId: string, afterId: string | null): Promise<Card> {
	return request('PATCH', `/api/cards/${seg(id)}/move`, { list_id: listId, after_id: afterId });
}

export function addAssignee(cardId: string, userId: string): Promise<Assignee> {
	return request('POST', `/api/cards/${seg(cardId)}/assignees`, { user_id: userId });
}

export function removeAssignee(cardId: string, userId: string): Promise<void> {
	return request('DELETE', `/api/cards/${seg(cardId)}/assignees/${seg(userId)}`);
}

export function addCardLabel(cardId: string, labelId: string): Promise<void> {
	return request('POST', `/api/cards/${seg(cardId)}/labels`, { label_id: labelId });
}

export function removeCardLabel(cardId: string, labelId: string): Promise<void> {
	return request('DELETE', `/api/cards/${seg(cardId)}/labels/${seg(labelId)}`);
}

// Attachments

export function uploadAttachment(cardId: string, file: File): Promise<Attachment> {
	const form = new FormData();
	form.append('file', file);
	return request('POST', `/api/cards/${seg(cardId)}/attachments`, form);
}

/** Downloads need the bearer token, so fetch the file and hand the browser a blob URL. */
export async function downloadAttachment(attachment: Attachment): Promise<void> {
	const res = await apiFetch(`/api/attachments/${seg(attachment.id)}/download`, { method: 'GET' });
	if (!res.ok) throw await toApiError(res);
	const url = URL.createObjectURL(await res.blob());
	const link = document.createElement('a');
	link.href = url;
	link.download = attachment.filename;
	link.click();
	// Revoking synchronously can cancel the download in some browsers.
	setTimeout(() => URL.revokeObjectURL(url), BLOB_URL_REVOKE_DELAY_MS);
}

export function deleteAttachment(id: string): Promise<void> {
	return request('DELETE', `/api/attachments/${seg(id)}`);
}

// Time entries

export function listTimeEntries(cardId: string): Promise<CardTimeEntries> {
	return request('GET', `/api/cards/${seg(cardId)}/time-entries`);
}

export function createTimeEntry(
	cardId: string,
	minutes: number,
	description: string | null
): Promise<LoggedTimeEntry> {
	return request('POST', `/api/cards/${seg(cardId)}/time-entries`, { minutes, description });
}

export function deleteTimeEntry(id: string): Promise<void> {
	return request('DELETE', `/api/time-entries/${seg(id)}`);
}

// Comments

export function createComment(cardId: string, body: string): Promise<Comment> {
	return request('POST', `/api/cards/${seg(cardId)}/comments`, { body });
}

export function updateComment(id: string, body: string): Promise<Comment> {
	return request('PATCH', `/api/comments/${seg(id)}`, { body });
}

export function deleteComment(id: string): Promise<void> {
	return request('DELETE', `/api/comments/${seg(id)}`);
}

// Labels

export function listLabels(boardId: string): Promise<Label[]> {
	return request('GET', `/api/boards/${seg(boardId)}/labels`);
}

export function createLabel(boardId: string, name: string, color: string): Promise<Label> {
	return request('POST', `/api/boards/${seg(boardId)}/labels`, { name, color });
}

export function deleteLabel(id: string): Promise<void> {
	return request('DELETE', `/api/labels/${seg(id)}`);
}

// Notifications

export function listNotifications(): Promise<Notification[]> {
	return request('GET', '/api/notifications');
}

export function markNotificationRead(id: string): Promise<Notification> {
	return request('PATCH', `/api/notifications/${seg(id)}/read`);
}

export function markAllNotificationsRead(): Promise<{ updated: number }> {
	return request('POST', '/api/notifications/read-all');
}

// Admin

export function getServerInfo(): Promise<ServerInfo> {
	return request('GET', '/api/admin/info');
}

export function getStorageUsage(): Promise<StorageUsage> {
	return request('GET', '/api/admin/storage');
}
