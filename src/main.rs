use axum::{
    Router,
    extract::{Json, State},
    response::{Html, IntoResponse},
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use walkdir::WalkDir;

#[derive(Parser)]
#[command(author, version, about = "Johnny.Decimal Library Wrangler")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scans a directory recursively and outputs the JD state as JSON
    Scan {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long, default_value = "state.json")]
        output: PathBuf,
    },
    /// Boots a local web server to visually arrange the library
    Serve {
        #[arg(default_value = "state.json")]
        state: PathBuf,
        #[arg(short, long, default_value_t = 3001)]
        port: u16,
    },
    /// Suggests shorter names for long files and generates a rename script
    Tidy {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Boot a web UI to review and edit names before exporting
        #[arg(short, long)]
        gui: bool,
        #[arg(short, long, default_value_t = 3002)]
        port: u16,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JdItem {
    pub id: String,
    pub title: String,
    pub physical_loc: usize,
    pub original_path: String,
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { path, output } => {
            println!("Scanning directory recursively: {:?}", path);
            let mut items = Vec::new();
            let re = Regex::new(r"^(\d{2}(?:\.\d{2})?)\s+(.*)$").unwrap();

            for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') || entry.depth() == 0 {
                        continue;
                    }
                    if let Some(caps) = re.captures(&name) {
                        let id = caps[1].to_string();
                        let title = caps[2].to_string();
                        if !items.iter().any(|i: &JdItem| i.id == id) {
                            items.push(JdItem {
                                id,
                                title,
                                physical_loc: 0,
                                original_path: entry.path().to_string_lossy().to_string(),
                            });
                        }
                    }
                }
            }
            items.sort_by(|a, b| a.id.cmp(&b.id));
            let index = JdIndex { items };
            fs::write(&output, serde_json::to_string_pretty(&index).unwrap()).unwrap();
            println!(
                "✅ Found {} JD items. Saved to {:?}",
                index.items.len(),
                output
            );
        }
        Commands::Serve { state: _, port } => {
            println!("Starting server on http://127.0.0.1:{}", port);
            let app = Router::new()
                .route("/", get(render_ui))
                .route("/api/export", post(export_script));
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        }
        Commands::Tidy { path, gui, port } => {
            println!("Scanning for long filenames in: {:?}", path);
            let mut renames = Vec::new();

            for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    let old_name = entry.file_name().to_string_lossy().to_string();
                    let new_name = propose_short_name(&old_name);

                    if old_name != new_name {
                        renames.push(RenameItem {
                            old_path: entry.path().to_string_lossy().to_string(),
                            new_name,
                        });
                    }
                }
            }

            println!("Found {} files to rename.", renames.len());

            if gui {
                println!("Starting GUI on http://127.0.0.1:{}", port);
                let shared_renames = Arc::new(renames);
                let app = Router::new()
                    .route("/", get(render_tidy_ui_handler))
                    .route("/api/export_tidy", post(export_tidy_script))
                    .with_state(shared_renames);
                let addr = SocketAddr::from(([127, 0, 0, 1], port));
                let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
                axum::serve(listener, app).await.unwrap();
            } else {
                let script = generate_tidy_script(&renames);
                fs::write("rename_script.sh", script).unwrap();
                println!("✅ Generated rename_script.sh. Review it and run: bash rename_script.sh");
            }
        }
    }
}

fn propose_short_name(filename: &str) -> String {
    let (stem, ext) = if let Some(pos) = filename.rfind('.') {
        filename.split_at(pos)
    } else {
        (filename, "")
    };

    let mut clean = stem.to_string();
    for junk in &[
        "-- Anna’s Archive",
        "-- Anna's Archive",
        "libgen.li",
        "_converted",
        "-- Z-Library",
    ] {
        clean = clean.replace(junk, "");
    }

    let parts: Vec<&str> = clean.split(" -- ").map(|s| s.trim()).collect();

    if parts.is_empty() {
        return filename.to_string();
    }

    let mut title = parts[0].to_string();
    let mut author = String::new();
    let mut year = String::new();

    if parts.len() > 1 {
        author = parts[1].to_string();
        author = author
            .replace("(auth_)", "")
            .replace("(author)", "")
            .replace("[", "")
            .replace("]", "");
        author = author
            .trim_matches(|c| c == ',' || c == '_' || c == ' ')
            .to_string();
    }

    let re_year = Regex::new(r"\b(19|20)\d{2}\b").unwrap();

    for i in 2..parts.len() {
        if let Some(mat) = re_year.find(parts[i]) {
            year = mat.as_str().to_string();
            break;
        }
    }

    if year.is_empty() {
        if let Some(mat) = re_year.find(&author) {
            year = mat.as_str().to_string();
            author = re_year.replace(&author, "").to_string();
            author = author
                .trim_matches(|c| c == ',' || c == '_' || c == ' ')
                .to_string();
        }
    }

    title = title
        .replace("_", " ")
        .replace("  ", " ")
        .trim()
        .to_string();

    let mut final_name = title.clone();
    if !author.is_empty() && author.to_lowercase() != "unknown" {
        final_name = format!("{} - {}", final_name, author);
    }
    if !year.is_empty() {
        final_name = format!("{} - {}", final_name, year);
    }

    if final_name.len() > 90 {
        let year_suffix = if !year.is_empty() {
            format!(" - {}", year)
        } else {
            String::new()
        };
        let author_suffix = if !author.is_empty() && author.to_lowercase() != "unknown" {
            format!(" - {}", author)
        } else {
            String::new()
        };

        let max_title_len = 90_usize.saturating_sub(year_suffix.len() + author_suffix.len() + 5);

        let mut short_title = title;
        if short_title.len() > max_title_len {
            short_title = short_title
                .chars()
                .take(max_title_len.saturating_sub(3))
                .collect::<String>()
                + "...";
        }
        final_name = format!("{}{}{}", short_title, author_suffix, year_suffix);
    }

    format!("{}{}", final_name, ext)
}

