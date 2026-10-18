# Johnny.Decimal (JD) Classification Ruleset

This document defines the strict structural rules for the Johnny.Decimal system as implemented by the `effective-guacamole-arranger`. 

The goal of this tool is to map a physical 3-shelf library (with 10 slots per shelf) to the logical JD numbering system.

## 1. Hierarchy Levels

The system is strictly divided into four levels:

1. **System (00-09)**: Meta rules, indexes, and management of the system itself.
2. **Area (10-19, 20-29, 30-39)**: Broad domains of life/work. 
   * *Constraint*: The physical library supports exactly three Areas, mapped directly to Shelves 1, 2, and 3.
3. **Category (10, 11, 12...)**: Specific topics within an Area.
   * *Constraint*: 10 Categories per Area (0 through 9).
4. **ID (11.01, 11.02...)**: Individual items or projects within a Category.
   * *Constraint*: 10 IDs per Category.
5. **File**: The actual documents (PDF, EPUB, etc.) residing inside an ID folder.

## 2. Physical Slot Mapping (The 3x10 Grid)

The physical shelf layout *is* the logical structure. 
* **Shelf 1**: Area 10-19 (Slots 1-10)
* **Shelf 2**: Area 20-29 (Slots 11-20)
* **Shelf 3**: Area 30-39 (Slots 21-30)

### The Management Rule (Slot X0)
The first slot of every shelf (Slot 1, Slot 11, Slot 21) is permanently locked to the **Management Category** for that Area (e.g., `10 Inbox`, `20 CS Management`, `30 Math Management`).
* *Rule*: Management folders are fixed. They cannot be moved to other slots.
* *Rule*: Management folders do not consume IDs (10.01, 10.02, etc. are handled *inside* the management folder, not on the physical shelf).

### Category Rules (Slots X1 - X9)
The remaining 9 slots on each shelf are for Categories.
* Slot 2 = `11`, Slot 3 = `12`... Slot 10 = `19`.
* *Dynamic Renumbering*: If you move a Category to a different slot (e.g., moving `32 Calculus` to slot 23), its logical ID automatically updates to `33`. The physical position defines the ID.

## 3. Naming Conventions

All folders must follow the format `XX Title` or `XX.YY Title`.
* **Area**: `10-19 Title` (e.g., `10-19 Personal`)
* **Category**: `XX Title` (e.g., `11 Books`)
* **ID**: `XX.YY Title` (e.g., `11.01 Cosmere`)

## 4. Exceptions & Nesting

* Nested Management: If a subfolder matches the `00-09` or `XX0` pattern (e.g., `30-39 Math/00-09 Archive`), it is treated as a meta/management folder and removed from the physical 3x10 grid, appearing in the top drawer instead.
* Files: Files do not have JD numbers. They live directly inside an ID folder (e.g., `11.01 Cosmere/Hoid Travails.epub`).

## 5. Future Validation Logic (TODO)

When implementing the validator, the code must check:
- [ ] Does the folder have 3 Areas? Are there more than 10 slots used?
- [ ] Is slot X0 reserved for management folders?
- [ ] Are all other slots populated with `XX1` to `XX9`?
- [ ] Does every ID match the parent Category? (e.g., `11.01` must live inside `11 Books`).
- [ ] Are there no more than 10 IDs in a Category?
