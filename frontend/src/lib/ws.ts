import { get } from 'svelte/store';
import * as api from './api';
import type { ServerEvent, ServerEventType } from './types';
import { currentUser } from './stores/auth.store';
import {
	applyCardCreated,
	applyCardDeleted,
	applyCardMoved,
	applyCardUpdated,
	applyListCreated,
	applyListDeleted,
	applyListReordered,
	applyListUpdated,
	reloadBoard
} from './stores/board.store';
import { addNotification, loadNotifications } from './stores/notifications.store';
import { upsertMember } from './stores/workspace.store';
import { activeUsers, connectionState } from './stores/ws.store';

const WS_PATH = '/ws';
const WS_PROTOCOL = 'zeroboard.v1';
/** The access token travels as a subprotocol so it never appears in URLs or logs. */
const BEARER_PROTOCOL_PREFIX = 'bearer.';
const CLOSE_NORMAL = 1000;
/** Server close code: the access token expired; refresh and reconnect. */
const CLOSE_TOKEN_EXPIRED = 4401;
const INITIAL_BACKOFF_MS = 1000;
const MAX_BACKOFF_MS = 30_000;

const KNOWN_EVENTS = new Set<ServerEventType>([
	'CARD_CREATED',
	'CARD_UPDATED',
	'CARD_DELETED',
	'CARD_MOVED',
	'LIST_CREATED',
	'LIST_UPDATED',
	'LIST_DELETED',
	'LIST_REORDERED',
	'MEMBER_JOINED',
	'MEMBER_LEFT',
	'PRESENCE_UPDATE',
	'NOTIFICATION',
	'PING'
]);

let socket: WebSocket | null = null;
let running = false;
let hasConnected = false;
let attempt = 0;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let joinedBoard: string | null = null;
let boardAccessLostHandler: ((boardId: string) => void) | null = null;

/** Called when the current user is removed from the workspace of the open board. */
export function setBoardAccessLostHandler(handler: (boardId: string) => void): void {
	boardAccessLostHandler = handler;
}

/** Opens the single app-wide connection; safe to call repeatedly. */
export function startRealtime(): void {
	if (running) return;
	running = true;
	hasConnected = false;
	attempt = 0;
	void open(false);
}

export function stopRealtime(): void {
	running = false;
	if (reconnectTimer) clearTimeout(reconnectTimer);
	reconnectTimer = null;
	joinedBoard = null;
	const current = socket;
	socket = null;
	current?.close(CLOSE_NORMAL);
	connectionState.set('idle');
	activeUsers.set([]);
}

export function joinBoard(boardId: string): void {
	joinedBoard = boardId;
	activeUsers.set([]);
	send({ type: 'JOIN_BOARD', payload: { boardId } });
}

export function leaveBoard(boardId: string): void {
	if (joinedBoard !== boardId) return;
	joinedBoard = null;
	activeUsers.set([]);
	send({ type: 'LEAVE_BOARD', payload: { boardId } });
}

function send(message: object): void {
	if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(message));
}

async function open(forceRefresh: boolean): Promise<void> {
	if (!running) return;
	connectionState.set(hasConnected ? 'reconnecting' : 'connecting');

	if (forceRefresh || api.accessTokenExpiresSoon()) {
		const result = await api.refreshAccessToken();
		if (!running) return;
		if (result === 'rejected') {
			stopRealtime();
			api.handleUnauthorized();
			return;
		}
		if (result === 'unavailable') {
			scheduleReconnect();
			return;
		}
	}
	const token = api.getAccessToken();
	if (!token) {
		scheduleReconnect();
		return;
	}

	const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
	const ws = new WebSocket(`${scheme}://${location.host}${WS_PATH}`, [
		WS_PROTOCOL,
		`${BEARER_PROTOCOL_PREFIX}${token}`
	]);
	socket = ws;

	ws.onopen = () => {
		if (socket !== ws) return;
		const isReconnect = hasConnected;
		hasConnected = true;
		attempt = 0;
		connectionState.set('open');
		if (joinedBoard) send({ type: 'JOIN_BOARD', payload: { boardId: joinedBoard } });
		// Events sent while disconnected are lost; re-fetch to catch up.
		if (isReconnect) {
			void reloadBoard();
			void loadNotifications();
		}
	};
	ws.onmessage = (event: MessageEvent) => {
		if (socket === ws) handleMessage(event.data);
	};
	ws.onclose = (event: CloseEvent) => {
		if (socket !== ws) return;
		socket = null;
		if (!running) return;
		if (event.code === CLOSE_TOKEN_EXPIRED) {
			void open(true);
			return;
		}
		scheduleReconnect();
	};
}

function scheduleReconnect(): void {
	if (!running) return;
	const delay = Math.min(INITIAL_BACKOFF_MS * 2 ** attempt, MAX_BACKOFF_MS);
	attempt += 1;
	connectionState.set('reconnecting');
	if (reconnectTimer) clearTimeout(reconnectTimer);
	reconnectTimer = setTimeout(() => {
		reconnectTimer = null;
		void open(false);
	}, delay);
}

function isServerEvent(value: unknown): value is ServerEvent {
	return (
		typeof value === 'object' &&
		value !== null &&
		'type' in value &&
		typeof value.type === 'string' &&
		KNOWN_EVENTS.has(value.type as ServerEventType)
	);
}

function handleMessage(data: unknown): void {
	if (typeof data !== 'string') return;
	let message: unknown;
	try {
		message = JSON.parse(data);
	} catch {
		return;
	}
	if (!isServerEvent(message)) return;

	switch (message.type) {
		case 'PING':
			send({ type: 'PONG' });
			break;
		case 'CARD_CREATED':
			applyCardCreated(message.payload.card);
			break;
		case 'CARD_UPDATED':
			applyCardUpdated(message.payload.card);
			break;
		case 'CARD_DELETED':
			applyCardDeleted(message.payload.cardId);
			break;
		case 'CARD_MOVED':
			applyCardMoved(message.payload.cardId, message.payload.toListId, message.payload.position);
			break;
		case 'LIST_CREATED':
			applyListCreated(message.payload.list);
			break;
		case 'LIST_UPDATED':
			applyListUpdated(message.payload.list);
			break;
		case 'LIST_DELETED':
			applyListDeleted(message.payload.listId);
			break;
		case 'LIST_REORDERED':
			applyListReordered(message.payload.listId, message.payload.position);
			break;
		case 'MEMBER_JOINED':
			if (message.payload.boardId === joinedBoard) upsertMember(message.payload.user);
			break;
		case 'MEMBER_LEFT':
			// Also sent when a user simply closes the board; PRESENCE_UPDATE covers that case.
			// Receiving it about ourselves means we were removed from the workspace.
			if (message.payload.boardId === joinedBoard && message.payload.userId === get(currentUser)?.id) {
				const boardId = joinedBoard;
				joinedBoard = null;
				activeUsers.set([]);
				boardAccessLostHandler?.(boardId);
			}
			break;
		case 'PRESENCE_UPDATE':
			if (message.payload.boardId === joinedBoard) activeUsers.set(message.payload.activeUsers);
			break;
		case 'NOTIFICATION':
			addNotification(message.payload.notification);
			break;
	}
}
