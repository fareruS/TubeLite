<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';

	interface HistoryEntry {
		video_id: string;
		title: string;
		channel: string;
		played_at: number;
		position_secs: number;
		duration_secs: number | null;
	}

	interface WatchLaterEntry {
		video_id: string;
		title: string;
		channel: string;
		added_at: number;
	}

	let entries = $state<HistoryEntry[]>([]);
	let watchLater = $state<WatchLaterEntry[]>([]);
	let error = $state('');

	function loadWatchLater() {
		invoke<WatchLaterEntry[]>('list_watch_later')
			.then((w) => {
				watchLater = w;
			})
			.catch((e) => {
				error = String(e);
			});
	}

	$effect(() => {
		invoke<HistoryEntry[]>('list_history', { limit: 50 })
			.then((h) => {
				entries = h;
			})
			.catch((e) => {
				error = String(e);
			});
		loadWatchLater();
	});

	async function removeSaved(videoId: string) {
		await invoke('remove_watch_later', { videoId });
		loadWatchLater();
	}

	function fmt(secs: number | null | undefined): string {
		if (secs == null) return '';
		const m = Math.floor(secs / 60);
		const s = Math.floor(secs % 60);
		return `${m}:${String(s).padStart(2, '0')}`;
	}
</script>

<h1>Library</h1>
{#if error}<p role="alert">{error}</p>{/if}

<h2>Watch later</h2>
{#if watchLater.length === 0}
	<p>Nothing saved — use “+ Watch later” on any video.</p>
{:else}
	<ul>
		{#each watchLater as w (w.video_id)}
			<li>
				<a href={`/watch/${w.video_id}`}>{w.title || w.video_id}</a> — {w.channel}
				<button onclick={() => removeSaved(w.video_id)}>Remove</button>
			</li>
		{/each}
	</ul>
{/if}

<h2>History</h2>
{#if entries.length === 0 && !error}
	<p>Nothing watched yet.</p>
{:else}
	<ul>
		{#each entries as e (e.video_id)}
			<li>
				<a href={`/watch/${e.video_id}`}>{e.title}</a> — {e.channel}
				{#if e.position_secs > 5} · resume at {fmt(e.position_secs)}{/if}
				{#if e.duration_secs != null} / {fmt(e.duration_secs)}{/if}
			</li>
		{/each}
	</ul>
{/if}
