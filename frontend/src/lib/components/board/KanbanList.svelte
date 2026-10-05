<script lang="ts">
	import { flip } from 'svelte/animate';
	import { dndzone, dragHandle, TRIGGERS, type DndEvent } from 'svelte-dnd-action';
	import * as api from '$lib/api';
	import Icon from '$lib/components/ui/Icon.svelte';
	import { FLIP_DURATION_MS, isShadowItem } from '$lib/dnd';
	import {
		applyCardCreated,
		applyListDeleted,
		applyListUpdated,
		setListCards
	} from '$lib/stores/board.store';
	import { toastError } from '$lib/stores/toast.store';
	import type { BoardCard, ListWithCards } from '$lib/types';
	import KanbanCard from './KanbanCard.svelte';

	const MAX_NAME_CHARS = 255;
	const MAX_TITLE_CHARS = 500;

	interface Props {
		list: ListWithCards;
		/** Column colour (hex), assigned by position on the board. */
		accent: string;
		canEdit: boolean;
		onopencard: (cardId: string) => void;
		ondragstart: (cardId: string, listId: string) => void;
		ondrop: (cardId: string, listId: string, cards: BoardCard[]) => void;
	}

	let { list, accent, canEdit, onopencard, ondragstart, ondrop }: Props = $props();

	let renaming = $state(false);
	let nameDraft = $state('');
	let adding = $state(false);
	let titleDraft = $state('');
	let saving = $state(false);

	function handleConsider(event: CustomEvent<DndEvent<BoardCard>>) {
		if (event.detail.info.trigger === TRIGGERS.DRAG_STARTED) {
			ondragstart(event.detail.info.id, list.id);
		}
		setListCards(list.id, event.detail.items);
	}

	function handleFinalize(event: CustomEvent<DndEvent<BoardCard>>) {
		const { items, info } = event.detail;
		setListCards(list.id, items);
		// Finalize fires on both zones; only the zone that now holds the card persists the move.
		if (items.some((c) => c.id === info.id)) ondrop(info.id, list.id, items);
	}

	function startRename() {
		if (!canEdit) return;
		nameDraft = list.name;
		renaming = true;
	}

	async function saveRename() {
		const name = nameDraft.trim();
		renaming = false;
		if (!name || name === list.name) return;
		try {
			applyListUpdated(await api.renameList(list.id, name));
		} catch (err) {
			toastError(err);
		}
	}

	function onRenameKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') void saveRename();
		if (event.key === 'Escape') renaming = false;
	}

	async function removeList() {
		const message =
			list.cards.length > 0
				? `Delete the "${list.name}" column and its ${list.cards.length} card(s)?`
				: `Delete the "${list.name}" column?`;
		if (!confirm(message)) return;
		try {
			await api.deleteList(list.id);
			applyListDeleted(list.id);
		} catch (err) {
			toastError(err);
		}
	}

	async function addCard(event: SubmitEvent) {
		event.preventDefault();
		const title = titleDraft.trim();
		if (!title || saving) return;
		saving = true;
		try {
			applyCardCreated(await api.createCard(list.id, title));
			titleDraft = '';
		} catch (err) {
			toastError(err);
		} finally {
			saving = false;
		}
	}

	function onAddKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			adding = false;
			titleDraft = '';
		}
	}

	function autofocus(node: HTMLElement) {
		node.focus();
	}
</script>

<section
	class="group/column flex max-h-full w-72 shrink-0 flex-col overflow-hidden rounded-2xl bg-slate-200/50 ring-1 ring-slate-900/5 backdrop-blur-sm"
	aria-label={list.name}
>
	<span class="h-1 shrink-0" style:background-color={accent}></span>
	<header class="flex items-center gap-1.5 px-3 pb-2 pt-2.5">
		{#if canEdit}
			<span
				use:dragHandle
				class="-ml-1 cursor-grab rounded p-0.5 text-slate-400 hover:bg-slate-300/50 hover:text-slate-600"
				aria-label="Drag column {list.name}"><Icon name="grip" class="h-4 w-4" strokeWidth={3} /></span
			>
		{/if}
		<span class="h-2.5 w-2.5 shrink-0 rounded-full" style:background-color={accent}></span>
		{#if renaming}
			<input
				class="input min-w-0 flex-1 px-2 py-1 text-sm font-semibold"
				maxlength={MAX_NAME_CHARS}
				bind:value={nameDraft}
				onblur={saveRename}
				onkeydown={onRenameKeydown}
				use:autofocus
			/>
		{:else}
			<button
				type="button"
				class="min-w-0 flex-1 truncate text-left text-sm font-semibold text-slate-800 {canEdit
					? 'cursor-text'
					: 'cursor-default'}"
				disabled={!canEdit}
				title={canEdit ? 'Rename column' : list.name}
				onclick={startRename}>{list.name}</button
			>
		{/if}
		<span
			class="rounded-full bg-white/80 px-2 py-0.5 text-xs font-semibold tabular-nums text-slate-500 ring-1 ring-inset ring-slate-900/5"
			>{list.cards.length}</span
		>
		{#if canEdit}
			<button
				type="button"
				class="icon-btn p-1 opacity-0 hover:bg-red-50 hover:text-red-600 focus-visible:opacity-100 group-hover/column:opacity-100"
				aria-label="Delete column {list.name}"
				onclick={removeList}><Icon name="trash" class="h-3.5 w-3.5" /></button
			>
		{/if}
	</header>

	<div
		class="flex min-h-16 flex-1 flex-col gap-2 overflow-y-auto px-2 pb-1 pt-0.5"
		use:dndzone={{
			items: list.cards,
			type: 'card',
			flipDurationMs: FLIP_DURATION_MS,
			dragDisabled: !canEdit,
			dropTargetStyle: {}
		}}
		onconsider={handleConsider}
		onfinalize={handleFinalize}
		aria-label="Cards in {list.name}"
	>
		{#each list.cards as card (card.id)}
			<div animate:flip={{ duration: FLIP_DURATION_MS }} class={isShadowItem(card) ? 'opacity-40' : ''}>
				<KanbanCard {card} onopen={onopencard} />
			</div>
		{/each}
	</div>

	{#if canEdit}
		<div class="p-2">
			{#if adding}
				<form onsubmit={addCard}>
					<input
						class="input w-full"
						placeholder="Card title, then Enter"
						maxlength={MAX_TITLE_CHARS}
						bind:value={titleDraft}
						onkeydown={onAddKeydown}
						onblur={() => {
							if (!titleDraft.trim()) adding = false;
						}}
						use:autofocus
					/>
				</form>
			{:else}
				<button
					type="button"
					class="flex w-full items-center gap-1.5 rounded-lg px-2 py-1.5 text-left text-sm font-medium text-slate-500 transition hover:bg-white/70 hover:text-slate-800"
					onclick={() => (adding = true)}><Icon name="plus" />Add card</button
				>
			{/if}
		</div>
	{/if}
</section>
