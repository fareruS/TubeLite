<script lang="ts">
	import { page } from '$app/stores';
	import { searchVideos } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { VideoCard } from '$lib/types';

	let q = $derived($page.url.searchParams.get('q') ?? '');
	let videos = $state<VideoCard[]>([]);
	let error = $state('');
	let loading = $state(false);

	$effect(() => {
		if (!q) {
			videos = [];
			return;
		}
		loading = true;
		error = '';
		searchVideos(q)
			.then((page) => {
				videos = page.items;
			})
			.catch((e) => {
				error = String(e);
			})
			.finally(() => {
				loading = false;
			});
	});
</script>

<h1>Search: {q}</h1>
{#if loading}<p>Loading…</p>{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#if !loading && !error && videos.length === 0 && q}<p>No results for “{q}”.</p>{/if}
<VideoList {videos} />
