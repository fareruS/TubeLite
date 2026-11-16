<script lang="ts">
	import { buildCommands, filterCommands, type Command } from '$lib/commands';
	import { applyTheme, type Theme } from '$lib/theme';
	import { goto } from '$app/navigation';
	import { matchShortcut, dispatchShortcut } from '$lib/shortcuts';

	let open = $state(false);
	let query = $state('');
	let selected = $state(0);
	let inputEl: HTMLInputElement | undefined = $state();
	let theme: Theme = $state('system');

	const commands: Command[] = buildCommands(
		(href) => goto(href),
		() => {
			theme = theme === 'dark' ? 'light' : 'dark';
			applyTheme(theme);
		}
	);
	let filtered = $derived(filterCommands(commands, query));

	function openPalette() {
		open = true;
		query = '';
		selected = 0;
		queueMicrotask(() => inputEl?.focus());
	}
	function closePalette() {
		open = false;
	}
	function run(cmd: Command) {
		closePalette();
		cmd.run();
	}

	$effect(() => {
		function onKey(e: KeyboardEvent) {
			const target = e.target as HTMLElement | null;
			const inField =
				!!target &&
				(target.tagName === 'INPUT' ||
					target.tagName === 'TEXTAREA' ||
					target.tagName === 'SELECT' ||
					target.isContentEditable);
			if (open) {
				if (e.key === 'Escape') closePalette();
				else if (e.key === 'ArrowDown') {
					e.preventDefault();
					selected = Math.min(selected + 1, filtered.length - 1);
				} else if (e.key === 'ArrowUp') {
					e.preventDefault();
					selected = Math.max(selected - 1, 0);
				} else if (e.key === 'Enter' && filtered[selected]) {
					run(filtered[selected]);
				}
				return;
			}
			const m = matchShortcut({
				key: e.key,
				ctrlKey: e.ctrlKey,
				metaKey: e.metaKey,
				shiftKey: e.shiftKey,
				inField
			});
			if (!m) return;
			if (m.action === 'open-palette' || m.action === 'show-help') {
				e.preventDefault();
				openPalette();
				return;
			}
			// Player shortcuts: only meaningful on the Watch page, but
			// dispatching them elsewhere is harmless — nobody listens.
			if (e.key === ' ') e.preventDefault(); // keep Space from scrolling
			dispatchShortcut(m);
		}
		window.addEventListener('keydown', onKey);
		return () => window.removeEventListener('keydown', onKey);
	});
</script>

{#if open}
	<div class="palette-backdrop" role="presentation" onclick={closePalette}></div>
	<div class="palette" role="dialog" aria-label="Command palette">
		<input
			bind:this={inputEl}
			bind:value={query}
			type="text"
			placeholder="Type a command or search…"
			aria-label="Command search"
			oninput={() => (selected = 0)}
		/>
		<ul>
			{#each filtered as cmd, i (cmd.id)}
				<li>
					<button
						class:selected={i === selected}
						onclick={() => run(cmd)}
						onmouseenter={() => (selected = i)}
					>
						<span>{cmd.label}</span>
						{#if cmd.hint}<kbd>{cmd.hint}</kbd>{/if}
					</button>
				</li>
			{/each}
			{#if filtered.length === 0}
				<li class="empty">No matching command.</li>
			{/if}
		</ul>
		<p class="foot">↑↓ navigate · Enter run · Esc close · Ctrl/⌘ K anywhere</p>
	</div>
{/if}

<style>
	.palette-backdrop {
		position: fixed;
		inset: 0;
		background: rgb(0 0 0 / 0.4);
		z-index: 50;
	}
	.palette {
		position: fixed;
		top: 15vh;
		left: 50%;
		transform: translateX(-50%);
		width: min(560px, 90vw);
		background: var(--bg);
		color: var(--fg);
		border: 1px solid var(--border);
		border-radius: 10px;
		z-index: 51;
		overflow: hidden;
	}
	.palette input {
		width: 100%;
		padding: 0.8rem 1rem;
		border: none;
		border-bottom: 1px solid var(--border);
		background: transparent;
		color: inherit;
		font: inherit;
		outline: none;
	}
	.palette ul {
		list-style: none;
		margin: 0;
		padding: 0.25rem;
		max-height: 40vh;
		overflow-y: auto;
	}
	.palette button {
		width: 100%;
		display: flex;
		justify-content: space-between;
		padding: 0.5rem 0.75rem;
		border: none;
		border-radius: 6px;
		background: transparent;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.palette button.selected {
		background: var(--accent-soft);
	}
	.palette .empty {
		padding: 0.75rem;
		opacity: 0.7;
	}
	.palette .foot {
		margin: 0;
		padding: 0.5rem 0.75rem;
		border-top: 1px solid var(--border);
		font-size: 0.8rem;
		opacity: 0.7;
	}
	.palette kbd {
		border: 1px solid var(--border);
		border-radius: 4px;
		padding: 0 0.35rem;
		font-size: 0.8rem;
	}
</style>
