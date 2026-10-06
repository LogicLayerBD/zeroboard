<script lang="ts">
	import * as api from '$lib/api';
	import Avatar from '$lib/components/ui/Avatar.svelte';
	import Icon from '$lib/components/ui/Icon.svelte';
	import { formatDate } from '$lib/format';
	import { currentUser } from '$lib/stores/auth.store';
	import { toastError, toastSuccess } from '$lib/stores/toast.store';
	import type { AdminUser, UserRole } from '$lib/types';

	const MAX_NAME_CHARS = 255;
	const MAX_EMAIL_LEN = 254;
	const ROLES: UserRole[] = ['admin', 'member'];

	let users = $state<AdminUser[]>([]);
	let search = $state('');
	let newName = $state('');
	let newEmail = $state('');
	let creating = $state(false);
	/** A temporary password to hand over; kept only until dismissed. */
	let revealed = $state<{ email: string; password: string } | null>(null);

	const filtered = $derived.by(() => {
		const q = search.trim().toLowerCase();
		if (!q) return users;
		return users.filter((u) => u.name.toLowerCase().includes(q) || u.email.toLowerCase().includes(q));
	});

	$effect(() => {
		api
			.listUsers()
			.then((list) => (users = list))
			.catch(toastError);
	});

	function replace(updated: AdminUser) {
		users = users.map((u) => (u.id === updated.id ? updated : u));
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		creating = true;
		try {
			const created = await api.createUser(newEmail.trim(), newName.trim());
			users = [...users, created.user];
			revealed = { email: created.user.email, password: created.temporary_password };
			newName = '';
			newEmail = '';
		} catch (err) {
			toastError(err);
		} finally {
			creating = false;
		}
	}

	async function changeRole(user: AdminUser, select: HTMLSelectElement) {
		const role = select.value as UserRole;
		if (!confirm(`Make ${user.name} an instance ${role}?`)) {
			select.value = user.role;
			return;
		}
		try {
			replace(await api.setUserRole(user.id, role));
		} catch (err) {
			toastError(err);
			select.value = user.role;
		}
	}

	async function resetPassword(user: AdminUser) {
		if (!confirm(`Reset the password for ${user.name}? They will be signed out everywhere.`)) return;
		try {
			const reset = await api.resetUserPassword(user.id);
			revealed = { email: user.email, password: reset.temporary_password };
		} catch (err) {
			toastError(err);
		}
	}

	async function toggleActive(user: AdminUser) {
		const active = user.deactivated_at === null;
		if (active && !confirm(`Deactivate ${user.name}? They are signed out and cannot log in.`)) return;
		try {
			replace(active ? await api.deactivateUser(user.id) : await api.reactivateUser(user.id));
			toastSuccess(`${user.name} was ${active ? 'deactivated' : 'reactivated'}.`);
		} catch (err) {
			toastError(err);
		}
	}

	async function copyPassword(password: string) {
		try {
			await navigator.clipboard.writeText(password);
			toastSuccess('Password copied.');
		} catch {
			// Clipboard needs a secure context; the password stays visible for manual copying.
		}
	}
</script>

