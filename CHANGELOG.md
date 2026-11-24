# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release pipeline refuses to release a tag `vX.Y.Z` without a matching
`## [X.Y.Z]` heading below, so add one before every tag.

## [Unreleased]

### Added

- Wine & Proton build management (Settings → Wine & Proton, `pezo wine …`):
  detects builds in Steam's `compatibilitytools.d` folders, Steam's own
  Proton, umu, Lutris and the system `wine`; downloads GE-Proton, UMU-Proton,
  Proton-CachyOS and Kron4ek Wine / Wine Staging-TkG releases with checksum
  verification; removes builds Pezo is allowed to delete.
- A default build for all Windows games plus a per-game override under
  Launch options. Proton builds run through umu-run (`PROTONPATH`), Wine
  builds through their own `bin/wine`. Windows installers and winetricks use
  the same build.
- GOG installs merge DLC setups shipped alongside the base installer and
  install the Visual C++, DirectX, OpenAL and Unreal runtimes from
  `__redist` silently into the game's prefix. The launch target and its
  arguments come from GOG's `goggame-*.info` play task.
- Repacks (FitGirl, xatab) are recognised and run their own installer in the
  game's prefix; setup.exe/MSI/NSIS installers run unattended into
  `C:\Games\<slug>` unless "Show installer windows" is enabled
  (`pezo install --interactive`).
- `pezo install` fetches cover art and writes a desktop launcher, like the app.
- Statically recompiled PC ports (PrimeRecomp, LAS Recomp, Savage Wind,
  SwitchRecomp builds, rexglue/XenonRecomp, OpenHuey) install from their
  archives or extracted folders: the game exe is picked over the zig, GCC,
  clang and Python toolchains they bundle. A builder package whose game exe
  doesn't exist yet (UltraTears before its `setup.bat` has run) is refused
  with a hint instead of registering a build tool as the game.

### Changed

- The macOS app is signed with a Developer ID certificate and notarized by Apple, so it opens without the "unidentified developer" warning.

### Fixed

- GOG installs failed immediately: innoextract was called with a
  nonexistent `--dir` option, and modern GOG installers' game files were
  looked for in their stub `app/` folder.
- FitGirl/xatab repacks were unpacked with innoextract, which only yields
  the installer's temp files; older FitGirl `setup-multi*.exe` repacks were
  registered as the game itself.
- Titles lost their numbers ("Fable 3" → "Fable", "Resident Evil 2 + 3" →
  "Resident Evil +"); version strings, build ids, bracketed tags and
  release-group names are still removed, and existing capitalisation is kept.
- Windows games start in their exe's folder, like a Windows shortcut.

### Security

- SQLite updated: rusqlite 0.40 to 0.43 brings a libsqlite3-sys without
  RUSTSEC-2026-0459.

## [0.1.0] - 2026-08-19

### Added

- Initial Pezo desktop app and `pezo-core` / `pezo-cli` workspace crates.