fn generate_tidy_script(items: &[RenameItem]) -> String {
    let mut script = String::from("#!/usr/bin/env bash\necho 'Renaming files...'\n\n");
    for item in items {
        let dir = std::path::Path::new(&item.old_path)
            .parent()
            .unwrap()
            .to_string_lossy();
        let old_name = std::path::Path::new(&item.old_path)
            .file_name()
            .unwrap()
            .to_string_lossy();
        script.push_str(&format!(
            "mv \"{}/{}\" \"{}/{}\"\n",
            dir, old_name, dir, item.new_name
        ));
    }
    script.push_str("\necho 'Done.'\n");
    script
}

async fn export_tidy_script(Json(items): Json<Vec<RenameItem>>) -> impl IntoResponse {
    generate_tidy_script(&items)
}

async fn render_ui() -> impl IntoResponse {
    let json_str =
        fs::read_to_string("state.json").unwrap_or_else(|_| "{\"items\":[]}".to_string());
    Html(generate_html(&json_str))
}

async fn export_script(Json(index): Json<JdIndex>) -> impl IntoResponse {
    let mut script = String::from("#!/usr/bin/env bash\necho 'Updating JD Library...'\n\n");
    script.push_str("mkdir -p shelf\n\n");
    for item in index.items {
        if item.physical_loc > 0 {
            let new_name = format!("{:02} - {} {}", item.physical_loc, item.id, item.title);
            script.push_str(&format!(
                "mv \"{}\" \"shelf/{}\"\n",
                item.original_path, new_name
            ));
        }
    }
    script.push_str("\necho 'Done. Look in the ./shelf/ folder.'\n");
    script
}

async fn render_tidy_ui_handler(State(renames): State<Arc<Vec<RenameItem>>>) -> impl IntoResponse {
    Html(generate_tidy_html(&renames))
}

