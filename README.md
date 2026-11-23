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
| GOG / Inno Setup installer       | unpacked with `innoextract` (no wine); DLC setups alongside merged in, bundled VC++/DirectX/OpenAL runtimes installed silently into the prefix, launch target from GOG's `goggame-*.info` | Wine/Proton |
| Repack (FitGirl, xatab…)         | its own installer runs in a fresh prefix with the installer window shown — tick "Limit installer to 2 GB of RAM" | Wine/Proton |
| Other Windows installer (.exe/.msi) | run in a fresh prefix into `C:\Games\<slug>`, unattended unless "Show installer windows" is on | Wine/Proton |
| Portable folder / bare exe       | registered in place                       | Wine/Proton |
| Folder with `dosbox.conf`        | registered in place                       | DOSBox  |
| Folder with ScummVM markers      | registered in place                       | ScummVM |
| Archive (zip/7z/tar/…)           | extracted with `bsdtar`, then re-classified | —     |

## Wine & Proton

Windows games run with a Wine or Proton *build*. With nothing picked, umu-run
uses its own default (latest UMU-Proton, downloaded on first launch). Pick a
default for all games in Settings → Wine & Proton, or override it per game
under Launch options.

- **Detected:** `~/.local/share/Steam/compatibilitytools.d` (and the
  `~/.steam/root` / Flatpak Steam equivalents), `/usr/share/steam/compatibilitytools.d`,
  Steam's own Proton in `steamapps/common`, umu's downloads, Lutris Wine
  runners, and the `wine` on PATH (`system`).
- **Downloadable:** GE-Proton, UMU-Proton, Proton-CachyOS, Wine and Wine
  Staging-TkG (Kron4ek WoW64 builds). Downloads are checked against the
  release's published sha256/sha512 before unpacking. Proton lands in
  `$XDG_DATA_HOME/Steam/compatibilitytools.d` (default
  `~/.local/share/…` — shared with Steam, umu and ProtonUp-Qt), Wine in
  `$XDG_DATA_HOME/pezo/wine`.
- **How they run:** Proton through `umu-run` with `PROTONPATH` set; Wine
  through the build's `bin/wine`. Both use the game's prefix, so switching a
  game between them keeps its saves and settings.

Games store the build *name*, so a library moved to another machine shows a
"not installed" error with an install hint until that build is installed there.

## Library layout

Everything lives on one disk (use ext4 — Proton misbehaves on NTFS), so the
library survives distro hops and machine moves:

```tree
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

- [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) — Proton outside Steam (required for Windows games unless every game uses a Wine build)
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
pezo install 4          # install candidate #4 (+ cover and desktop launcher)
pezo install 4 --interactive  # show a setup.exe's own installer window
pezo games              # list installed games
pezo launch broken-sword  # blocks until exit, records playtime

pezo wine list                       # installed Wine/Proton builds (* = default)
pezo wine available                  # downloadable sources
pezo wine available ge-proton        # releases of one source
pezo wine install ge-proton          # latest release (or pass a tag)
pezo wine default GE-Proton11-7      # default for all games (`umu` resets)
pezo wine use broken-sword wine-11.19-amd64-wow64   # per game (`default` resets)
pezo wine remove GE-Proton11-6
```

The desktop app and the CLI share the same core and the same database — use
whichever fits.
