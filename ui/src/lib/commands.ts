// Command palette commands — plain data, no DOM.
// Each command has an id, a label, an optional shortcut hint, and a
// `run` that the app wires up (navigation, theme, player toggles).
// Filtering is a pure function so the palette is testable without Svelte.

export interface Command {
	id: string;
	label: string;
	hint?: string;
	group: string;
	run: () => void;
}

export function buildCommands(nav: (href: string) => void, toggleTheme: () => void): Command[] {
	return [
		{ id: 'go-home', label: 'Go to Home', hint: 'g h', group: 'Navigate', run: () => nav('/') },
		{ id: 'go-search', label: 'Go to Search', hint: 'g s', group: 'Navigate', run: () => nav('/search') },
		{
			id: 'go-subscriptions',
			label: 'Go to Subscriptions',
			hint: 'g u',
			group: 'Navigate',
			run: () => nav('/subscriptions')
		},
		{
			id: 'go-library',
			label: 'Go to Library',
			hint: 'g l',
			group: 'Navigate',
			run: () => nav('/library')
		},
		{
			id: 'go-settings',
			label: 'Go to Settings',
			hint: 'g ,',
			group: 'Navigate',
			run: () => nav('/settings')
		},
		{
			id: 'toggle-theme',
			label: 'Toggle light / dark theme',
			group: 'Appearance',
			run: toggleTheme
		},
		{
			id: 'toggle-audio-only',
			label: 'Toggle audio-only mode',
			hint: 'a',
			group: 'Playback',
			run: () => {
				if (typeof window !== 'undefined') {
					window.dispatchEvent(new CustomEvent('tubelite:toggle-audio-only'));
				}
			}
		}
	];
}

export function filterCommands(commands: Command[], query: string): Command[] {
	const q = query.trim().toLowerCase();
	if (!q) return commands;
	return commands.filter(
		(c) => c.label.toLowerCase().includes(q) || c.group.toLowerCase().includes(q)
	);
}
