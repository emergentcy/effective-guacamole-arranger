# Review: Concerns, Fixes, and Next Steps for effective-guacamole-arranger

This document captures the full set of concerns, suggested improvements, and prioritized next steps discovered while reviewing the repository. Its purpose is to provide clear, actionable context for the next agent or contributor who will continue development.

---

## Quick summary

The project is well scoped: a small Rust CLI plus a lightweight web UI to scan a Johnny.Decimal (JD) folder tree, visualize a 3×10 physical layout, allow editing, and export a script to apply changes. The implementation mostly follows the intended flow, but there are a few critical correctness bugs, cross-platform issues, and missing validations that should be addressed before this tool is used on non-trivial archives.

This file documents those issues and provides prioritized remediation steps, testing suggestions, and small implementation patterns to follow.

---

## High-level gaps vs. docs/ruleset

- The rules in `docs/johnny-decimal-rules.md` (the JD ruleset) are documented but not enforced programmatically. The codebase currently lacks a validator that checks the invariants (exactly 3 Areas, reserved X0 slots, max 10 categories/IDs, ID -> parent Category consistency). The TODO in README should become first-class validation code executed after scanning and before serving/exporting.

---

## Critical correctness issues (must fix before wide usage)

1. Path and Area detection (highest priority)
   - Problem: code relies on naive string splitting (e.g. `split('/')` and indexing) to determine Area or folder components. This is brittle on Windows (backslash separators), when the scan root is absolute, and in cases with unexpected directory depth. It leads to wrong Area assignment and broken mapping.
   - Fix: use `std::path::Path` API (components(), ancestors(), file_name()) and determine Area by either:
     - finding the nearest ancestor whose directory name matches the Area regex (e.g. `^\d{2}-\d{2}\s+.*`), or
     - deterministically using the first directory under the scan root as the Area (if the repository layout guarantees that structure).
   - Add unit tests that cover Unix paths, Windows paths, absolute/relative roots, and edge cases.

2. Serve command ignores provided `--state` argument
   - Problem: `serve` CLI has a state arg but the server reads a hardcoded `state.json`. This makes `--state other.json` a no-op.
   - Fix: pass the PathBuf into the Axum state or closure so request handlers read the configured path. Add tests or a runtime check that `--state` is respected.

3. Unsafe assumptions / many unwraps causing panics
   - Problem: `unwrap()` is used across code for Regex compilation, file IO, binding the server, etc.
   - Fix: return Result from functions or use proper error handling (anyhow, thiserror, or map_err + meaningful messages). The CLI should exit cleanly with a non-zero code and user-facing error messages rather than panicking.

4. Shell script generation: escaping and platform-safety
   - Problem: export generates a bash `mv` script. Filenames can contain quotes, backticks, newlines, or other characters that break naive quoting. Also Windows users cannot run the script.
   - Fix options (choose one or more):
     - Produce a JSON manifest (old_path -> new_path) as the canonical export and optionally a shell script. Recommend manifest as default export.
     - If producing shell scripts, implement robust shell-escaping. Consider using shlex-style escaping or a Rust crate for shell escaping.
     - Provide a Windows PowerShell export or a platform-agnostic `apply` mode that uses std::fs::rename (executed by the binary) behind a `--apply` flag that requires an explicit confirmation.

5. Physical location and ID mapping edge cases
   - Problem: mapping numeric JD ranges to physical slots works for valid inputs but fails silently for malformed values.
   - Fix: add defensive validation and explicit errors when parse fails, and log what was found with context.

---

## Cross-platform and UX concerns

- Path handling should always use Path/OsStr/OsString for logic; only convert to UTF-8 strings for display when necessary. Avoid logic that depends on `to_string_lossy()`.
- The exported shell script is Unix-specific. Consider offering multiple export formats (JSON manifest, POSIX shell script, PowerShell script) or an internal "sync" mode that executes file moves safely.
- The UI currently embeds state in rendered HTML; prefer a small REST endpoint (GET /api/state) so the UI fetches JSON. This avoids needing to regenerate HTML on state changes and makes saving/persisting easier.

---

## Validation rules to implement (derived from docs/johnny-decimal-rules.md)

Implement a validation module that checks the following invariants and returns a structured list of violations:

