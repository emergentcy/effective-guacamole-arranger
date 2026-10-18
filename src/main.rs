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

mod config;
mod jd;
use config::*;
use jd::{JdIndex, JdItem, RenameItem};

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
        #[arg(short, long, default_value = STATE_FILENAME)]
        output: PathBuf,
    },
    /// Boots a local web server to visually arrange the library
    Serve {
        #[arg(default_value = STATE_FILENAME)]
        state: PathBuf,
        #[arg(short, long, default_value_t = DEFAULT_SERVE_PORT)]
        port: u16,
    },
    /// Suggests shorter names for long files and generates a rename script
    Tidy {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Boot a web UI to review and edit names before exporting
        #[arg(short, long)]
        gui: bool,
        #[arg(short, long, default_value_t = DEFAULT_TIDY_PORT)]
        port: u16,
    },
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
                        if num >= JD_AREA_MIN && num <= JD_AREA_MAX {
                            let decade = num / 10; // 1 for 10-19, 2 for 20-29, etc.
                            let row = num % 10;    // 0, 1, 2, ..., 9
                            // Slot mapping: 10->1, 11->2, ..., 19->10, 20->11, ...
                            physical_loc = (decade - 1) * SLOTS_PER_SHELF + row + 1;
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
        Commands::Serve { state, port } => {
            println!("Starting server on http://127.0.0.1:{}", port);
            let app = Router::new()
                .route("/", get(render_ui))
                .route("/api/export", post(export_script));
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        }
        Commands::Tidy { path, gui, port } => {
            println!("Scanning for long filenames in: {}" , path.display());
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
                fs::write(RENAME_SCRIPT, script).unwrap();
                println!("✅ Generated {}. Review it and run: bash {}", RENAME_SCRIPT, RENAME_SCRIPT);
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
    let mut script = String::from(format!("{}echo 'Renaming files...'\n\n", SHELL_HEADER));
    for item in items {
        let dir = std::path::Path::new(&item.old_path)
            .parent()
            .unwrap()
            .to_string_lossy();
        let old_name = std::path::Path::new(&item.old_path)
            .file_name()
            .unwrap()
            .to_string_lossy();
        let from = jd::shell_quote(&format!("{}/{}", dir, old_name));
        let to = jd::shell_quote(&format!("{}/{}", dir, item.new_name));
        script.push_str(&format!("mv {} {}\n", from, to));
    }
    script.push_str("\necho 'Done.'\n");
    script
}

async fn export_tidy_script(Json(items): Json<Vec<RenameItem>>) -> impl IntoResponse {
    generate_tidy_script(&items)
}

async fn render_ui() -> impl IntoResponse {
    let json_str = fs::read_to_string(STATE_FILENAME).unwrap_or_else(|_| "{\"items\":[]}".to_string());
    let html = generate_html(&json_str);
    (
        [(axum::http::header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")],
        Html(html),
    )
}

use axum::http::StatusCode;

fn escape_json_for_script(json: &str) -> String {
    // Prevent </script> from ending the script tag when embedding JSON directly in HTML
    json.replace("</script>", "<\\/script>")
}

async fn export_script(Json(index): Json<JdIndex>) -> impl IntoResponse {
    let errors = jd::validate_index(&index);
    if !errors.is_empty() {
        let msg = format!("Validation failed:\n{}", errors.join("\n"));
        return (StatusCode::BAD_REQUEST, msg);
    }

    let mut script = String::from(format!("{}echo 'Updating JD Library...'\n\n", SHELL_HEADER));
    script.push_str(&format!("mkdir -p {}\n\n", jd::shell_quote(SHELF_DIR)));
    
    for item in index.items.iter() {
        if item.physical_loc > 0 && item.level == "category" {
            if item.id.ends_with('0') {
                continue;
            }

            let slot_index = item.physical_loc - 1;
            let new_id = (slot_index / SLOTS_PER_SHELF + 1) * SLOTS_PER_SHELF + (slot_index % SLOTS_PER_SHELF);
            let safe_title = item.title.replace('/', "_");
            let new_name = format!("{:02} - {} {}", item.physical_loc, new_id, safe_title);

            let from = jd::shell_quote(&item.original_path);
            let to = jd::shell_quote(&format!("{}/{}", SHELF_DIR, new_name));
            script.push_str(&format!("mv {} {}\n", from, to));
        }
    }
    script.push_str("\necho 'Done. Look in the ./shelf/ folder.'\n");
    (StatusCode::OK, script)
}

async fn render_tidy_ui_handler(State(renames): State<Arc<Vec<RenameItem>>>) -> impl IntoResponse {
    Html(generate_tidy_html(&renames))
}

fn generate_tidy_html(items: &[RenameItem]) -> String {
    // Embed the JSON inside a <script type="application/json"> tag and read it via textContent.
    let json_data = serde_json::to_string(items).unwrap();
    let safe_json = escape_json_for_script(&json_data);
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
    <button id="exportBtn">Export Rename Script</button>

    <div id="list"></div>
    <div id="output"></div>

    <script id="tidy-data" type="application/json">JSON_DATA_HERE</script>
    <script>
        const items = JSON.parse(document.getElementById('tidy-data').textContent || '[]');
        const list = document.getElementById('list');
        const exportBtn = document.getElementById('exportBtn');

        function renderList() {
            list.innerHTML = '';
            if (items.length === 0) {
                const p = document.createElement('div');
                p.style.color = '#888';
                p.textContent = '(No files to tidy)';
                list.appendChild(p);
                return;
            }

            items.forEach((item, index) => {
                const row = document.createElement('div');
                row.className = 'file-row';

                const oldDiv = document.createElement('div');
                oldDiv.className = 'old-name';
                oldDiv.textContent = item.old_path;

                const input = document.createElement('input');
                input.type = 'text';
                input.id = `input-${index}`;
                input.value = item.new_name;

                row.appendChild(oldDiv);
                row.appendChild(input);
                list.appendChild(row);
            });
        }

        async function generateScript() {
            exportBtn.textContent = 'Generating...';
            items.forEach((item, index) => {
                const el = document.getElementById(`input-${index}`);
                if (el) item.new_name = el.value;
            });
            try {
                const res = await fetch('/api/export_tidy', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(items)
                });
                const scriptText = await res.text();
                const output = document.getElementById('output');
                output.textContent = scriptText;
                output.style.display = 'block';
                exportBtn.textContent = 'Export Rename Script';
            } catch (e) {
                alert('Failed to generate script.');
                exportBtn.textContent = 'Export Rename Script';
            }
        }

        exportBtn.addEventListener('click', generateScript);
        renderList();
    </script>
</body>
</html>
"##;
    template.replace("JSON_DATA_HERE", &safe_json)
}

fn generate_html(json_data: &str) -> String {
    let safe_json = escape_json_for_script(json_data);
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
        
        .slot { flex: 1; min-height: 60px; border: 1px dashed #333; margin-bottom: 8px; border-radius: 4px; display: flex; flex-direction: column; justify-content: center; cursor: pointer; padding: 4px; }
        .slot:hover { background: #2a2a2a; }
        .slot.locked { cursor: default; border-style: solid; }
        .slot.locked:hover { background: transparent; }
        
        .item { background: #333; padding: 8px; border-radius: 4px; width: 100%; border-left: 4px solid #00ccff; font-size: 12px; box-sizing: border-box; display: flex; justify-content: space-between; }
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

        #stats-bar { position: fixed; bottom: 0; left: 0; width: 100%; background: #222; border-top: 2px solid #444; color: #888; font-size: 12px; display: flex; justify-content: space-around; padding: 8px 0; }
        #stats-bar span { color: #fff; font-weight: bold; }

        #modal { display: none; position: fixed; top: 0; left: 0; width: 100%; height: 100%; background: rgba(17,17,17,0.95); z-index: 100; flex-direction: column; padding: 15px; box-sizing: border-box; }
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
        <button class="export" id="exportBtn">Export</button>
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
        const state = JSON.parse(document.getElementById('state-data').textContent || '{"items":[]}');

        const SHELF_COUNT = __SHELF_COUNT__;
        const SLOTS_PER_SHELF = __SLOTS_PER_SHELF__;

        const areas = [...new Set(state.items.filter(i => i.level === 'area' && !i.title.includes('System')).map(i => i.original_path))].sort();
        let shelfPaths = [areas[0], areas[1], areas[2]];
        let selectedItemId = null;

        state.items.forEach(i => { if (i.title) i.title = i.title.replace(/\s+/g, ' ').trim(); });

        function updateStats() {
            document.getElementById('stat-areas').textContent = state.items.filter(i => i.level === 'area').length;
            document.getElementById('stat-cats').textContent = state.items.filter(i => i.level === 'category').length;
            document.getElementById('stat-ids').textContent = state.items.filter(i => i.level === 'id').length;
            document.getElementById('stat-files').textContent = state.items.filter(i => i.is_file).length;
        }

        function getAreaClass(areaPath) {
            if (!areaPath) return "";
            if (areaPath.includes("Personal")) return "area-personal";
            if (areaPath.includes("CS")) return "area-cs";
            if (areaPath.includes("Math")) return "area-math";
            if (areaPath.includes("System")) return "area-system";
            return "";
        }

        function createText(tag, className, text) {
            const el = document.createElement(tag);
            if (className) el.className = className;
            el.textContent = text;
            return el;
        }

        function getDisplayId(item) {
            if (item.physical_loc > 0 && (item.level === 'category' || item.level === 'id')) {
                const slot_index = item.physical_loc - 1;
                const decade = Math.floor(slot_index / SLOTS_PER_SHELF) + 1;
                const row = (slot_index % SLOTS_PER_SHELF);
                if (item.level === 'category') {
                    return `${decade}${row}`;
                } else {
                    return `${decade}${row}.${item.id.split('.')[1] || '00'}`;
                }
            }
            return item.id;
        }

        function render() {
            updateStats();
            const metaList = document.getElementById('meta-list');
            metaList.innerHTML = '';
            const sysArea = state.items.find(i => i.level === 'area' && i.title.includes('System'));
            if (sysArea) {
                const sysChildren = state.items.filter(i => i.parent_path === sysArea.original_path);
                sysChildren.sort((a,b) => a.id.localeCompare(b.id));
                sysChildren.forEach(item => metaList.appendChild(createItemElement(item, false)));
            }

            const pool = document.getElementById('pool');
            // clear pool but keep title
            Array.from(pool.children).forEach((c, idx) => { if (!c.classList || !c.classList.contains('pool-title')) c.remove(); });
            const unassigned = state.items.filter(i => i.physical_loc === 0 && (i.level === 'category' || i.level === 'id') && (!sysArea || i.area !== sysArea.original_path));
            if (unassigned.length === 0) {
                const span = createText('div', null, '(All placed)');
                span.style.color = '#444';
                pool.appendChild(span);
            } else {
                unassigned.forEach(item => pool.appendChild(createItemElement(item, false)));
            }

            const grid = document.getElementById('grid');
            grid.innerHTML = '';

            for (let s = 0; s < SHELF_COUNT; s++) {
                const shelfContainer = document.createElement('div');
                shelfContainer.className = 'shelf';
                const areaPath = shelfPaths[s];
                if (!areaPath) continue;

                const shelfBox = document.createElement('div');
                shelfBox.className = 'shelf-box';

                const header = document.createElement('div');
                header.className = 'shelf-header';
                const select = document.createElement('select');
                const defaultOpt = document.createElement('option');
                defaultOpt.value = '';
                defaultOpt.textContent = '-- Select Area --';
                select.appendChild(defaultOpt);
                areas.forEach(area => {
                    const opt = document.createElement('option');
                    opt.value = area;
                    opt.textContent = area.split('/').pop();
                    if (area === areaPath) opt.selected = true;
                    select.appendChild(opt);
                });
                select.onchange = (e) => {
                    shelfPaths[s] = e.target.value;
                    render();
                };
                header.appendChild(select);
                shelfBox.appendChild(header);

                const allChildren = state.items.filter(i => i.parent_path === areaPath);
                let mgmtItem = allChildren.find(i => i.level === 'category' && i.id.endsWith('0'));
                let physicalItems = allChildren.filter(i => i !== mgmtItem && (i.level === 'category' || i.level === 'id')).sort((a,b) => a.id.localeCompare(b.id));
                let itemsToShow = physicalItems.filter(i => i.level === 'category');
                if (itemsToShow.length === 0) {
                    itemsToShow = physicalItems.filter(i => i.level === 'id');
                }

                for (let r = 0; r < SLOTS_PER_SHELF; r++) {
                    const slotIndex = s * SLOTS_PER_SHELF + r;
                    const slot = document.createElement('div');
                    slot.className = 'slot';
                    if (r === 0) {
                        slot.classList.add('locked');
                        if (mgmtItem) {
                            slot.appendChild(createItemElement(mgmtItem, true));
                        } else {
                            const span = createText('span', null, 'Row 1 (Mgmt)');
                            span.style.color = '#444';
                            span.style.fontSize = '10px';
                            slot.appendChild(span);
                        }
                    } else {
                        slot.addEventListener('click', () => placeItem(slotIndex));
                        let item = itemsToShow.find(i => i.physical_loc === slotIndex + 1);
                        if (item) {
                            slot.appendChild(createItemElement(item, false));
                        } else {
                            const span = createText('span', null, `Row ${r + 1}`);
                            span.style.color = '#444';
                            span.style.fontSize = '10px';
                            slot.appendChild(span);
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

            const idSpan = document.createElement('span');
            idSpan.className = 'id';
            idSpan.textContent = getDisplayId(item);

            const titleSpan = document.createElement('span');
            titleSpan.className = 'title';
            titleSpan.textContent = item.title;

            content.appendChild(idSpan);
            content.appendChild(titleSpan);

            content.addEventListener('click', (e) => { e.stopPropagation(); openModal(item); });
            div.appendChild(content);

            if (!isLocked) {
                const selBtn = document.createElement('button');
                selBtn.className = 'btn-select';
                selBtn.innerText = '✥';
                selBtn.title = 'Select to move';
                selBtn.addEventListener('click', (e) => { e.stopPropagation(); selectedItemId = (selectedItemId === item.id) ? null : item.id; render(); });
                div.appendChild(selBtn);
            }

            return div;
        }

        function placeItem(slotIndex) {
            if (!selectedItemId) return;
            if (slotIndex % SLOTS_PER_SHELF === 0) return;

            const item = state.items.find(i => i.id === selectedItemId);
            if (!item) return;

            if (slotIndex === -1) {
                item.physical_loc = 0;
                selectedItemId = null;
                render();
                return;
            }

            const targetShelf = Math.floor(slotIndex / SLOTS_PER_SHELF);
            const targetAreaPath = shelfPaths[targetShelf];
            if (item.area !== targetAreaPath) {
                alert('You cannot move an item to a shelf of a different Area.');
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
            document.getElementById('modal-title').textContent = item.id ? `${item.id} ${item.title}` : item.title;
            const content = document.getElementById('modal-content');
            content.innerHTML = '';
            const children = state.items.filter(i => i.parent_path === item.original_path);
            children.sort((a,b) => {
                if (a.is_file !== b.is_file) return a.is_file ? 1 : -1;
                return a.id.localeCompare(b.id);
            });
            if (children.length === 0) {
                const p = createText('p', null, 'No subfolders or files found here.');
                p.style.color = '#888';
                content.appendChild(p);
                return;
            }

            children.forEach(child => {
                const div = document.createElement('div');
                if (child.is_file) {
                    div.className = 'file-item';
                    div.textContent = `📄 ${child.title}`;
                } else {
                    div.className = 'folder-item';
                    div.textContent = `📁 ${child.id ? child.id + ' ' : ''}${child.title}`;
                    div.addEventListener('click', () => openModal(child));
                }
                content.appendChild(div);
            });
        }

        function closeModal() { document.getElementById('modal').style.display = 'none'; }

        async function generateScript() {
            const btn = document.getElementById('exportBtn');
            btn.innerText = '...';
            try {
                const res = await fetch('/api/export', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify(state)
                });
                const scriptText = await res.text();
                const output = document.getElementById('output');
                output.textContent = scriptText;
                output.style.display = 'block';
                btn.innerText = 'Export';
                window.scrollTo(0, document.body.scrollHeight);
            } catch (e) {
                alert('Failed to generate script.');
                btn.innerText = 'Export';
            }
        }

        document.getElementById('exportBtn').addEventListener('click', generateScript);
        // replace placeholders with numeric constants (from Rust)
        (function injectConstants(){
            const txt = document.documentElement.innerHTML;
        })();

        render();
    </script>
</body>
</html>
"##;
    // inject numeric constants so the JS runs without relying on undefined identifiers
    template
        .replace("JSON_DATA_HERE", &safe_json)
        .replace("__SHELF_COUNT__", &SHELF_COUNT.to_string())
        .replace("__SLOTS_PER_SHELF__", &SLOTS_PER_SHELF.to_string())
}
