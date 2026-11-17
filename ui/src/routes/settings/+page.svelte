<script lang="ts">
	import { accountStatus, getCapabilities, secretBackend, signIn, signOut } from '$lib/api';
	import { DEFAULT_SETTINGS, loadSettings, saveSettings, type AppSettings } from '$lib/settings';
	import type { AccountStatus, Capabilities } from '$lib/types';
	import { pageCacheClear } from '$lib/pagecache';

	let settings = $state<AppSettings>(DEFAULT_SETTINGS);
	let caps = $state<Capabilities | null>(null);
	let account = $state<AccountStatus | null>(null);
	let backend = $state('');
	let busy = $state(false);
	let status = $state('');
	let error = $state('');

	$effect(() => {
		loadSettings()
			.then((s) => (settings = s))
			.catch((e) => (error = String(e)));
		getCapabilities()
			.then((c) => (caps = c))
			.catch((e) => (error = String(e)));
		accountStatus()
			.then((a) => (account = a))
			.catch((e) => (error = String(e)));
		secretBackend()
			.then((b) => (backend = b))
			.catch((e) => (error = String(e)));
	});

	async function save() {
		status = 'Saving…';
		try {
			await saveSettings(settings);
			pageCacheClear();
			status = 'Saved.';
		} catch (e) {
			status = '';
			error = String(e);
		}
	}

	async function doSignIn() {
		busy = true;
		error = '';
		status = 'Waiting for sign-in…';
		try {
			account = await signIn();
			status = account.signed_in ? 'Signed in.' : 'Not signed in.';
		} catch (e) {
			status = '';
			error = String(e);
		} finally {
			busy = false;
		}
	}

	async function doSignOut() {
		busy = true;
		error = '';
		try {
			account = await signOut();
			status = 'Signed out.';
		} catch (e) {
			error = String(e);
		} finally {
			busy = false;
		}
	}
</script>

<h1>Settings</h1>
{#if error}<p role="alert">{error}</p>{/if}

<h2>Account</h2>
<p>
	{#if account?.signed_in}
		Signed in{#if account.account_id} ({account.account_id}){/if}.
	{:else if account}
		Not signed in.
	{:else}
		Loading account status…
	{/if}
</p>
{#if backend}
	<p class="note">
		Cookies are stored in the {backend}.
		{#if backend === 'encrypted file'}
			No OS keyring was found on this machine, so they live in an encrypted file instead —
			less safe than the keyring (see the app docs).
		{/if}
	</p>
{/if}
{#if account?.signed_in}
	<button type="button" disabled={busy} onclick={doSignOut}>Sign out</button>
{:else}
	<button type="button" disabled={busy} onclick={doSignIn}>Sign in to YouTube</button>
	<p class="note">
		Opens a small sign-in window for Google's own sign-in page. Your password never touches
		this app — the app only keeps the session cookies the sign-in leaves behind.
	</p>
{/if}

<form
	onsubmit={(e) => {
		e.preventDefault();
		save();
	}}
>
	<label>
		Quality
		<select bind:value={settings.quality}>
			<option value="auto">Auto (720p)</option>
			<option value="144">144p</option>
			<option value="240">240p</option>
			<option value="360">360p</option>
			<option value="480">480p</option>
			<option value="720">720p</option>
			<option value="1080">1080p</option>
			<option value="audio">Audio only</option>
		</select>
	</label>

	<label class="check-inline">
		<input type="checkbox" bind:checked={settings.audioOnly} />
		Audio-only mode
	</label>

	<label>
		Theme
		<select bind:value={settings.theme}>
			<option value="system">System</option>
			<option value="light">Light</option>
			<option value="dark">Dark</option>
		</select>
	</label>

	<label>
		yt-dlp path (backup source)
		<input
			type="text"
			bind:value={settings.ytdlpPath}
			placeholder="/path/to/yt-dlp (leave empty to disable)"
		/>
	</label>

	<button type="submit">Save settings</button>
	{#if status}<span role="status">{status}</span>{/if}
</form>

{#if caps}
	<h2>Source capabilities</h2>
	<ul>
		<li>Search: {caps.search ? 'yes' : 'no'}</li>
		<li>Subscriptions: {caps.subscriptions ? 'yes' : 'no'}</li>
		<li>Captions: {caps.captions ? 'yes' : 'no'}</li>
		<li>Chapters: {caps.chapters ? 'yes' : 'no'}</li>
		<li>Audio only: {caps.audio_only ? 'yes' : 'no'}</li>
		<li>Gapless: {caps.gapless ? 'yes' : 'no'}</li>
	</ul>
	{#if caps.limitation_note}<p class="note">{caps.limitation_note}</p>{/if}
{/if}

<p class="note">
	This app is unofficial and not affiliated with YouTube or Google.
</p>

<style>
	form {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		max-width: 480px;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	label.check-inline {
		display: inline-flex;
		flex-direction: row;
		align-items: center;
		gap: 0.5rem;
	}
	.note {
		opacity: 0.7;
		font-size: 0.85rem;
	}
</style>
