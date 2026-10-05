<script lang="ts">
	import { goto } from '$app/navigation';
	import * as api from '$lib/api';
	import AuthCard from '$lib/components/layout/AuthCard.svelte';
	import { signIn } from '$lib/stores/auth.store';

	/** Mirrors the backend limits so most mistakes are caught before a round trip. */
	const MIN_PASSWORD_CHARS = 8;
	const MAX_PASSWORD_BYTES = 72;
	const MAX_NAME_CHARS = 255;
	const MAX_EMAIL_LEN = 254;

	let name = $state('');
	let email = $state('');
	let password = $state('');
	let error = $state('');
	let submitting = $state(false);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		error = '';
		if (new TextEncoder().encode(password).length > MAX_PASSWORD_BYTES) {
			error = `Password must be at most ${MAX_PASSWORD_BYTES} bytes.`;
			return;
		}
		submitting = true;
		try {
			await api.register(email, name, password);
			await signIn(email, password);
			await goto('/', { replaceState: true });
		} catch (err) {
			error = api.errorMessage(err);
		} finally {
			submitting = false;
		}
	}
</script>

<svelte:head><title>Create account · ZeroBoard</title></svelte:head>

<AuthCard title="Create your account">
	<form class="space-y-4" onsubmit={submit}>
		<label class="block text-sm">
			<span class="mb-1 block font-medium text-slate-700">Name</span>
			<input
				autocomplete="name"
				required
				maxlength={MAX_NAME_CHARS}
				class="w-full rounded-md border border-slate-300 px-3 py-2 focus:border-indigo-500 focus:outline-none"
				bind:value={name}
			/>
		</label>
		<label class="block text-sm">
			<span class="mb-1 block font-medium text-slate-700">Email</span>
			<input
				type="email"
				autocomplete="email"
				required
				maxlength={MAX_EMAIL_LEN}
				class="w-full rounded-md border border-slate-300 px-3 py-2 focus:border-indigo-500 focus:outline-none"
				bind:value={email}
			/>
		</label>
		<label class="block text-sm">
			<span class="mb-1 block font-medium text-slate-700">Password</span>
			<input
				type="password"
				autocomplete="new-password"
				required
				minlength={MIN_PASSWORD_CHARS}
				class="w-full rounded-md border border-slate-300 px-3 py-2 focus:border-indigo-500 focus:outline-none"
				bind:value={password}
			/>
			<span class="mt-1 block text-xs text-slate-400">At least {MIN_PASSWORD_CHARS} characters.</span>
		</label>
		{#if error}
			<p class="text-sm text-red-600" role="alert">{error}</p>
		{/if}
		<button
			type="submit"
			class="w-full rounded-md bg-indigo-600 py-2 text-sm font-semibold text-white hover:bg-indigo-700 disabled:opacity-50"
			disabled={submitting}>{submitting ? 'Creating account…' : 'Create account'}</button
		>
	</form>
	<p class="mt-6 text-center text-sm text-slate-500">
		Already have an account? <a href="/login" class="font-medium text-indigo-600 hover:underline"
			>Sign in</a
		>
	</p>
</AuthCard>
