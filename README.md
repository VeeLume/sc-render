# sc-render

Renders Star Citizen items the way the game's inventory shows them: its pose, its light rig,
its materials. Everything is read from your own `Data.p4k` at runtime; no game data ships with
this repo or with apps built on it.

- **`crates/sc-render`**: reads models, materials, skeletons and textures from the local install
  (through [sc-holotable](https://github.com/VeeLume/sc-holotable)) and prepares them for a GPU
  renderer.
- **`crates/tauri-plugin-sc-render`**: serves them to a Tauri webview.
- **`packages/sc-render`** (`@veelume/sc-render`): the three.js renderer. Hand it a
  `RenderSource` and a `SceneItem`; it draws one inventory cell, interactively or as an icon.
- **`apps/lab`**: the app the renderer is developed and checked in.

```ts
import { Cell } from '@veelume/sc-render';
import { tauriSource } from '@veelume/sc-render/tauri';

const cell = new Cell(canvas, tauriSource());
cell.resize(canvas.clientWidth, canvas.clientHeight);
const log = await cell.show(sceneItem, { lightScale: 2, ambient: 0.15 });
const icon = cell.capture(112); // the cell as its inventory slot shows it
```

## Development

```sh
pnpm install
just dev
```
