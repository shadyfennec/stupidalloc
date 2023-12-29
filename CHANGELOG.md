# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2023-12-29

### Added

- `logging` feature
  - Creates companion files with information about corresponding allocated memory
- `graphics` feature
  - Allows user to create graphical windows to visually display and interact with allocated memory
- `always-graphics` feature
  - Always create graphical window for every allocation
- New function to disable stupid allocation in current thread
- New example to showcase `graphics` feature

### Changed
- Inner `HashMap` is now explicitely from [`hashbrown`](https://crates.io/crate/hashbrown)
- Replaced inner map `Mutex` with `RwLock`
- Behaviour of allocation fallback corrected for thread correctness

## [0.1.0] - 2023-07-08

- `StupidAlloc` allocator
- `interactive` feature, makes users confirm allocations and de-allocation using dialog boxes

[Unreleased]: https://github.com/shadyfennec/stupidalloc/compare/v0.2.0..HEAD
[0.2.0]: https://github.com/shadyfennec/stupidalloc/compare/v0.1.0..v0.2.0
[0.1.0]: https://github.com/shadyfennec/stupidalloc/releases/tag/v0.1.0