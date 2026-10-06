// Mirrors the backend JSON exactly: snake_case fields, Unix-millisecond timestamps.

export type UserRole = 'admin' | 'member';
export type WorkspaceRole = 'admin' | 'member' | 'viewer';

export interface User {
	id: string;
	email: string;
	name: string;
	avatar_color: string;
	role: UserRole;
	created_at: number;
	updated_at: number;
}

export interface Session {
	access_token: string;
	token_type: string;
	expires_in: number;
	user: User;
}

export interface TokenRefresh {
	access_token: string;
	token_type: string;
	expires_in: number;
}

export interface Workspace {
	id: string;
	name: string;
	created_by: string;
	created_at: number;
	updated_at: number;
}

export interface Member {
	user_id: string;
	name: string;
	email: string;
	avatar_color: string;
	role: WorkspaceRole;
	joined_at: number;
}

export interface Board {
	id: string;
	workspace_id: string | null;
	name: string;
	archived: boolean;
	created_by: string;
	created_at: number;
	updated_at: number;
}

export interface List {
	id: string;
	board_id: string;
	name: string;
	position: number;
	created_at: number;
	updated_at: number;
}

export interface Card {
	id: string;
	list_id: string;
	board_id: string;
	title: string;
	description: string | null;
	position: number;
	due_date: number | null;
	created_by: string;
	created_at: number;
	updated_at: number;
}

export interface BoardCard extends Card {
	assignee_ids: string[];
	label_ids: string[];
}

export interface ListWithCards extends List {
	cards: BoardCard[];
}

export interface BoardDetails extends Board {
	lists: ListWithCards[];
}

export interface Label {
	id: string;
	board_id: string;
	name: string;
	color: string;
}

export interface Assignee {
	user_id: string;
	name: string;
	email: string;
	avatar_color: string;
}

export interface Attachment {
	id: string;
	card_id: string;
	filename: string;
	size_bytes: number;
	uploaded_by: string;
	uploaded_at: number;
}

export interface TimeEntry {
	id: string;
	card_id: string;
	user_id: string;
	minutes: number;
	description: string | null;
	logged_at: number;
}

export interface CardTimeEntries {
	time_entries: TimeEntry[];
	total_minutes: number;
}

export interface LoggedTimeEntry {
	time_entry: TimeEntry;
	total_minutes: number;
}

export interface Comment {
	id: string;
	card_id: string;
	user_id: string;
	body: string;
	created_at: number;
	updated_at: number;
}

export interface CardDetails extends Card {
	assignees: Assignee[];
	labels: Label[];
	attachments: Attachment[];
	time_entries: TimeEntry[];
	comments: Comment[];
}

/** Absent fields stay unchanged; `null` clears `description` / `due_date`. */
export interface CardChanges {
	title?: string;
	description?: string | null;
	due_date?: number | null;
}

export interface Notification {
	id: string;
	user_id: string;
	type: string;
	/** JSON-encoded {@link NotificationPayload}. */
	payload: string;
	read: boolean;
	created_at: number;
}

export interface NotificationPayload {
	card_id: string;
	board_id: string;
	message: string;
}

export interface ServerInfo {
	version: string;
	uptime_seconds: number;
	db_size_bytes: number;
	user_count: number;
}

export interface WorkspaceStorage {
	workspace_id: string | null;
	workspace_name: string | null;
	attachment_count: number;
	size_bytes: number;
}

export interface StorageUsage {
	total_bytes: number;
	attachment_count: number;
	workspaces: WorkspaceStorage[];
}

export interface InstanceSettings {
	/** The first account can always register, whatever this says. */
	registration_enabled: boolean;
}

export interface AdminUser {
	id: string;
	email: string;
	name: string;
	avatar_color: string;
	role: UserRole;
	created_at: number;
	/** `null` while the account is active. */
	deactivated_at: number | null;
	workspace_count: number;
}

export interface CreatedUser {
	user: AdminUser;
	/** Shown once; the server keeps only a hash. */
	temporary_password: string;
}

export interface PasswordReset {
	temporary_password: string;
}

export interface InviteCandidate {
	user_id: string;
	name: string;
	email: string;
	avatar_color: string;
}

export type ServerEvent =
	| { type: 'CARD_CREATED'; payload: { card: Card } }
	| { type: 'CARD_UPDATED'; payload: { card: Card } }
	| { type: 'CARD_DELETED'; payload: { cardId: string; listId: string } }
	| {
			type: 'CARD_MOVED';
			payload: { cardId: string; fromListId: string; toListId: string; position: number };
	  }
	| { type: 'LIST_CREATED'; payload: { list: List } }
	| { type: 'LIST_UPDATED'; payload: { list: List } }
	| { type: 'LIST_DELETED'; payload: { listId: string } }
	| { type: 'LIST_REORDERED'; payload: { listId: string; position: number } }
	| { type: 'MEMBER_JOINED'; payload: { user: Member; boardId: string } }
	| { type: 'MEMBER_LEFT'; payload: { userId: string; boardId: string } }
	| { type: 'PRESENCE_UPDATE'; payload: { boardId: string; activeUsers: string[] } }
	| { type: 'NOTIFICATION'; payload: { notification: Notification } }
	| { type: 'PING' };

export type ServerEventType = ServerEvent['type'];
