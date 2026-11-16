<script lang="ts">
	import { page } from '$app/stores';
	import { invoke } from '@tauri-apps/api/core';
	import VideoList from '$lib/VideoList.svelte';
	import type { VideoCard } from '$lib/types';

	interface ChannelPage {
		id: string;
		title: string;
		description: string;
		thumbnails: { url: string; width: number; height: number }[];
		subscriber_count: number | null;
		videos: VideoCard[];
	}

	let id = $derived($page.params.id ?? '');
	let channel = $state<ChannelPage | null>(null);
	let error = $state('');
	let subscribed = $state(false);

	$effect(() => {
		invoke<ChannelPage>('get_channel', { id })
			.then((c) => {
				channel = c;
			})
			.catch((e) => {
				error = String(e);
			});
		invoke<{ channel_id: string }[]>('list_subscriptions')
			.then((subs) => {
				subscribed = subs.some((s) => s.channel_id === id);
			})
			.catch(() => {});
	});

	async function toggleSub() {
		if (!channel) return;
		try {
			if (subscribed) {
				await invoke('unsubscribe', { channelId: id });
				subscribed = false;
			} else {
				await invoke('subscribe', { channelId: id, title: channel.title });
				subscribed = true;
			}
		} catch (e) {
			error = String(e);
		}
	}
</script>

{#if error}<p role="alert">{error}</p>{/if}
{#if channel}
	{#if channel.thumbnails[0]}
		<img src={channel.thumbnails[0].url} alt="" width="88" height="88" />
	{/if}
	<h1>{channel.title}</h1>
	<p>
		{#if channel.subscriber_count != null}{channel.subscriber_count.toLocaleString()} subscribers · {/if}
		<button onclick={toggleSub}>{subscribed ? 'Subscribed ✓' : 'Subscribe'}</button>
	</p>
	<p>{channel.description}</p>
	<VideoList videos={channel.videos} />
{:else if !error}
	<p>Loading…</p>
{/if}
