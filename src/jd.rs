use serde::{Deserialize, Serialize};

use crate::config::*;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JdItem {
    pub id: String,
    pub title: String,
    pub physical_loc: usize,
    pub original_path: String,
    pub area: String,
    pub is_file: bool,
    pub parent_path: String,
    pub level: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JdIndex {
    pub items: Vec<JdItem>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RenameItem {
    pub old_path: String,
    pub new_name: String,
}

/// Return shell-quoted string safe for single-quoted POSIX shell usage.
/// It wraps the input in single quotes and escapes internal single quotes.
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let escaped = s.replace('\'', "'\\''");
    format!("'{}'", escaped)
}

/// Validate the JD index according to the rules in docs/johnny-decimal-rules.md.
/// Returns a vector of error messages. Empty vector => valid.
pub fn validate_index(index: &JdIndex) -> Vec<String> {
    let mut errors: Vec<String> = Vec::new();

    let areas: Vec<&JdItem> = index.items.iter().filter(|i| i.level == "area").collect();
    if areas.len() != SHELF_COUNT {
        errors.push(format!(
            "Expected exactly {} Areas (shelves), found {}.",
            SHELF_COUNT,
            areas.len()
        ));
    }

    use std::collections::HashMap;
    let categories: Vec<&JdItem> = index.items.iter().filter(|i| i.level == "category").collect();
    let mut ids_per_category: HashMap<String, usize> = HashMap::new();
    let mut path_map: HashMap<&str, &JdItem> = HashMap::new();
    for item in index.items.iter() {
        path_map.insert(&item.original_path, item);
    }

    for cat in &categories {
        if let Ok(num) = cat.id.parse::<usize>() {
            if num < JD_AREA_MIN || num > JD_AREA_MAX {
                errors.push(format!("Category id '{}' out of valid JD range ({}..{}).", cat.id, JD_AREA_MIN, JD_AREA_MAX));
            }
        } else {
            errors.push(format!("Category id '{}' is not a valid number.", cat.id));
        }

        if cat.id.ends_with('0') {
            if !MANAGEMENT_SLOTS.contains(&cat.physical_loc) {
                errors.push(format!(
                    "Management category '{}' is not in a management slot (physical_loc={}).",
                    cat.id, cat.physical_loc
                ));
            }
        } else if MANAGEMENT_SLOTS.contains(&cat.physical_loc) {
            errors.push(format!(
                "Non-management category '{}' found in a management slot (physical_loc={}).",
                cat.id, cat.physical_loc
            ));
        }
    }

    let mut cats_per_shelf: [usize; SHELF_COUNT] = [0; SHELF_COUNT];
    for cat in &categories {
        if cat.physical_loc >= 1 && cat.physical_loc <= SHELF_COUNT * SLOTS_PER_SHELF {
            let shelf = (cat.physical_loc - 1) / SLOTS_PER_SHELF;
            cats_per_shelf[shelf] += 1;
        }
    }
    for (idx, count) in cats_per_shelf.iter().enumerate() {
        if *count > MAX_CATEGORIES_PER_SHELF {
            errors.push(format!(
                "Shelf {} has {} categories (including management) — exceeds {} slots.",
                idx + 1,
                count,
                MAX_CATEGORIES_PER_SHELF
            ));
        }
        let non_mgmt = categories
            .iter()
            .filter(|c| {
                let shelf = if c.physical_loc >= 1 && c.physical_loc <= SHELF_COUNT * SLOTS_PER_SHELF {
                    (c.physical_loc - 1) / SLOTS_PER_SHELF
                } else {
                    usize::MAX
                };
                shelf == idx && !c.id.ends_with('0')
            })
            .count();
        if non_mgmt > MAX_NON_MGMT_CATEGORIES {
            errors.push(format!(
                "Shelf {} has {} non-management categories — exceeds {} allowed.",
                idx + 1,
                non_mgmt,
                MAX_NON_MGMT_CATEGORIES
            ));
        }
    }

    for item in index.items.iter().filter(|i| i.level == "id") {
        if let Some(pos) = item.id.find('.') {
            let prefix = &item.id[..pos];
            if let Some(parent) = path_map.get(item.parent_path.as_str()) {
                if parent.level != "category" {
                    errors.push(format!(
                        "ID '{}' parent is not a category: parent_path='{}' parent_level='{}'.",
                        item.id, item.parent_path, parent.level
                    ));
                } else if parent.id != prefix {
                    errors.push(format!(
                        "ID '{}' prefix '{}' does not match parent category id '{}'.",
                        item.id, prefix, parent.id
                    ));
                }
            } else {
                errors.push(format!(
                    "ID '{}' has no parent folder found in index (parent_path='{}').",
                    item.id, item.parent_path
                ));
            }

            let counter = ids_per_category.entry(prefix.to_string()).or_insert(0);
            *counter += 1;
            if *counter > MAX_IDS_PER_CATEGORY {
                errors.push(format!("Category '{}' has more than {} IDs ({}).", prefix, MAX_IDS_PER_CATEGORY, *counter));
            }
        } else {
            errors.push(format!("ID '{}' malformed (expected 'XX.YY').", item.id));
        }
    }

    let mut seen_slots: HashMap<usize, Vec<&str>> = HashMap::new();
    for cat in &categories {
        if cat.physical_loc > 0 {
            seen_slots.entry(cat.physical_loc).or_default().push(&cat.id);
        }
    }
    for (slot, ids) in seen_slots.iter() {
        if ids.len() > 1 {
            errors.push(format!("Multiple categories occupy physical slot {}: {:?}", slot, ids));
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_quote_empty() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn test_shell_quote_simple() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_quote_single_quote() {
        assert_eq!(shell_quote("o'clock"), "'o'\\''clock'");
    }

    #[test]
    fn test_validate_basic_good() {
        let items = vec![
            JdItem { id: "10-19".to_string(), title: "Personal".to_string(), physical_loc: 0, original_path: "area/10-19 Personal".to_string(), area: "area".to_string(), is_file: false, parent_path: "".to_string(), level: "area".to_string() },
            JdItem { id: "20-29".to_string(), title: "CS".to_string(), physical_loc: 0, original_path: "area/20-29 CS".to_string(), area: "area".to_string(), is_file: false, parent_path: "".to_string(), level: "area".to_string() },
            JdItem { id: "30-39".to_string(), title: "Math".to_string(), physical_loc: 0, original_path: "area/30-39 Math".to_string(), area: "area".to_string(), is_file: false, parent_path: "".to_string(), level: "area".to_string() },

            JdItem { id: "10".to_string(), title: "Inbox".to_string(), physical_loc: 1, original_path: "area/10-19 Personal/10 Inbox".to_string(), area: "area".to_string(), is_file: false, parent_path: "area/10-19 Personal".to_string(), level: "category".to_string() },
            JdItem { id: "20".to_string(), title: "CS Management".to_string(), physical_loc: 11, original_path: "area/20-29 CS/20 CS Management".to_string(), area: "area".to_string(), is_file: false, parent_path: "area/20-29 CS".to_string(), level: "category".to_string() },
            JdItem { id: "30".to_string(), title: "Math Management".to_string(), physical_loc: 21, original_path: "area/30-39 Math/30 Math Management".to_string(), area: "area".to_string(), is_file: false, parent_path: "area/30-39 Math".to_string(), level: "category".to_string() },

            JdItem { id: "11".to_string(), title: "Books".to_string(), physical_loc: 2, original_path: "area/10-19 Personal/11 Books".to_string(), area: "area".to_string(), is_file: false, parent_path: "area/10-19 Personal".to_string(), level: "category".to_string() },
            JdItem { id: "11.01".to_string(), title: "Cosmere".to_string(), physical_loc: 0, original_path: "area/10-19 Personal/11 Books/11.01 Cosmere".to_string(), area: "area".to_string(), is_file: false, parent_path: "area/10-19 Personal/11 Books".to_string(), level: "id".to_string() },
        ];
        let idx = JdIndex { items };
        let errs = validate_index(&idx);
        assert!(errs.is_empty(), "expected no errors but got: {:?}", errs);
    }

    #[test]
    fn test_validate_bad_management_slot() {
        let items = vec![
            JdItem { id: "10-19".to_string(), title: "Personal".to_string(), physical_loc: 0, original_path: "area/10-19 Personal".to_string(), area: "area".to_string(), is_file: false, parent_path: "".to_string(), level: "area".to_string() },
            JdItem { id: "20-29".to_string(), title: "CS".to_string(), physical_loc: 0, original_path: "area/20-29 CS".to_string(), area: "area".to_string(), is_file: false, parent_path: "".to_string(), level: "area".to_string() },
            JdItem { id: "30-39".to_string(), title: "Math".to_string(), physical_loc: 0, original_path: "area/30-39 Math".to_string(), area: "area".to_string(), is_file: false, parent_path: "".to_string(), level: "area".to_string() },
            JdItem { id: "10".to_string(), title: "Inbox".to_string(), physical_loc: 2, original_path: "area/10-19 Personal/10 Inbox".to_string(), area: "area".to_string(), is_file: false, parent_path: "area/10-19 Personal".to_string(), level: "category".to_string() },
        ];
        let idx = JdIndex { items };
        let errs = validate_index(&idx);
        assert!(!errs.is_empty(), "expected errors but got none");
    }
}
