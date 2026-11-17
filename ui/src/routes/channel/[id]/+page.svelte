<script lang="ts">
	import { page } from '$app/stores';
	import { invoke } from '@tauri-apps/api/core';
	import { getChannelVideos } from '$lib/api';
	import VideoList from '$lib/VideoList.svelte';
	import type { ChannelHeader, FeedItem } from '$lib/types';

	let id = $derived($page.params.id ?? '');
	let items = $state<FeedItem[]>([]);
	let channel = $state<ChannelHeader | null>(null);
	let error = $state('');
	let loading = $state(false);
	let subscribed = $state(false);

	// The real header when the response carries one; the first
	// video's channel name is only the fallback (the Videos-tab
	// response may not carry a header).
	let channelName = $derived(channel?.title || items[0]?.channel_name || id);
	let avatarUrl = $derived(channel?.thumbnails[0]?.url ?? '');

	$effect(() => {
		if (!id) return;
		loading = true;
		error = '';
		channel = null;
		getChannelVideos(id)
			.then((p) => {
				items = p.items;
				channel = p.channel ?? null;
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
	{#if avatarUrl}<img src={avatarUrl} alt="" width="88" height="88" />{/if}
	<h1>{channelName}</h1>
	{#if channel?.handle}<p>{channel.handle}</p>{/if}
	{#if channel?.subscriber_count_text}<p>{channel.subscriber_count_text}</p>{/if}
	{#if channel?.description}<p style="white-space: pre-line">{channel.description}</p>{/if}
	<p><button onclick={toggleSub}>{subscribed ? 'Subscribed ✓' : 'Subscribe'}</button></p>
	{#if items.length === 0}<p>No videos from this channel yet.</p>{/if}
{/if}
<VideoList {items} />
