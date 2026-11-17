<script lang="ts">
	import { getSubscriptions } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { FeedItem } from '$lib/types';

	let items = $state<FeedItem[]>([]);
	let error = $state('');
	let loading = $state(false);

	$effect(() => {
		loading = true;
		error = '';
		getSubscriptions()
			.then((page) => {
				items = page.items;
			})
			.catch((e) => {
				error = String(e);
			})
			.finally(() => {
				loading = false;
			});
	});
</script>

<h1>Subscriptions</h1>
{#if loading}<p>Loading…</p>{/if}
{#if error}<p role="alert">couldn't load feed: {error}</p>{/if}
{#if !loading && !error && items.length === 0}
	<p>No subscriptions yet — open a channel and hit Subscribe.</p>
{/if}
<VideoList {items} />
