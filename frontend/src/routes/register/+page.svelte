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
	<form class="space-y-5" onsubmit={submit}>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700">Name</span>
			<input
				autocomplete="name"
				required
				maxlength={MAX_NAME_CHARS}
				class="input w-full py-2.5"
				bind:value={name}
			/>
		</label>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700">Email</span>
			<input
				type="email"
				autocomplete="email"
				required
				maxlength={MAX_EMAIL_LEN}
				class="input w-full py-2.5"
				bind:value={email}
			/>
		</label>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700">Password</span>
			<input
				type="password"
				autocomplete="new-password"
				required
				minlength={MIN_PASSWORD_CHARS}
				class="input w-full py-2.5"
				bind:value={password}
			/>
			<span class="mt-1 block text-xs text-slate-400">At least {MIN_PASSWORD_CHARS} characters.</span>
		</label>
		{#if error}
			<p class="rounded-lg bg-red-50 px-3 py-2 text-sm text-red-700 ring-1 ring-inset ring-red-200" role="alert">{error}</p>
		{/if}
		<button
			type="submit"
			class="btn-primary w-full py-2.5"
			disabled={submitting}>{submitting ? 'Creating account…' : 'Create account'}</button
		>
	</form>
	<p class="mt-8 text-center text-sm text-slate-500">
		Already have an account? <a href="/login" class="link"
			>Sign in</a
		>
	</p>
</AuthCard>
