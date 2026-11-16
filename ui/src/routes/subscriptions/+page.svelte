<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';

	interface Subscription {
		channel_id: string;
		title: string;
		added_at: number;
	}

	let subs = $state<Subscription[]>([]);
	let error = $state('');

	function load() {
		invoke<Subscription[]>('list_subscriptions')
			.then((s) => {
				subs = s;
			})
			.catch((e) => {
				error = String(e);
			});
	}

	$effect(load);

	async function unsub(channelId: string) {
		await invoke('unsubscribe', { channelId });
		load();
	}
</script>

<h1>Subscriptions</h1>
{#if error}<p role="alert">{error}</p>{/if}
{#if subs.length === 0 && !error}
	<p>No subscriptions yet — open a channel and hit Subscribe.</p>
{:else}
	<ul>
		{#each subs as s (s.channel_id)}
			<li>
				<a href={`/channel/${s.channel_id}`}>{s.title}</a>
				<button onclick={() => unsub(s.channel_id)}>Unsubscribe</button>
			</li>
		{/each}
	</ul>
{/if}