fn generate_tidy_html(items: &[RenameItem]) -> String {
    let json_data = serde_json::to_string(items).unwrap();
    let template = r##"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>JD Tidy</title>
    <style>
        body { font-family: monospace; background: #1a1a1a; color: #eee; padding: 10px; }
        .file-row { background: #222; padding: 10px; margin-bottom: 10px; border-radius: 4px; border-left: 4px solid #ff9900; }
        .old-name { color: #888; font-size: 12px; margin-bottom: 5px; word-wrap: break-word; }
        input { width: 100%; padding: 8px; background: #111; color: #00ccff; border: 1px solid #444; font-family: monospace; border-radius: 4px; box-sizing: border-box; }
        button { background: #00ccff; color: #000; border: none; padding: 12px; font-weight: bold; cursor: pointer; border-radius: 4px; width: 100%; font-size: 16px; margin-bottom: 20px; }
        #output { display: none; margin-top: 20px; background: #000; padding: 15px; border: 1px solid #444; overflow-x: auto; color: #00ff00; font-size: 12px; white-space: pre-wrap; }
    </style>
</head>
<body>
    <h2>Tidy Library</h2>
    <p style="color: #888; font-size: 12px;">Review the proposed names below. Edit them freely, then click Export.</p>
    <button onclick="generateScript()">Export Rename Script</button>
    <div id="list"></div>
    <div id="output"></div>

    <script>
        const items = JSON_DATA_HERE;
        const list = document.getElementById('list');

        items.forEach((item, index) => {
            const row = document.createElement('div');
            row.className = 'file-row';
            row.innerHTML = `<div class="old-name">${item.old_path}</div><input type="text" id="input-${index}" value="${item.new_name}" />`;
            list.appendChild(row);
        });

        async function generateScript() {
            const btn = document.querySelector('button');
            btn.innerText = 'Generating...';
            items.forEach((item, index) => {
                item.new_name = document.getElementById(`input-${index}`).value;
            });
            try {
                const res = await fetch('/api/export_tidy', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(items)
                });
                const scriptText = await res.text();
                const output = document.getElementById('output');
                output.innerText = scriptText;
                output.style.display = 'block';
                btn.innerText = 'Export Rename Script';
            } catch (e) {
                alert("Failed to generate script.");
                btn.innerText = 'Export Rename Script';
            }
        }
    </script>
</body>
</html>
"##;
    template.replace("JSON_DATA_HERE", &json_data)
}

fn generate_html(json_data: &str) -> String {
    let template = r##"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>JD Library Wrangler</title>
    <style>
        body { font-family: monospace; background: #1a1a1a; color: #eee; padding: 10px; margin: 0; }
        .header { display: flex; justify-content: space-between; align-items: center; padding: 10px; }
        .grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin-top: 20px; }
        .shelf { border: 2px dashed #444; padding: 8px; border-radius: 8px; background: #222; }
        .shelf-title { text-align: center; font-weight: bold; margin-bottom: 10px; color: #ff9900; font-size: 14px; }
        .slot { min-height: 50px; border: 1px dashed #333; margin-bottom: 8px; border-radius: 4px; display: flex; align-items: center; justify-content: center; cursor: pointer; }
        .slot:hover { background: #2a2a2a; }
        .item { background: #333; padding: 8px; border-radius: 4px; width: 100%; border-left: 4px solid #00ccff; font-size: 12px; box-sizing: border-box; word-wrap: break-word; cursor: pointer; }
        .item.selected { border: 2px solid #00ff00; background: #1a331a; }
        .item span { color: #00ccff; font-weight: bold; display: block; margin-bottom: 4px; font-size: 14px; }
        button { background: #00ccff; color: #000; border: none; padding: 10px 15px; font-weight: bold; cursor: pointer; border-radius: 4px; font-size: 14px; }
        #output { display: none; margin-top: 20px; background: #000; padding: 15px; border: 1px solid #444; overflow-x: auto; color: #00ff00; font-size: 12px; white-space: pre-wrap; }
    </style>
</head>
<body>
    <div class="header">
        <h2>JD Wrangler</h2>
        <button onclick="generateScript()">Export Script</button>
    </div>
    <p style="padding: 0 10px; font-size: 12px; color: #888;">Tap an item to select it, then tap an empty slot to place it.</p>
    <div class="grid" id="grid"></div>
    <div id="output"></div>
    <script>
        const state = JSON_DATA_HERE;
        const totalSlots = 30;
        let slots = new Array(totalSlots).fill(null);
        let selectedItemId = null;
        state.items.forEach(item => {
            if (item.physical_loc > 0 && item.physical_loc <= totalSlots) slots[item.physical_loc - 1] = item;
        });
        const grid = document.getElementById('grid');
        function render() {
            grid.innerHTML = '';
            for (let s = 0; s < 3; s++) {
                const shelf = document.createElement('div');
                shelf.className = 'shelf';
                shelf.innerHTML = `<div class="shelf-title">SHELF ${s + 1}</div>`;
                for (let r = 0; r < 10; r++) {
                    const slotIndex = s * 10 + r;
                    const slot = document.createElement('div');
                    slot.className = 'slot';
                    slot.dataset.slotId = slotIndex;
                    slot.onclick = () => handleSlotClick(slotIndex);
                    const item = slots[slotIndex];
                    if (item) {
                        const div = document.createElement('div');
                        div.className = 'item' + (selectedItemId === item.id ? ' selected' : '');
                        div.innerHTML = `<span>${item.id}</span>${item.title}`;
                        div.onclick = (e) => { e.stopPropagation(); handleItemClick(item.id); };
                        slot.appendChild(div);
                    } else {
                        slot.innerHTML = `<span style="color:#444; font-size:10px;">${slotIndex + 1}</span>`;
                    }
                    shelf.appendChild(slot);
                }
                grid.appendChild(shelf);
            }
        }
        function handleItemClick(itemId) {
            selectedItemId = (selectedItemId === itemId) ? null : itemId;
            render();
        }
        function handleSlotClick(slotIndex) {
            if (!selectedItemId) return;
            const item = state.items.find(i => i.id === selectedItemId);
            if (!item) return;
            const oldSlotIndex = item.physical_loc > 0 ? item.physical_loc - 1 : slots.findIndex(i => i && i.id === item.id);
            const destinationItem = slots[slotIndex];
            if (oldSlotIndex >= 0) slots[oldSlotIndex] = null;
            if (destinationItem) {
                destinationItem.physical_loc = oldSlotIndex + 1 > 0 ? oldSlotIndex + 1 : 0;
                slots[oldSlotIndex] = destinationItem;
            }
            item.physical_loc = slotIndex + 1;
            slots[slotIndex] = item;
            selectedItemId = null;
            render();
        }
        async function generateScript() {
            const btn = document.querySelector('button');
            btn.innerText = '...';
            try {
                const res = await fetch('/api/export', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(state)
                });
                const scriptText = await res.text();
                const output = document.getElementById('output');
                output.innerText = scriptText;
                output.style.display = 'block';
                btn.innerText = 'Export Script';
            } catch (e) {
                alert("Failed to generate script.");
                btn.innerText = 'Export Script';
            }
        }
        render();
    </script>
</body>
</html>
"##;
    template.replace("JSON_DATA_HERE", json_data)
}
