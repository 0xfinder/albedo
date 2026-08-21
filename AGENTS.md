# Agent Guidelines

This file provides guidance for automated agents working in this repo.

## Scope
- Keep changes minimal and aligned with existing patterns.
- Prefer simple, direct solutions over abstractions.
- Ask for clarification only when a decision would materially change behavior.

## Git and commits
- Keep commits small and focused on a single purpose.
- Use Conventional Commits (e.g., `feat: ...`, `fix: ...`, `docs: ...`).
- Avoid bundling refactors with functional changes unless required.

## Versioning and releases
- Follow Semantic Versioning (MAJOR.MINOR.PATCH):
  - MAJOR: breaking changes (e.g., DB schema migrations, config/env changes that require manual action).
  - MINOR: new features, backwards compatible.
  - PATCH: bug fixes and small tweaks only.
- While pre-1.0 (0.x.y), bump MINOR for breaking changes instead of MAJOR.
- The git tag is the single source of truth for the version. `build.rs` derives it via `git describe --tags --always --dirty`; untagged builds show `<commits>-<sha>`. Do not bump `version` in `Cargo.toml` — it is a placeholder.
- Release steps:
  1. Tag the release commit: `git tag vX.Y.Z`.
  2. Push with tags: `git push && git push --tags`.
- The version is embedded at build time (`src/main.rs` `VERSION`) and shown by `/version` and on startup.

## Code quality
- Match existing formatting and naming conventions.
- Add comments only when behavior is non-obvious.
- Update or add documentation when behavior changes.

## Tests
- Run relevant tests when changes are non-trivial.
- If tests are unavailable, note what you would run.