<section class="mt-10">
	<div class="mb-3 flex flex-wrap items-center gap-3">
		<h2 class="section-title">Users · {users.length}</h2>
		<input
			type="search"
			class="input ml-auto w-full py-1.5 sm:w-64"
			placeholder="Search name or email"
			aria-label="Search users"
			bind:value={search}
		/>
	</div>

	{#if revealed}
		<div
			class="mb-4 rounded-2xl border border-amber-200 dark:border-amber-500/30 bg-amber-50 dark:bg-amber-500/10 p-4 text-sm"
			role="status"
		>
			<p class="font-medium text-amber-800 dark:text-amber-300">
				Temporary password for {revealed.email}. It is shown only once; share it securely.
			</p>
			<div class="mt-2 flex flex-wrap items-center gap-2">
				<code class="rounded-lg bg-white dark:bg-slate-900 px-3 py-1.5 font-mono text-slate-900 dark:text-white"
					>{revealed.password}</code
				>
				<button type="button" class="btn-ghost btn-sm" onclick={() => copyPassword(revealed?.password ?? '')}
					>Copy</button
				>
				<button type="button" class="btn-ghost btn-sm" onclick={() => (revealed = null)}>Done</button>
			</div>
		</div>
	{/if}

	<div class="panel overflow-x-auto">
		<table class="w-full border-separate border-spacing-0 text-sm">
			<thead class="bg-slate-50 dark:bg-slate-800/50 text-left text-xs font-semibold uppercase tracking-wider text-slate-500 dark:text-slate-400">
				<tr>
					<th class="px-5 py-3">User</th>
					<th class="px-5 py-3">Role</th>
					<th class="px-5 py-3 text-right">Workspaces</th>
					<th class="px-5 py-3">Joined</th>
					<th class="px-5 py-3">Status</th>
					<th class="px-5 py-3"><span class="sr-only">Actions</span></th>
				</tr>
			</thead>
			<tbody>
				{#each filtered as user (user.id)}
					{@const isSelf = user.id === $currentUser?.id}
					{@const active = user.deactivated_at === null}
					<tr class="hover:bg-slate-50 dark:hover:bg-slate-800/60" class:opacity-60={!active}>
						<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3">
							<div class="flex items-center gap-3">
								<Avatar name={user.name} color={user.avatar_color} />
								<div class="min-w-0">
									<p class="truncate font-medium text-slate-900 dark:text-white">
										{user.name}
										{#if isSelf}<span class="font-normal text-slate-400 dark:text-slate-500">(you)</span>{/if}
									</p>
									<p class="truncate text-xs text-slate-500 dark:text-slate-400">{user.email}</p>
								</div>
							</div>
						</td>
						<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3">
							<select
								class="input w-auto py-1 capitalize"
								aria-label="Instance role for {user.name}"
								value={user.role}
								disabled={isSelf}
								title={isSelf ? 'You cannot change your own role' : undefined}
								onchange={(e) => changeRole(user, e.currentTarget)}
							>
								{#each ROLES as role (role)}
									<option value={role}>{role}</option>
								{/each}
							</select>
						</td>
						<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3 text-right tabular-nums"
							>{user.workspace_count}</td
						>
						<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3 whitespace-nowrap"
							>{formatDate(user.created_at)}</td
						>
						<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3">
							<span
								class="rounded-full px-2.5 py-0.5 text-xs font-medium {active
									? 'bg-emerald-50 dark:bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
									: 'bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300'}"
								>{active ? 'Active' : 'Deactivated'}</span
							>
						</td>
						<td class="border-t border-slate-100 dark:border-slate-800 px-5 py-3">
							{#if !isSelf}
								<div class="flex items-center justify-end gap-1">
									<button
										type="button"
										class="icon-btn"
										aria-label="Reset password for {user.name}"
										title="Reset password"
										onclick={() => resetPassword(user)}><Icon name="key" /></button
									>
									<button type="button" class="btn-ghost btn-sm" onclick={() => toggleActive(user)}
										>{active ? 'Deactivate' : 'Reactivate'}</button
									>
								</div>
							{/if}
						</td>
					</tr>
				{:else}
					<tr><td colspan="6" class="px-4 py-6 text-center text-slate-500 dark:text-slate-400">No users match.</td></tr>
				{/each}
			</tbody>
		</table>
	</div>

	<form class="panel mt-4 p-5" onsubmit={create}>
		<p class="text-sm font-semibold text-slate-900 dark:text-white">Add a user</p>
		<p class="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
			Creates the account with a one-time temporary password. Works even when registration is disabled.
		</p>
		<div class="mt-4 flex flex-wrap gap-2">
			<input class="input min-w-0 flex-1" placeholder="Name" maxlength={MAX_NAME_CHARS} required bind:value={newName} />
			<input
				type="email"
				class="input min-w-0 flex-1"
				placeholder="person@example.com"
				maxlength={MAX_EMAIL_LEN}
				required
				bind:value={newEmail}
			/>
			<button type="submit" class="btn-primary" disabled={creating}>Add user</button>
		</div>
	</form>
</section>
