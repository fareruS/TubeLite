<script lang="ts">
	import { page } from '$app/stores';
	import { invoke } from '@tauri-apps/api/core';
	import { goto } from '$app/navigation';
	import VideoList from '$lib/VideoList.svelte';
	import type { VideoCard } from '$lib/types';

	let id = $derived($page.params.id ?? '');
	let videos = $state<VideoCard[]>([]);
	let error = $state('');

	$effect(() => {
		invoke<{ items: VideoCard[] }>('get_playlist', { id })
			.then((page) => {
				videos = page.items;
			})
			.catch((e) => {
				error = String(e);
			});
	});

	function playAll() {
		if (videos[0]) goto(`/watch/${videos[0].id}`);
	}
</script>

<h1>Playlist</h1>
{#if error}<p role="alert">{error}</p>{/if}
{#if videos.length > 0}
	<p>{videos.length} videos <button onclick={playAll}>▶ Play all</button></p>
{/if}
<VideoList {videos} />