1. There are exactly 3 Areas (IDs 10-19, 20-29, 30-39 mapped to the three shelves). If fewer, mark which are missing; if more, list extras.
2. Slot X0 (the first slot of each shelf: physical slots 1, 11, 21) is reserved for Management. Ensure a Management category exists at each X0 or report that the slot is unoccupied.
3. Each Area must not have more than 10 slots used (management + 9 categories). Report overflow.
4. Category naming: categories must be named `XX Title` where XX is derived by the slot. Flag mismatches.
5. ID ownership: every ID `XX.YY` must be located within the category folder `XX Title`.
6. Maximum 10 IDs per Category (01..10 or 00..09 depending on convention) — enforce whichever convention the docs define; clarify convention in README/rules.

Expose the validator in two places:
- `scan` CLI: fail fast or print warnings and generate a `state.json` with violations included.
- `serve` UI: show violations in the UI with clear instructions.

---

## Tests, CI, and maintainability

- Add unit tests for:
  - Name parsing (Area, Category, ID) with many permutations.
  - Physical location mapping (10..39 -> 1..30) and invalid numbers.
  - Validator rules (create small temp directories and assert violations/non-violations).
- Add an integration test that runs `scan` on a temporary sample JD tree and asserts the produced `JdIndex` contains expected items.
- Add a GitHub Action workflow:
  - cargo fmt -- --check
  - cargo clippy -- -D warnings (optional)
  - cargo test

---

## Security / safety notes

- Never execute generated scripts automatically. The UX should require either the user to run the script manually or enable an explicit `--apply` option that will perform atomic, logged renames.
- When offering `--apply`, provide a dry-run mode and confirm prompts. Consider backups or a revert plan.

---

## API / Data format improvements

- Make `state.json` schema explicit (document fields). Consider versioning the schema (e.g. `state_version: 1`) so future changes can migrate easily.
- Export a JSON manifest for renames (array of { old_path, new_path, reason }) in addition to or instead of a shell script.
- Provide endpoints:
  - GET /api/state -> returns JdIndex
  - POST /api/state -> save state.json (requires authentication? local-only binding OK)
  - POST /api/export -> returns JSON manifest or script

---

## Suggested immediate code changes (minimal diffs)

1. Path handling: replace any `split('/')` usage with a function like `fn find_area_ancestor(path: &Path, root: &Path, re_area: &Regex) -> Option<String>` that iterates `ancestors()` and returns the first ancestor whose `file_name()` matches the area pattern.

2. Serve CLI: change server init to capture the `state: PathBuf` into an Arc and pass to handlers via Axum's State extractor. Update render_ui and API handlers to read that PathBuf.

3. Generate manifest: add `fn generate_manifest(items: &[RenameItem]) -> serde_json::Value` and make `export` accept `format=(json|sh|ps1)`.

4. Error handling: replace top-level unwraps in `main()` with `match` or `?` and map errors to user-friendly messages. Consider returning `Result<(), anyhow::Error>` from main with `#[tokio::main]`.

---

## Priority action list (1..8)

1. Fix path/area detection logic (use Path APIs). Add unit tests.
2. Make `serve` respect the `--state` argument and add /api/state.
3. Implement the validator described above and surface violations in CLI and UI.
4. Harden export flow: manifest + robust shell escaping + platform-specific exports.
5. Reduce `unwrap()` usage and add proper error handling.
6. Add tests for parsing and mapping; add GitHub Actions for CI.
7. Document `state.json` schema and add a `version` field.
8. Finalize `docs/johnny-decimal-rules.md` (it was truncated) and convert rules to machine-checkable invariants used by validator.

---

## Recommended PR checklist for each change

- [ ] Add unit tests that cover the new behavior.
- [ ] Add integration test for `scan` producing a valid `state.json` on sample dataset.
- [ ] Update README / docs to mention platform specifics for export formats.
- [ ] Run `cargo fmt` and `cargo clippy`.
- [ ] Add or update CHANGELOG with the behavioral changes (particularly around export formats and `serve --state`).

---

## Small style & repo housekeeping notes

- Remove the duplicate `typescript` entry in .gitignore.
- Keep static UI assets in a `static/` folder and serve them via `include_str!` or an asset bundler for easier iteration.
- Consider moving complex JS into a minimal bundle under `static/` and reference via `index.html` served by the binary.

---

If you want, I can implement the two smallest, highest-value changes in a follow-up commit:
1. Fix the path/area detection function and associated unit tests.
2. Make `serve` accept and use the `--state` PathBuf (and add a GET /api/state endpoint).

I have committed this document to `docs/agent-review-concerns.md` so the repository now contains a single canonical summary that the next agent or contributor can use.
