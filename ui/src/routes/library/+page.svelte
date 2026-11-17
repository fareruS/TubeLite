<script lang="ts">
	import { getHistory, getLibraryPlaylists, getWatchLater } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { FeedItem } from '$lib/types';

	let watchLater = $state<FeedItem[]>([]);
	let history = $state<FeedItem[]>([]);
	let playlists = $state<FeedItem[]>([]);
	let wlError = $state('');
	let histError = $state('');
	let plError = $state('');
	let wlLoading = $state(false);
	let histLoading = $state(false);
	let plLoading = $state(false);

	$effect(() => {
		wlLoading = true;
		getWatchLater()
			.then((p) => {
				watchLater = p.items;
			})
			.catch((e) => {
				wlError = String(e);
			})
			.finally(() => {
				wlLoading = false;
			});
		histLoading = true;
		getHistory()
			.then((p) => {
				history = p.items;
			})
			.catch((e) => {
				histError = String(e);
			})
			.finally(() => {
				histLoading = false;
			});
		plLoading = true;
		getLibraryPlaylists()
			.then((p) => {
				playlists = p.items;
			})
			.catch((e) => {
				plError = String(e);
			})
			.finally(() => {
				plLoading = false;
			});
	});
</script>

<h1>Library</h1>

<h2>Watch later</h2>
{#if wlLoading}<p>Loading…</p>{/if}
{#if wlError}<p role="alert">couldn't load feed: {wlError}</p>{/if}
{#if !wlLoading && !wlError && watchLater.length === 0}
	<p>Nothing saved — use “+ Watch later” on any video.</p>
{/if}
<VideoList items={watchLater} />

<h2>History</h2>
{#if histLoading}<p>Loading…</p>{/if}
{#if histError}<p role="alert">couldn't load feed: {histError}</p>{/if}
{#if !histLoading && !histError && history.length === 0}
	<p>Nothing watched yet.</p>
{/if}
<VideoList items={history} />

<h2>Playlists</h2>
{#if plLoading}<p>Loading…</p>{/if}
{#if plError}<p role="alert">couldn't load feed: {plError}</p>{/if}
{#if !plLoading && !plError && playlists.length === 0}
	<p>No playlists yet.</p>
{/if}
<ul>
	{#each playlists as pl (pl.video_id)}
		<li>
			<a href={`/playlist/${pl.video_id}`}>{pl.title}</a>
			{#if pl.channel_name} — {pl.channel_name}{/if}
		</li>
	{/each}
</ul>
