// Themes via CSS variables; commit root tokens once, never per tick / per pointer move.
export type Theme = 'light' | 'dark' | 'system';
export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  const resolved = theme === 'system' ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark') : theme;
  root.dataset.theme = resolved;
}
