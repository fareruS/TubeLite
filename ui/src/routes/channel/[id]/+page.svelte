<script lang="ts">
	import { page } from '$app/stores';
	import { invoke } from '@tauri-apps/api/core';
	import { getChannelVideos } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { FeedItem } from '$lib/types';

	let id = $derived($page.params.id ?? '');
	let items = $state<FeedItem[]>([]);
	let error = $state('');
	let loading = $state(false);
	let subscribed = $state(false);

	let channelName = $derived(items[0]?.channel_name ?? id);

	$effect(() => {
		if (!id) return;
		loading = true;
		error = '';
		getChannelVideos(id)
			.then((p) => {
				items = p.items;
			})
			.catch((e) => {
				error = String(e);
			})
			.finally(() => {
				loading = false;
			});
		invoke<{ channel_id: string }[]>('list_subscriptions')
			.then((subs) => {
				subscribed = subs.some((s) => s.channel_id === id);
			})
			.catch(() => {});
	});

	async function toggleSub() {
		try {
			if (subscribed) {
				await invoke('unsubscribe', { channelId: id });
				subscribed = false;
			} else {
				await invoke('subscribe', { channelId: id, title: channelName });
				subscribed = true;
			}
		} catch (e) {
			error = String(e);
		}
	}
</script>

{#if loading}<p>Loading…</p>{/if}
{#if error}<p role="alert">couldn't load feed: {error}</p>{/if}
{#if !loading && !error}
	<h1>{channelName}</h1>
	<p><button onclick={toggleSub}>{subscribed ? 'Subscribed ✓' : 'Subscribe'}</button></p>
	{#if items.length === 0}<p>No videos from this channel yet.</p>{/if}
{/if}
<VideoList {items} />
