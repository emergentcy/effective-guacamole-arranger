// Centralized configuration and constants to avoid magic numbers/strings across the codebase.
// Keep this small and explicit so it's easy to find project-wide settings.

pub const JD_AREA_MIN: usize = 10;
pub const JD_AREA_MAX: usize = 39;

pub const SHELF_COUNT: usize = 3;
pub const SLOTS_PER_SHELF: usize = 10;

// Physical management slots (first slot on each shelf)
pub const MANAGEMENT_SLOTS: [usize; 3] = [1, 11, 21];

// Filenames and directories
pub const STATE_FILENAME: &str = "state.json";
pub const RENAME_SCRIPT: &str = "rename_script.sh";
pub const SHELF_DIR: &str = "shelf";

// Server defaults
pub const DEFAULT_SERVE_PORT: u16 = 3001;
pub const DEFAULT_TIDY_PORT: u16 = 3002;

// Script header used for generated shell scripts
pub const SHELL_HEADER: &str = "#!/usr/bin/env bash\n";

// Limits
pub const MAX_CATEGORIES_PER_SHELF: usize = 10; // includes management slot
pub const MAX_NON_MGMT_CATEGORIES: usize = 9; // per shelf
pub const MAX_IDS_PER_CATEGORY: usize = 10;
