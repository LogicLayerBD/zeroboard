<script lang="ts">
	import * as api from '$lib/api';
	import Modal from '$lib/components/ui/Modal.svelte';
	import { toastSuccess } from '$lib/stores/toast.store';

	/** Mirrors the backend limits so most mistakes are caught before a round trip. */
	const MIN_PASSWORD_CHARS = 8;
	const MAX_PASSWORD_BYTES = 72;

	interface Props {
		onclose: () => void;
	}

	let { onclose }: Props = $props();

	let currentPassword = $state('');
	let newPassword = $state('');
	let confirmPassword = $state('');
	let error = $state('');
	let submitting = $state(false);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		error = '';
		if (new TextEncoder().encode(newPassword).length > MAX_PASSWORD_BYTES) {
			error = `Password must be at most ${MAX_PASSWORD_BYTES} bytes.`;
			return;
		}
		if (newPassword !== confirmPassword) {
			error = 'New passwords do not match.';
			return;
		}
		submitting = true;
		try {
			await api.changePassword(currentPassword, newPassword);
			toastSuccess('Password changed. Your other sessions were signed out.');
			onclose();
		} catch (err) {
			error = api.errorMessage(err);
		} finally {
			submitting = false;
		}
	}
</script>

<Modal {onclose} label="Change password">
	<form class="mx-auto max-w-md space-y-5 p-6 sm:p-8" onsubmit={submit}>
		<h2 class="text-lg font-semibold text-slate-900 dark:text-white">Change password</h2>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700 dark:text-slate-200">Current password</span>
			<input
				type="password"
				autocomplete="current-password"
				required
				class="input w-full py-2.5"
				bind:value={currentPassword}
			/>
		</label>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700 dark:text-slate-200">New password</span>
			<input
				type="password"
				autocomplete="new-password"
				required
				minlength={MIN_PASSWORD_CHARS}
				class="input w-full py-2.5"
				bind:value={newPassword}
			/>
			<span class="mt-1 block text-xs text-slate-400 dark:text-slate-500">At least {MIN_PASSWORD_CHARS} characters.</span>
		</label>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700 dark:text-slate-200">Confirm new password</span>
			<input
				type="password"
				autocomplete="new-password"
				required
				minlength={MIN_PASSWORD_CHARS}
				class="input w-full py-2.5"
				bind:value={confirmPassword}
			/>
		</label>
		{#if error}
			<p class="rounded-lg bg-red-50 dark:bg-red-500/10 px-3 py-2 text-sm text-red-700 dark:text-red-400 ring-1 ring-inset ring-red-200 dark:ring-red-500/30" role="alert">{error}</p>
		{/if}
		<div class="flex justify-end gap-2">
			<button type="button" class="btn-ghost" onclick={onclose}>Cancel</button>
			<button type="submit" class="btn-primary" disabled={submitting}
				>{submitting ? 'Saving…' : 'Change password'}</button
			>
		</div>
	</form>
</Modal>
