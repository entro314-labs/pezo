# Pezo

Offline game library manager for Linux. Point it at a disk, drop in your GOG
offline installers, abandonware folders and archives, and Pezo scans,
classifies, installs and launches everything — Proton for Windows games (via
umu-launcher), DOSBox for DOS-era titles, ScummVM for adventure-engine
classics.

Pezo does not reimplement Wine, extraction or emulation. It orchestrates
proven tools and keeps the bookkeeping:

| Item found                       | Install strategy                          | Runner  |
| -------------------------------- | ----------------------------------------- | ------- |
| GOG / Inno Setup installer       | unpacked with `innoextract` (no wine)     | Proton  |
| Other Windows installer (.exe/.msi) | run interactively in a fresh prefix via `umu-run` | Proton  |
| Portable folder / bare exe       | registered in place                       | Proton  |
| Folder with `dosbox.conf`        | registered in place                       | DOSBox  |
| Folder with ScummVM markers      | registered in place                       | ScummVM |
| Archive (zip/7z/tar/…)           | extracted with `bsdtar`, then re-classified | —     |

## Library layout

Everything lives on one disk (use ext4 — Proton misbehaves on NTFS), so the
library survives distro hops and machine moves:

```
/mnt/games/
├── installers/   # drop your GOG setup exes, folders and archives here
├── library/      # installed games
├── prefixes/     # wine prefixes, one per game
└── pezo.db       # the catalog (SQLite)
```

## Structure

```
crates/pezo-core/   # scan, classify, install, launch, db — the brain
crates/pezo-cli/    # `pezo` CLI over the same core
src-tauri/          # Tauri 2 desktop app backend
src/                # React 19 + Vite 8 + Tailwind 4 frontend
```

## Runtime dependencies (Linux)

- [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) — Proton outside Steam (required for Windows games)
- `innoextract` — unpack GOG installers (required for GOG installers)
- `bsdtar` (libarchive) — archives
- `dosbox-staging`, `scummvm` — optional runners

`pezo doctor` (or the Setup tab in the app) reports what's missing with
per-distro install hints.

## Development

```sh
mise install            # node 24 + rust 1.97
pnpm install
pnpm tauri dev          # desktop app (vite dev server + tauri)
cargo run -p pezo-cli -- --help
cargo test --workspace  # core unit tests
pnpm build              # typecheck (tsc / TS7) + vite production build
pnpm lint               # oxlint
```

## CLI

```sh
pezo init /mnt/games    # set library root, create layout
pezo doctor             # check external tools
pezo scan               # classify everything in installers/
pezo candidates         # list what was found
pezo install 4          # install candidate #4
pezo games              # list installed games
pezo launch broken-sword  # blocks until exit, records playtime
```

The desktop app and the CLI share the same core and the same database — use
whichever fits.
