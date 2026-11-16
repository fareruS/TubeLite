# THIRD_PARTY.md

Dependency licenses. Complete the exact resolved versions from Cargo.lock /
package-lock.json at each release. Project license: TBD (owner choice).

| Dependency | License |
|---|---|
| tauri 2.11.1 | MIT / Apache-2.0 |
| libmpv2 6.0.0 | LGPL-2.1 (links libmpv; note LGPL obligations for distribution) |
| reqwest 0.12 (rustls) | MIT / Apache-2.0 |
| tokio 1.x | MIT |
| rusqlite 0.32 (bundled SQLite) | MIT |
| serde / serde_json | MIT / Apache-2.0 |
| Svelte 5 / SvelteKit 2 | MIT |
| Tailwind CSS 4 | MIT |
| @tauri-apps/api 2.11.1 | MIT / Apache-2.0 |

Note: Limusic is GPL-3.0. This repo copies no Limusic code; it reimplements
from architectural ideas only. If the owner chooses GPL-3.0, that changes
nothing about the code, but the LICENSE file must then be GPL-3.0.
