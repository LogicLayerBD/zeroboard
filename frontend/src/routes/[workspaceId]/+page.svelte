<script lang="ts">
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import { accentFor } from '$lib/colors';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
	import Spinner from '$lib/components/ui/Spinner.svelte';
	import { formatDate } from '$lib/format';
	import { currentUser } from '$lib/stores/auth.store';
	import { toastError, toastSuccess } from '$lib/stores/toast.store';
	import {
		boards,
		currentWorkspace,
		members,
		myRole,
		removeMemberLocally,
		removeWorkspaceLocally,
		upsertMember,
		upsertWorkspace,
		workspaceLoading
	} from '$lib/stores/workspace.store';
	import type { Member, WorkspaceRole } from '$lib/types';

	const MAX_NAME_CHARS = 255;
	const MAX_EMAIL_LEN = 254;
	const ROLES: WorkspaceRole[] = ['admin', 'member', 'viewer'];
	const DEFAULT_INVITE_ROLE: WorkspaceRole = 'member';
	const ROLE_BADGE: Record<WorkspaceRole, string> = {
		admin: 'bg-violet-50 dark:bg-violet-500/10 text-violet-700 dark:text-violet-300 ring-1 ring-inset ring-violet-200 dark:ring-violet-500/30',
		member: 'bg-sky-50 dark:bg-sky-500/10 text-sky-700 dark:text-sky-300 ring-1 ring-inset ring-sky-200 dark:ring-sky-500/30',
		viewer: 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 ring-1 ring-inset ring-slate-200 dark:ring-slate-700'
	};

	let inviteEmail = $state('');
	let inviteRole = $state<WorkspaceRole>(DEFAULT_INVITE_ROLE);
	let inviting = $state(false);
	let renaming = $state(false);
	let nameDraft = $state('');

	const isAdmin = $derived($myRole === 'admin');

	async function invite(event: SubmitEvent) {
		event.preventDefault();
		const workspace = $currentWorkspace;
		if (!workspace) return;
		inviting = true;
		try {
			const member = await api.inviteMember(workspace.id, inviteEmail.trim(), inviteRole);
			upsertMember(member);
			toastSuccess(`${member.name} was added to ${workspace.name}.`);
			inviteEmail = '';
			inviteRole = DEFAULT_INVITE_ROLE;
		} catch (err) {
			toastError(err);
		} finally {
			inviting = false;
		}
	}

	async function changeRole(member: Member, select: HTMLSelectElement) {
		const workspace = $currentWorkspace;
		if (!workspace) return;
		try {
			upsertMember(
				await api.changeMemberRole(workspace.id, member.user_id, select.value as WorkspaceRole)
			);
		} catch (err) {
			toastError(err);
			select.value = member.role;
		}
	}

	async function remove(userId: string, name: string) {
		const workspace = $currentWorkspace;
		if (!workspace || !confirm(`Remove ${name} from ${workspace.name}?`)) return;
		try {
			await api.removeMember(workspace.id, userId);
			removeMemberLocally(userId);
		} catch (err) {
			toastError(err);
		}
	}

	function startRename() {
		nameDraft = $currentWorkspace?.name ?? '';
		renaming = true;
	}

	async function saveRename(event: SubmitEvent) {
		event.preventDefault();
		const workspace = $currentWorkspace;
		const name = nameDraft.trim();
		renaming = false;
		if (!workspace || !name || name === workspace.name) return;
		try {
			upsertWorkspace(await api.renameWorkspace(workspace.id, name));
		} catch (err) {
			toastError(err);
		}
	}

	async function deleteWorkspace() {
		const workspace = $currentWorkspace;
		if (!workspace) return;
		const typed = prompt(
			`Deleting "${workspace.name}" archives all of its boards and removes every member.\nType the workspace name to confirm.`
		);
		if (typed !== workspace.name) return;
		try {
			await api.deleteWorkspace(workspace.id);
			removeWorkspaceLocally(workspace.id);
			await goto('/');
		} catch (err) {
			toastError(err);
		}
	}
</script>

<svelte:head><title>{$currentWorkspace?.name ?? 'Workspace'} · ZeroBoard</title></svelte:head>

