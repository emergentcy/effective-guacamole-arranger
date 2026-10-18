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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { path, output } => {
            println!("Scanning directory recursively: {:?}", path);
            let mut items = Vec::new();
            
            let re_area = Regex::new(r"^(\d{2})-(\d{2})\s+(.*)$").unwrap();
            let re_cat = Regex::new(r"^(\d{2})\s+(.*)$").unwrap();
            let re_id = Regex::new(r"^(\d{2}\.\d{2})\s+(.*)$").unwrap();

            for entry in WalkDir::new(&path).into_iter().filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || entry.depth() == 0 { continue; }
                
                let path_str = entry.path().to_string_lossy().to_string();
                let parent_path = entry.path().parent().unwrap_or(std::path::Path::new("")).to_string_lossy().to_string();
                let is_file = entry.file_type().is_file();

                let (id, title, level) = if let Some(caps) = re_area.captures(&name) {
                    (format!("{}-{}", &caps[1], &caps[2]), caps[3].to_string(), "area")
                } else if let Some(caps) = re_cat.captures(&name) {
                    (caps[1].to_string(), caps[2].to_string(), "category")
                } else if let Some(caps) = re_id.captures(&name) {
                    (caps[1].to_string(), caps[2].to_string(), "id")
                } else if is_file {
                    ("".to_string(), name.clone(), "file")
                } else {
                    ("".to_string(), name.clone(), "unknown")
                };

                if level == "unknown" { continue; }

                // Calculate physical_loc based on JD logic
                let mut physical_loc = 0;
                if level == "category" || level == "id" {
                    let base_id = if level == "category" { id.clone() } else { id.split('.').next().unwrap_or(&id).to_string() };
                    if let Ok(num) = base_id.parse::<usize>() {
                        if num >= 10 && num <= 39 {
                            let decade = num / 10; // 1 for 10-19, 2 for 20-29, etc.
                            let row = num % 10;    // 0, 1, 2, ..., 9
                            // Slot mapping: 10->1, 11->2, ..., 19->10, 20->11, ...
                            physical_loc = (decade - 1) * 10 + row + 1;
                        }
                    }
                }

                let parts: Vec<&str> = path_str.split('/').collect();
                let area = if parts.len() > 1 { parts[1].to_string() } else { "Uncategorized".to_string() };

                items.push(JdItem {
                    id, title, physical_loc,
                    original_path: path_str,
                    area,
                    is_file,
                    parent_path,
                    level: level.to_string(),
                });
            }

            let index = JdIndex { items };
            fs::write(&output, serde_json::to_string_pretty(&index).unwrap()).unwrap();
            println!("✅ Found {} total items. Saved to {:?}", index.items.len(), output);
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
    let json_str = fs::read_to_string("state.json").unwrap_or_else(|_| "{\"items\":[]}".to_string());
    let html = generate_html(&json_str);
    (
        [(axum::http::header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")],
        Html(html),
    )
}

async fn export_script(Json(index): Json<JdIndex>) -> impl IntoResponse {
    let mut script = String::from("#!/usr/bin/env bash\necho 'Updating JD Library...'\n\n");
    script.push_str("mkdir -p shelf\n\n");
    
    for item in index.items {
        // Only move Categories (folders that contain IDs)
        if item.physical_loc > 0 && item.level == "category" {
            
            // Skip Management folders (XX0) - they stay where they are
            if item.id.ends_with('0') {
                continue;
            }

            // Calculate the new JD ID based on the physical slot
            let slot_index = item.physical_loc - 1;
            let new_id = (slot_index / 10 + 1) * 10 + (slot_index % 10);
            
            let new_name = format!("{:02} - {} {}", item.physical_loc, new_id, item.title);
            script.push_str(&format!("mv \"{}\" \"shelf/{}\"\n", item.original_path, new_name));
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
    <title>JD Wrangler</title>
    <style>
        body { font-family: monospace; background: #1a1a1a; color: #eee; padding: 10px; margin: 0; padding-bottom: 60px; }
        .header { display: flex; justify-content: space-between; align-items: center; padding: 10px; }
        .meta-drawer { background: #111; border: 1px solid #444; border-radius: 8px; margin-bottom: 20px; padding: 10px; }
        .meta-drawer summary { cursor: pointer; color: #ff9900; font-weight: bold; font-size: 14px; }
        .meta-list { margin-top: 10px; display: flex; flex-wrap: wrap; gap: 8px; }

        .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin-top: 10px; }
        .shelf { display: flex; flex-direction: column; gap: 5px; }
        .shelf-box { border: 2px dashed #444; padding: 8px; border-radius: 8px; background: #222; flex: 1; display: flex; flex-direction: column; }
        
        .shelf-header { margin-bottom: 10px; text-align: center; }
        .shelf-header select { background: transparent; color: #00ccff; border: none; font-family: monospace; font-size: 14px; font-weight: bold; cursor: pointer; outline: none; text-align: center; }
        
        .slot { flex: 1; min-height: 60px; border: 1px dashed #333; margin-bottom: 8px; border-radius: 4px; display: flex; flex-direction: column; justify-content: center; cursor: pointer; padding: 4px; box-sizing: border-box; }
        .slot:hover { background: #2a2a2a; }
        .slot.locked { cursor: default; border-style: solid; }
        .slot.locked:hover { background: transparent; }
        
        .item { background: #333; padding: 8px; border-radius: 4px; width: 100%; border-left: 4px solid #00ccff; font-size: 12px; box-sizing: border-box; display: flex; justify-content: space-between; align-items: flex-start; overflow-wrap: anywhere; }
        .item.selected { border: 2px solid #00ff00; background: #1a331a; }
        .item-content { flex: 1; min-width: 0; cursor: pointer; }
        .item span.id { color: #00ccff; font-weight: bold; display: block; margin-bottom: 4px; font-size: 14px; }
        .item span.title { word-break: break-word; line-height: 1.3; }
        
        .btn-select { background: none; border: none; color: #888; cursor: pointer; font-size: 16px; padding: 0 0 0 5px; }
        .btn-select:hover { color: #fff; }
        
        .pool { background: #111; border: 1px solid #444; padding: 10px; border-radius: 8px; margin-bottom: 15px; max-height: 150px; overflow-y: auto; display: flex; flex-wrap: wrap; gap: 8px; }
        .pool .item { width: auto; flex: 1 1 250px; border-left-color: #888; }
        .pool-title { color: #666; width: 100%; font-size: 14px; margin-bottom: 5px; }
        
        button.export { background: #00ccff; color: #000; border: none; padding: 10px 15px; font-weight: bold; cursor: pointer; border-radius: 4px; font-size: 14px; }
        
        .area-personal { border-left-color: #00ccff !important; }
        .area-cs { border-left-color: #00ff00 !important; }
        .area-math { border-left-color: #ff00ff !important; }
        .area-system { border-left-color: #ff9900 !important; background: #2a1f16; }

        #stats-bar { position: fixed; bottom: 0; left: 0; width: 100%; background: #222; border-top: 2px solid #444; color: #888; font-size: 12px; display: flex; justify-content: space-around; padding: 10px 0; box-sizing: border-box; z-index: 50; }
        #stats-bar span { color: #fff; font-weight: bold; }

        #modal { display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%; background: rgba(17,17,17,0.95); z-index: 100; flex-direction: column; padding: 15px; box-sizing: border-box; overflow-y: auto; }
        #modal-header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px; border-bottom: 2px solid #444; padding-bottom: 10px; }
        #modal-title { color: #00ccff; font-weight: bold; font-size: 18px; }
        #modal-close { background: #ff4444; color: #fff; border: none; padding: 8px 12px; border-radius: 4px; font-weight: bold; cursor: pointer; }
        .file-item { padding: 10px; background: #222; margin-bottom: 5px; border-radius: 4px; border-left: 2px solid #666; }
        .folder-item { padding: 10px; background: #222; margin-bottom: 5px; border-radius: 4px; border-left: 2px solid #00ccff; cursor: pointer; }
        .folder-item:hover { background: #333; }
    </style>
</head>
<body>
    <script id="state-data" type="application/json">JSON_DATA_HERE</script>

    <div class="header">
        <h2>JD Wrangler</h2>
        <button class="export" onclick="generateScript()">Export</button>
    </div>

    <details class="meta-drawer" open>
        <summary>00-09 System Management</summary>
        <div class="meta-list" id="meta-list"></div>
    </details>

    <div class="pool" id="pool" onclick="placeItem(-1)">
        <div class="pool-title">Unassigned Items</div>
    </div>

    <div class="grid" id="grid"></div>
    
    <div id="stats-bar">
        <div>Areas: <span id="stat-areas">0</span></div>
        <div>Categories: <span id="stat-cats">0</span></div>
        <div>IDs: <span id="stat-ids">0</span></div>
        <div>Files: <span id="stat-files">0</span></div>
    </div>

    <div id="output" style="display:none; margin-top:20px; background:#000; padding:15px; border:1px solid #444; color:#00ff00; white-space:pre-wrap; font-size:12px;"></div>

    <div id="modal">
        <div id="modal-header">
            <div id="modal-title">Folder</div>
            <button id="modal-close" onclick="closeModal()">Close</button>
        </div>
        <div id="modal-content" style="padding-bottom: 50px;"></div>
    </div>

    <script>
        const state = JSON.parse(document.getElementById('state-data').textContent);
        
        // Extract physical areas and sort them
        const areas = [...new Set(state.items.filter(i => i.level === 'area' && !i.title.includes('System')).map(i => i.original_path))].sort();
        
        // Initial shelf mapping
        let shelfPaths = [areas[0], areas[1], areas[2]];
        
        let selectedItemId = null;

        state.items.forEach(i => { if (i.title) i.title = i.title.replace(/\s+/g, ' ').trim(); });

        function updateStats() {
            document.getElementById('stat-areas').innerText = state.items.filter(i => i.level === 'area').length;
            document.getElementById('stat-cats').innerText = state.items.filter(i => i.level === 'category').length;
            document.getElementById('stat-ids').innerText = state.items.filter(i => i.level === 'id').length;
            document.getElementById('stat-files').innerText = state.items.filter(i => i.is_file).length;
        }

        function getAreaClass(areaPath) {
            if (!areaPath) return "";
            if (areaPath.includes("Personal")) return "area-personal";
            if (areaPath.includes("CS")) return "area-cs";
            if (areaPath.includes("Math")) return "area-math";
            if (areaPath.includes("System")) return "area-system";
            return "";
        }

        // Calculate display ID for UI (allows live swapping)
        function getDisplayId(item) {
            if (item.physical_loc > 0 && (item.level === 'category' || item.level === 'id')) {
                const slot_index = item.physical_loc - 1;
                const decade = Math.floor(slot_index / 10) + 1;
                const row = (slot_index % 10);
                if (item.level === 'category') {
                    return `${decade}${row}`;
                } else {
                    return `${decade}${row}.${item.id.split('.')[1] || '00'}`; // Preserve the .XX part for IDs
                }
            }
            return item.id;
        }

        function render() {
            updateStats();

            // Meta Drawer
            const metaList = document.getElementById('meta-list');
            metaList.innerHTML = '';
            const sysArea = state.items.find(i => i.level === 'area' && i.title.includes('System'));
            if (sysArea) {
                const sysChildren = state.items.filter(i => i.parent_path === sysArea.original_path);
                sysChildren.sort((a,b) => a.id.localeCompare(b.id));
                sysChildren.forEach(item => metaList.appendChild(createItemElement(item, false)));
            }

            // Pool
            const pool = document.getElementById('pool');
            pool.innerHTML = '<div class="pool-title">Unassigned Items</div>';
            
            // Find any Category/ID that belongs to a physical area but isn't placed
            const unassigned = state.items.filter(i => i.physical_loc === 0 && (i.level === 'category' || i.level === 'id') && i.area !== sysArea.original_path);
            if (unassigned.length === 0) pool.innerHTML += '<span style="color:#444;">(All placed)</span>';
            unassigned.forEach(item => pool.appendChild(createItemElement(item, false)));

            // Grid
            const grid = document.getElementById('grid');
            grid.innerHTML = '';

            for (let s = 0; s < 3; s++) {
                const shelfContainer = document.createElement('div');
                shelfContainer.className = 'shelf';
                
                const areaPath = shelfPaths[s];
                if (!areaPath) continue;

                const shelfBox = document.createElement('div');
                shelfBox.className = 'shelf-box';

                // Header with Dropdown
                const header = document.createElement('div');
                header.className = 'shelf-header';
                
                const select = document.createElement('select');
                select.innerHTML = `<option value="">-- Select Area --</option>`;
                areas.forEach(area => {
                    const sel = (area === areaPath) ? 'selected' : '';
                    select.innerHTML += `<option value="${area}" ${sel}>${area.split('/').pop()}</option>`;
                });
                select.onchange = (e) => {
                    shelfPaths[s] = e.target.value;
                    render();
                };
                header.appendChild(select);
                shelfBox.appendChild(header);

                // Get all children of this Area
                const allChildren = state.items.filter(i => i.parent_path === areaPath);
                
                // Find management folder (Category ending in 0)
                let mgmtItem = allChildren.find(i => i.level === 'category' && i.id.endsWith('0'));
                
                // Get physical items (Categories first, fallback to IDs if no Categories)
                let physicalItems = allChildren.filter(i => i !== mgmtItem && (i.level === 'category' || i.level === 'id')).sort((a,b) => a.id.localeCompare(b.id));
                
                let itemsToShow = physicalItems.filter(i => i.level === 'category');
                if (itemsToShow.length === 0) {
                    // If there are no Categories, show IDs directly
                    itemsToShow = physicalItems.filter(i => i.level === 'id');
                }

                for (let r = 0; r < 10; r++) {
                    const slotIndex = s * 10 + r;
                    const slot = document.createElement('div');
                    slot.className = 'slot';
                    
                    if (r === 0) {
                        // Row 1 is locked for Management
                        slot.classList.add('locked');
                        if (mgmtItem) {
                            slot.appendChild(createItemElement(mgmtItem, true)); // locked=true
                        } else {
                            slot.innerHTML = `<span style="color:#444; font-size:10px; text-align:center;">Row 1 (Mgmt)</span>`;
                        }
                    } else {
                        slot.onclick = () => placeItem(slotIndex);
                        
                        // Find the item that lives in this slot
                        let item = itemsToShow.find(i => i.physical_loc === slotIndex + 1);
                        
                        // If it's an unassigned item in the pool, we can display it in its default slot
                        if (!item && itemsToShow.length > 0) {
                            // Optional: Auto-place unassigned items based on their default position
                            // For now, leave empty until user moves them from pool, or auto-assign them.
                            // Let's auto-assign if they have a default ID matching the slot.
                            // Actually, the scanner already sets physical_loc if the ID matches. So they are placed!
                        }

                        if (item) {
                            slot.appendChild(createItemElement(item, false));
                        } else {
                            slot.innerHTML = `<span style="color:#444; font-size:10px; text-align:center;">Row ${r + 1}</span>`;
                        }
                    }
                    shelfBox.appendChild(slot);
                }

                shelfContainer.appendChild(shelfBox);
                grid.appendChild(shelfContainer);
            }
        }

        function createItemElement(item, isLocked) {
            const div = document.createElement('div');
            div.className = 'item ' + getAreaClass(item.area) + (selectedItemId === item.id ? ' selected' : '');
            
            const content = document.createElement('div');
            content.className = 'item-content';
            const displayId = getDisplayId(item);
            content.innerHTML = `<span class="id">${displayId}</span><span class="title">${item.title}</span>`;
            
            content.onclick = (e) => {
                e.stopPropagation();
                openModal(item);
            };

            div.appendChild(content);

            if (!isLocked) {
                const selBtn = document.createElement('button');
                selBtn.className = 'btn-select';
                selBtn.innerText = '✥';
                selBtn.title = "Select to move";
                selBtn.onclick = (e) => {
                    e.stopPropagation();
                    selectedItemId = (selectedItemId === item.id) ? null : item.id;
                    render();
                };
                div.appendChild(selBtn);
            }

            return div;
        }

        function placeItem(slotIndex) {
            if (!selectedItemId) return;
            
            // Prevent dropping into the locked first row of any shelf
            if (slotIndex % 10 === 0) return;

            const item = state.items.find(i => i.id === selectedItemId);
            if (!item) return;

            if (slotIndex === -1) {
                item.physical_loc = 0;
                selectedItemId = null;
                render();
                return;
            }

            const targetShelf = Math.floor(slotIndex / 10);
            const targetAreaPath = shelfPaths[targetShelf];
            if (item.area !== targetAreaPath) {
                alert("You cannot move an item to a shelf of a different Area.");
                return;
            }

            const oldItem = state.items.find(i => i.physical_loc === slotIndex + 1);
            if (oldItem) {
                oldItem.physical_loc = item.physical_loc; 
            }

            item.physical_loc = slotIndex + 1;
            selectedItemId = null;
            render();
        }

        function openModal(item) {
            document.getElementById('modal').style.display = 'flex';
            document.getElementById('modal-title').innerText = item.id ? `${item.id} ${item.title}` : item.title;
            
            const content = document.getElementById('modal-content');
            content.innerHTML = '';
            
            const children = state.items.filter(i => i.parent_path === item.original_path);
            children.sort((a,b) => {
                if (a.is_file !== b.is_file) return a.is_file ? 1 : -1;
                return a.id.localeCompare(b.id);
            });

            if (children.length === 0) {
                content.innerHTML = '<p style="color:#888;">No subfolders or files found here.</p>';
                return;
            }

            children.forEach(child => {
                const div = document.createElement('div');
                if (child.is_file) {
                    div.className = 'file-item';
                    div.innerHTML = `📄 ${child.title}`;
                } else {
                    div.className = 'folder-item';
                    div.innerHTML = `📁 ${child.id ? child.id + ' ' : ''}${child.title}`;
                    div.onclick = () => openModal(child); 
                }
                content.appendChild(div);
            });
        }

        function closeModal() {
            document.getElementById('modal').style.display = 'none';
        }

        async function generateScript() {
            const btn = document.querySelector('button.export');
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
                btn.innerText = 'Export';
                window.scrollTo(0, document.body.scrollHeight);
            } catch (e) {
                alert("Failed to generate script.");
                btn.innerText = 'Export';
            }
        }

        render();
    </script>
</body>
</html>
"##;
    template.replace("JSON_DATA_HERE", json_data)
}
