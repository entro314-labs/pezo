# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release pipeline refuses to release a tag `vX.Y.Z` without a matching
`## [X.Y.Z]` heading below, so add one before every tag.

## [Unreleased]

### Security

- SQLite updated: rusqlite 0.40 to 0.43 brings a libsqlite3-sys without
  RUSTSEC-2026-0459.

## [0.1.0] - 2026-08-19

### Added

- Initial Pezo desktop app and `pezo-core` / `pezo-cli` workspace crates.