<div class="overflow-y-auto">
	{#if $workspaceLoading}
		<Spinner />
	{:else}
		<div class="border-b border-slate-200 dark:border-slate-800 bg-white dark:bg-slate-900">
			<div class="mx-auto flex max-w-5xl items-center gap-4 px-4 py-6 sm:px-8 sm:py-8">
				<span
					class="flex h-14 w-14 shrink-0 items-center justify-center rounded-2xl text-2xl font-bold text-white shadow-lg"
					style:background-color={accentFor($currentWorkspace?.id ?? '')}
				>
					{($currentWorkspace?.name ?? '').charAt(0).toUpperCase()}
				</span>
				<div class="min-w-0 flex-1">
					{#if renaming}
						<form onsubmit={saveRename}>
							<input
								class="input text-2xl font-bold"
								maxlength={MAX_NAME_CHARS}
								bind:value={nameDraft}
								onblur={() => (renaming = false)}
							/>
						</form>
					{:else}
						<div class="flex items-center gap-2">
							<h1 class="truncate text-2xl font-bold sm:text-3xl tracking-tight text-slate-900 dark:text-white">
								{$currentWorkspace?.name ?? ''}
							</h1>
							{#if isAdmin}
								<button type="button" class="icon-btn" aria-label="Rename workspace" onclick={startRename}
									><Icon name="pencil" /></button
								>
							{/if}
						</div>
					{/if}
					<p class="mt-1 flex items-center gap-3 text-sm text-slate-500 dark:text-slate-400">
						<span class="inline-flex items-center gap-1"
							><Icon name="kanban" class="h-3.5 w-3.5" />{$boards.length} boards</span
						>
						<span class="inline-flex items-center gap-1"
							><Icon name="users" class="h-3.5 w-3.5" />{$members.length} members</span
						>
					</p>
				</div>
			</div>
		</div>

		<div class="mx-auto max-w-5xl px-4 py-6 sm:px-8 sm:py-8">
			<section>
				<h2 class="section-title mb-3">Boards</h2>
				<ul class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
					{#each $boards as board (board.id)}
						{@const accent = accentFor(board.id)}
						<li>
							<a
								href="/{encodeURIComponent($currentWorkspace?.id ?? '')}/{encodeURIComponent(board.id)}"
								class="panel group relative block h-28 overflow-hidden p-5 transition hover:-translate-y-0.5 hover:shadow-lift"
							>
								<span class="absolute inset-x-0 top-0 h-1.5" style:background-color={accent}></span>
								<span
									class="pointer-events-none absolute -bottom-8 -right-8 h-24 w-24 rounded-full opacity-10 transition group-hover:scale-125"
									style:background-color={accent}
								></span>
								<span class="relative flex items-start gap-2">
									<Icon name="kanban" class="mt-0.5 h-4 w-4 text-slate-400 dark:text-slate-500" />
									<span class="font-semibold text-slate-900 dark:text-white">{board.name}</span>
								</span>
							</a>
						</li>
					{:else}
						<li
							class="rounded-2xl border-2 border-dashed border-slate-200 dark:border-slate-800 p-8 text-center text-sm text-slate-500 dark:text-slate-400 sm:col-span-2 lg:col-span-3"
						>
							No boards yet. Create one from the sidebar.
						</li>
					{/each}
				</ul>
			</section>

			<section class="mt-12 max-w-3xl">
				<h2 class="section-title mb-3">Members</h2>
				<ul class="panel divide-y divide-slate-100 dark:divide-slate-800">
					{#each $members as member (member.user_id)}
						<li class="flex flex-wrap items-center gap-3 px-4 py-3.5 sm:flex-nowrap sm:px-5">
							<Avatar name={member.name} color={member.avatar_color} />
							<div class="min-w-0 flex-1">
								<p class="truncate text-sm font-medium text-slate-900 dark:text-white">
									{member.name}
									{#if member.user_id === $currentUser?.id}<span class="font-normal text-slate-400 dark:text-slate-500"
											>(you)</span
										>{/if}
								</p>
								<p class="truncate text-xs text-slate-500 dark:text-slate-400">
									{member.email} · joined {formatDate(member.joined_at)}
								</p>
							</div>
							{#if isAdmin}
								<select
									class="input w-auto py-1.5 capitalize"
									aria-label="Role for {member.name}"
									value={member.role}
									onchange={(e) => changeRole(member, e.currentTarget)}
								>
									{#each ROLES as role (role)}
										<option value={role}>{role}</option>
									{/each}
								</select>
								<button
									type="button"
									class="icon-btn hover:bg-red-50 dark:hover:bg-red-500/10 hover:text-red-600 dark:hover:text-red-400"
									aria-label="Remove {member.name}"
									title="Remove {member.name}"
									onclick={() => remove(member.user_id, member.name)}
									><Icon name="trash" /></button
								>
							{:else}
								<span
									class="rounded-full px-2.5 py-0.5 text-xs font-medium capitalize {ROLE_BADGE[member.role]}"
									>{member.role}</span
								>
							{/if}
						</li>
					{/each}
				</ul>

				{#if isAdmin}
					<form class="panel mt-4 p-5" onsubmit={invite}>
						<p class="text-sm font-semibold text-slate-900 dark:text-white">Invite a teammate</p>
						<p class="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
							Teammates need an account first: they register, then you add them by email.
						</p>
						<div class="mt-4 flex flex-wrap gap-2">
							<input
								type="email"
								class="input min-w-0 flex-1"
								placeholder="teammate@example.com"
								maxlength={MAX_EMAIL_LEN}
								required
								bind:value={inviteEmail}
							/>
							<select class="input w-auto capitalize" aria-label="Role" bind:value={inviteRole}>
								{#each ROLES as role (role)}
									<option value={role}>{role}</option>
								{/each}
							</select>
							<button type="submit" class="btn-primary" disabled={inviting}>Invite</button>
						</div>
					</form>

					<div class="mt-12 rounded-2xl border border-red-200 dark:border-red-500/30 bg-red-50/50 dark:bg-red-500/5 p-5">
						<h3 class="text-sm font-semibold text-red-700 dark:text-red-400">Danger zone</h3>
						<p class="mt-1 text-sm text-slate-600 dark:text-slate-300">
							Deleting the workspace archives its boards and removes all members.
						</p>
						<button type="button" class="btn-danger mt-4" onclick={deleteWorkspace}
							><Icon name="trash" />Delete workspace</button
						>
					</div>
				{/if}
			</section>
		</div>
	{/if}
</div>
