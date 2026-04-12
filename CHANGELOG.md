# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [0.1.0] - 2026-04-12

Initial release.

### Added

- FUSE filesystem projecting GitHub repositories, issues, pull requests, actions, and source files as local files.
- Wasm-based provider architecture using the WIT Component Model (wasmtime).
- Effect-based runtime: providers return effect descriptions, host interprets and executes them.
- Git-backed reconciliation via custom remote helper.
- GitHub provider with full read-only projection of repos, issues, PRs, actions, and source trees.
- Per-provider capability declarations and runtime enforcement.
- TOML-based provider configuration.
