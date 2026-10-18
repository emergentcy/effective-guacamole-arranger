# effective-guacamole-arranger

A Rust-based CLI and visual web interface for managing, visually arranging, and synchronizing a Johnny.Decimal physical library.

## Overview

This tool bridges the gap between a logical Johnny.Decimal filesystem and your physical storage (e.g., a 3-shelf bookcase). It acts as a Virtual Filesystem Browser:
1. It scans your existing JD folder structure.
2. It generates a visual 3x10 grid representing your physical shelves.
3. You can drag/drop items to swap them, which automatically recalculates their JD numbers.
4. It exports a shell script to rename/move the actual files on your disk to match the visual layout.

## Tech Stack

* **Rust**: Core logic, CLI (Clap), and local web server (Axum).
* **Serde**: JSON serialization for the `state.json` index.
* **WalkDir**: Recursive filesystem scanning.
* **HTML/CSS/JS**: Lightweight, vanilla frontend served directly from the Rust binary.

## Current Features

* **Strict Parsing**: Separates folders into Areas, Categories, IDs, and Files.
* **System Drawer**: `00-09` folders are pulled out into a special management drawer.
* **Locked Slots**: Row 1 of every shelf is locked for the `XX0` Management category.
* **Dynamic IDs**: Swapping Categories visually updates their logical `XX` prefix.
* **Drill-down Modal**: Tap any folder to open a modal and browse its subfolders and files (read-only for files).
* **Export**: Generates a bash script to rename and relocate folders based on the physical UI layout.

## TODOs for Next Session

- [ ] **Ruleset Validation**: Implement the logic defined in `docs/johnny-decimal-rules.md` to prevent users from breaking the JD structure.
- [ ] **State Saving**: Currently, the UI state is held in memory and only exported via script. We need an API endpoint to save `state.json` directly from the UI.
- [ ] **Sync Mode**: Instead of just generating a script, directly execute the `mv` commands from the Rust server.
- [ ] **UI Polish**: Refine the CSS for mobile view, fix the weird `typescript` binary artifact issue, and add a "Back" button in the drill-down modal.

## Usage

1. Scan your library to generate the index:
   `cargo run -- scan /path/to/your/jd_LastHopes`

2. Boot the visual web interface:
   `cargo run -- serve`

3. Open `http://localhost:3001` in your browser.
