<script lang="ts">
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import Avatar from '$lib/components/ui/Avatar.svelte';
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

<div class="overflow-y-auto p-8">
	{#if $workspaceLoading}
		<Spinner />
	{:else}
		<div class="flex items-center gap-3">
			{#if renaming}
				<form onsubmit={saveRename}>
					<input
						class="rounded-md border border-indigo-400 px-2 py-1 text-2xl font-semibold focus:outline-none"
						maxlength={MAX_NAME_CHARS}
						bind:value={nameDraft}
						onblur={() => (renaming = false)}
					/>
				</form>
			{:else}
				<h1 class="text-2xl font-semibold">{$currentWorkspace?.name ?? ''}</h1>
				{#if isAdmin}
					<button type="button" class="text-sm text-indigo-600 hover:underline" onclick={startRename}
						>Rename</button
					>
				{/if}
			{/if}
		</div>

		<section class="mt-8">
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-slate-500">Boards</h2>
			<ul class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
				{#each $boards as board (board.id)}
					<li>
						<a
							href="/{encodeURIComponent($currentWorkspace?.id ?? '')}/{encodeURIComponent(board.id)}"
							class="block h-24 rounded-lg bg-indigo-600 p-4 font-semibold text-white shadow-sm hover:bg-indigo-700"
						>
							{board.name}
						</a>
					</li>
				{:else}
					<li class="text-sm text-slate-500">No boards yet. Create one from the sidebar.</li>
				{/each}
			</ul>
		</section>

		<section class="mt-10 max-w-3xl">
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-slate-500">Members</h2>
			<ul class="divide-y divide-slate-100 rounded-lg border border-slate-200 bg-white">
				{#each $members as member (member.user_id)}
					<li class="flex items-center gap-3 px-4 py-3">
						<Avatar name={member.name} color={member.avatar_color} />
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm font-medium">
								{member.name}
								{#if member.user_id === $currentUser?.id}<span class="text-slate-400">(you)</span>{/if}
							</p>
							<p class="truncate text-xs text-slate-500">
								{member.email} · joined {formatDate(member.joined_at)}
							</p>
						</div>
						{#if isAdmin}
							<select
								class="rounded-md border border-slate-300 bg-white px-2 py-1 text-sm"
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
								class="text-sm text-slate-400 hover:text-red-600"
								aria-label="Remove {member.name}"
								onclick={() => remove(member.user_id, member.name)}>Remove</button
							>
						{:else}
							<span class="rounded bg-slate-100 px-2 py-0.5 text-xs capitalize text-slate-600"
								>{member.role}</span
							>
						{/if}
					</li>
				{/each}
			</ul>

			{#if isAdmin}
				<form class="mt-4 flex flex-wrap gap-2" onsubmit={invite}>
					<input
						type="email"
						class="min-w-0 flex-1 rounded-md border border-slate-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none"
						placeholder="teammate@example.com"
						maxlength={MAX_EMAIL_LEN}
						required
						bind:value={inviteEmail}
					/>
					<select
						class="rounded-md border border-slate-300 bg-white px-2 py-2 text-sm"
						aria-label="Role"
						bind:value={inviteRole}
					>
						{#each ROLES as role (role)}
							<option value={role}>{role}</option>
						{/each}
					</select>
					<button
						type="submit"
						class="rounded-md bg-indigo-600 px-4 py-2 text-sm font-semibold text-white hover:bg-indigo-700 disabled:opacity-50"
						disabled={inviting}>Invite</button
					>
				</form>
				<p class="mt-2 text-xs text-slate-500">
					Teammates need an account first: they register, then you add them by email.
				</p>

				<div class="mt-10 rounded-lg border border-red-200 p-4">
					<h3 class="text-sm font-semibold text-red-700">Danger zone</h3>
					<p class="mt-1 text-sm text-slate-600">
						Deleting the workspace archives its boards and removes all members.
					</p>
					<button
						type="button"
						class="mt-3 rounded-md border border-red-300 px-3 py-1.5 text-sm text-red-700 hover:bg-red-50"
						onclick={deleteWorkspace}>Delete workspace</button
					>
				</div>
			{/if}
		</section>
	{/if}
</div>
