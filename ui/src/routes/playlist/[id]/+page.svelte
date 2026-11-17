<script lang="ts">
	import { page } from '$app/stores';
	import { goto } from '$app/navigation';
	import { getPlaylistFeed } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { FeedItem } from '$lib/types';

	let id = $derived($page.params.id ?? '');
	let items = $state<FeedItem[]>([]);
	let error = $state('');
	let loading = $state(false);

	$effect(() => {
		if (!id) return;
		loading = true;
		error = '';
		getPlaylistFeed(id)
			.then((p) => {
				items = p.items;
			})
			.catch((e) => {
				error = String(e);
			})
			.finally(() => {
				loading = false;
			});
	});

	function playAll() {
		if (items[0]) goto(`/watch/${items[0].video_id}`);
	}
</script>

<h1>Playlist</h1>
{#if loading}<p>Loading…</p>{/if}
{#if error}<p role="alert">couldn't load feed: {error}</p>{/if}
{#if !loading && !error && items.length === 0}<p>This playlist is empty.</p>{/if}
{#if items.length > 0}
	<p>{items.length} videos <button onclick={playAll}>▶ Play all</button></p>
{/if}
<VideoList {items} />
