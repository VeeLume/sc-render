# sc-render

Star Citizen items rendered the way the game's inventory shows them, from the user's own
`Data.p4k`. A Rust crate, a Tauri plugin and a framework-free TypeScript renderer, plus the lab
app they are developed in.

```
crates/sc-render/                GPU prep: models packed for upload, .mtl as data, DDS decoded
                                 (mips, normal / gloss packing), hue LUT, skeletons; the
                                 SceneItem contract and its builder from holotable's scene.
crates/tauri-plugin-sc-render/   Commands (load_model / load_material / load_skeleton) and the
                                 `scr` byte scheme. Capability `sc-render:default`.
packages/sc-render/              @veelume/sc-render: Cell, materials, posing, capture. The host
                                 passes a RenderSource; `./tauri` is the plugin's, `./svelte`
                                 a Viewport. Ships source, no build step.
apps/lab/                        sc-item-render: the lab app (Tauri + SvelteKit). Its catalog
                                 (DataCore items, Starlume inventory dumps, .chf colours) is
                                 apps/lab/crates/lab-catalog, not part of the library.
```

## Where things belong

The split is a Firefly decision (project `sc-item-render`, "The renderer is its own repo;
holotable keeps only game knowledge"):

- **sc-holotable**: game knowledge: format readers, item recipes, scene assembly
  (`Visuals::scene`). A reader or a rule about the game's data goes there, not here.
- **This repo**: turning that into pixels. Decoding and packing, shaders, posing, lighting, the
  cell.
- **Apps** (Starlume, the lab): presentation (`Fallback` poses), their own inventory data, icon
  caching per game build.

Nothing here may ship game data: every asset is read from the local install at runtime.

## Rules

- **`packages/sc-render/src/types.ts` is generated** from the crate's types:
  `cargo run -p sc-render --features specta --example export_types`. Rerun it after changing
  any type the package consumes.
- **The package never talks to Tauri directly** except in `src/tauri.ts`. Everything else
  reads through `RenderSource` and the `Library` cache in front of it.
- **three is pinned to one minor** (peer `>=0.186 <0.187`): the materials patch three's shader
  chunks by source line and warn when a chunk changed. Bumping three means checking those
  warnings in the lab.
- **Bumping holotable**: the four `sc-*` workspace dependencies in the root `Cargo.toml` move
  together. Compare `scene_probe` output (lab-catalog example) before and after.
- The lab's dev run renders a fixed item set and writes snapshots and logs to
  `%APPDATA%/icu.veelume.scitemrender/snapshots/`; compare them across renderer changes.

## Running it

```
just dev        # tauri dev (apps/lab)
just check      # what CI runs: fmt, clippy -D warnings, svelte-check
```

`tauri dev` watches only `apps/lab/src-tauri`: after changing `crates/` or
`apps/lab/crates/`, touch a file there to rebuild.

## Releasing

Consumers pin a tag: `github:VeeLume/sc-render#vX.Y.Z&path:packages/sc-render` for the package,
`{ git = "https://github.com/VeeLume/sc-render", tag = "vX.Y.Z" }` for the crates. Bump the
versions in `packages/sc-render/package.json` and both crate manifests together, then tag.
Never move a tag.

## Template provenance

- `.copier-answers.yml` → template-rust (justfile, git hooks, CI caller), `app_dir: apps/lab`.
- `apps/lab/.copier-answers-tauri.yml` → template-tauri (the app scaffold). It was scaffolded
  as its own repo (`workspace: true`) before moving here; the root `Cargo.toml` is now the
  hand-written workspace, so check what a `copier update` of the scaffold proposes for
  `Cargo.toml` before accepting it.

## Git

Conventional Commits, enforced by `.githooks/commit-msg`. `pre-commit` runs `cargo fmt` and
re-stages; `pre-push` runs fmt + clippy. Hooks activate via `git config core.hooksPath .githooks`.
