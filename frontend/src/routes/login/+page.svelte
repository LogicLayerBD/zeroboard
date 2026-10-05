<script lang="ts">
	import { goto } from '$app/navigation';
	import { ApiError, errorMessage } from '$lib/api';
	import AuthCard from '$lib/components/layout/AuthCard.svelte';
	import { signIn } from '$lib/stores/auth.store';

	let email = $state('');
	let password = $state('');
	let error = $state('');
	let submitting = $state(false);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		error = '';
		submitting = true;
		try {
			await signIn(email, password);
			await goto('/', { replaceState: true });
		} catch (err) {
			// Never reveal whether the email exists.
			error =
				err instanceof ApiError && err.status === 401
					? 'Invalid email or password.'
					: errorMessage(err);
		} finally {
			submitting = false;
		}
	}
</script>

<svelte:head><title>Sign in · ZeroBoard</title></svelte:head>

<AuthCard title="Sign in to your account">
	<form class="space-y-4" onsubmit={submit}>
		<label class="block text-sm">
			<span class="mb-1 block font-medium text-slate-700">Email</span>
			<input
				type="email"
				autocomplete="email"
				required
				class="w-full rounded-md border border-slate-300 px-3 py-2 focus:border-indigo-500 focus:outline-none"
				bind:value={email}
			/>
		</label>
		<label class="block text-sm">
			<span class="mb-1 block font-medium text-slate-700">Password</span>
			<input
				type="password"
				autocomplete="current-password"
				required
				class="w-full rounded-md border border-slate-300 px-3 py-2 focus:border-indigo-500 focus:outline-none"
				bind:value={password}
			/>
		</label>
		{#if error}
			<p class="text-sm text-red-600" role="alert">{error}</p>
		{/if}
		<button
			type="submit"
			class="w-full rounded-md bg-indigo-600 py-2 text-sm font-semibold text-white hover:bg-indigo-700 disabled:opacity-50"
			disabled={submitting}>{submitting ? 'Signing in…' : 'Sign in'}</button
		>
	</form>
	<p class="mt-6 text-center text-sm text-slate-500">
		No account? <a href="/register" class="font-medium text-indigo-600 hover:underline">Create one</a>
	</p>
</AuthCard>
