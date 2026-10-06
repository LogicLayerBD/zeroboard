<script lang="ts">
	import { goto } from '$app/navigation';
	import { ApiError, errorMessage, registrationOpen } from '$lib/api';
	import AuthCard from '$lib/components/layout/AuthCard.svelte';
	import PasswordInput from '$lib/components/ui/PasswordInput.svelte';
	import { signIn } from '$lib/stores/auth.store';

	let email = $state('');
	let password = $state('');
	let error = $state('');
	let submitting = $state(false);
	// Hidden until the server confirms sign-up is open, so it never flashes.
	let canRegister = $state(false);

	$effect(() => {
		registrationOpen()
			.then((open) => (canRegister = open))
			.catch(() => (canRegister = false));
	});

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		error = '';
		submitting = true;
		try {
			await signIn(email, password);
			await goto('/', { replaceState: true });
		} catch (err) {
			// Never reveal whether the email exists. 403 (deactivated) only follows a correct password.
			if (err instanceof ApiError && err.status === 401) {
				error = 'Invalid email or password.';
			} else if (err instanceof ApiError && err.status === 403 && err.message) {
				error = err.message;
			} else {
				error = errorMessage(err);
			}
		} finally {
			submitting = false;
		}
	}
</script>

<svelte:head><title>Sign in · ZeroBoard</title></svelte:head>

<AuthCard title="Sign in to your account">
	<form class="space-y-5" onsubmit={submit}>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700 dark:text-slate-200">Email</span>
			<input
				type="email"
				autocomplete="email"
				required
				class="input w-full py-2.5"
				bind:value={email}
			/>
		</label>
		<label class="block">
			<span class="mb-1.5 block text-sm font-medium text-slate-700 dark:text-slate-200">Password</span>
			<PasswordInput autocomplete="current-password" required bind:value={password} />
		</label>
		{#if error}
			<p class="rounded-lg bg-red-50 dark:bg-red-500/10 px-3 py-2 text-sm text-red-700 dark:text-red-400 ring-1 ring-inset ring-red-200 dark:ring-red-500/30" role="alert">{error}</p>
		{/if}
		<button
			type="submit"
			class="btn-primary w-full py-2.5"
			disabled={submitting}>{submitting ? 'Signing in…' : 'Sign in'}</button
		>
	</form>
	<p class="mt-8 text-center text-sm text-slate-500 dark:text-slate-400">
		{#if canRegister}
			No account? <a href="/register" class="link">Create one</a>
		{:else}
			No account? Ask your administrator to add you.
		{/if}
	</p>
</AuthCard>
