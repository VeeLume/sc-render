# Icons

**These are placeholders.** A grey ring on a dark rounded square, shipped only
so `tauri dev` and `tauri build` work on the first run — `tauri-build` refuses to
compile without `icon.ico`.

Replace them before anything ships:

```sh
pnpm tauri icon path/to/source.png     # 1024×1024 PNG with transparency
```

That regenerates `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`,
`icon.ico`, the Windows Store logos, and the Android mipmaps. Only the five
files listed in `tauri.conf.json` are committed by the template; `tauri icon`
writes the rest next to them.

A placeholder icon surviving to release is the single most common template
leftover — it's the first thing users see.
