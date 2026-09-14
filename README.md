<img width="1916" height="1667" alt="Screenshot 2026-09-14 000709" src="https://github.com/user-attachments/assets/3b266640-2b4e-426b-b5d3-0b665094dd67" />
# RE2HD Builder

Self-contained builder for the classic PC edition: feed it the disc image, it applies the community HD texture and audio mod archives, and assembles a ready-to-run build folder. The packaged app embeds everything (7-Zip, all mod archives, menu music) into a single portable executable. No searching for files, no external tools needed.

## Features

- Drag & drop the disc image; mods auto-find next to it or are read straight from the embedded payloads
- 9-step pipeline with live per-step progress (yellow = working, green = done)
- One-file distribution: payloads appended to the exe, extracted to `%TEMP%\re2hd_embedded` on first run
- Built-in chiptune menu player (MOD / XM / S3M / IT), shuffle start, volume, mute and skip
- Retro terminal styling with a startup jingle on completion

## Config reminder (important)

In the classic REbirth configuration menu, **do not enable texture filtering** with these HD textures. Leave texture filtering / smoothing **off**, otherwise the HD texture packs render blurred instead of crisp.

## Build

```sh
cargo build --release
```

## Pack into a single-file exe

```sh
cargo run --release --bin pack -- <input-exe> <output-exe> [music-dir]
```

Example:

```sh
cargo run --release --bin pack -- target\release\re2hd-builder.exe re2hd-builder-packed.exe "C:\Somewhere\tracker-music"
```

The pack tool embeds `tools/7z.exe`, `tools/7z.dll`, the six mod archives, the music archive and every tracker file in `music-dir`.

## Repository layout

- `src/`: app (GUI), build pipeline, archive layer, embedded-payload engine, tracker music player
- `src/bin/`: `pack` (embed payloads into an exe), `headless` (no-UI end-to-end run)
- `assets/`: window/exe icon, retro font, Windows resource script
- `tools/`: 7-Zip binaries used by the (re)pack step

## Notes

Third-party mods and 7-Zip are the property of their respective authors. This tool is a convenience wrapper that assembles them.
