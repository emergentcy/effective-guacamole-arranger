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
    let json_data = serde_json::to_string(items).unwrap();
    let safe_json = escape_json_for_script(&json_data);
    let template = r##"
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <title>JD Tidy</title>
</head>
<body>
  <header>
    <h1>JD Tidy</h1>
    <div>
      <button id="exportBtn" type="button">Export Rename Script</button>
    </div>
  </header>

  <main>
    <ul id="tidy-list" role="list"></ul>
    <pre id="output" hidden></pre>
  </main>

  <script id="tidy-data" type="application/json">JSON_DATA_HERE</script>
  <script>
  (function(){
    const items = JSON.parse(document.getElementById('tidy-data').textContent || '[]');
    const list = document.getElementById('tidy-list');
    function makeNode(item, idx){
      const li = document.createElement('li');
      li.dataset.idx = String(idx);

      const old = document.createElement('div');
      old.textContent = item.old_path;

      const input = document.createElement('input');
      input.value = item.new_name || '';

      const btn = document.createElement('button');
      btn.textContent = 'Apply';
      btn.type = 'button';
      btn.addEventListener('click', () => { item.new_name = input.value; });

      li.appendChild(old);
      li.appendChild(document.createTextNode(' '));
      li.appendChild(input);
      li.appendChild(document.createTextNode(' '));
      li.appendChild(btn);
      return li;
    }
    items.forEach((it,i) => list.appendChild(makeNode(it,i)));
    document.getElementById('exportBtn').addEventListener('click', async ()=>{
      const res = await fetch('/api/export_tidy', {
        method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify(items)
      });
      const txt = await res.text();
      const out = document.getElementById('output'); out.textContent = txt; out.hidden = false;
    });
  })();
  </script>
</body>
</html>
"##;
    template.replace("JSON_DATA_HERE", &safe_json)
}

fn generate_html(json_data: &str) -> String {
    let safe_json = escape_json_for_script(json_data);
    let template = r##"
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <title>JD Wrangler</title>
</head>
<body>
  <header>
    <h1 id="app-title">JD Wrangler</h1>
    <div id="controls">
      <button id="exportBtn" type="button">Export</button>
    </div>
  </header>

  <main id="app">
    <section id="system-management" aria-labelledby="sys-heading">
      <h2 id="sys-heading">00-09 System Management</h2>
      <ul id="meta-list" role="list"></ul>
    </section>

    <section id="unassigned" aria-labelledby="pool-heading">
      <h2 id="pool-heading">Unassigned Items</h2>
      <ul id="pool" role="list"></ul>
    </section>

    <section id="shelves" aria-labelledby="shelves-heading">
      <h2 id="shelves-heading" hidden>Library Shelves</h2>
      <div id="shelf-list" role="list"></div>
    </section>
  </main>

  <footer>
    <div id="stats">
      <span>Areas: <strong id="stat-areas">0</strong></span>
      <span>Categories: <strong id="stat-cats">0</strong></span>
      <span>IDs: <strong id="stat-ids">0</strong></span>
      <span>Files: <strong id="stat-files">0</strong></span>
    </div>
  </footer>

  <script id="state-data" type="application/json">JSON_DATA_HERE</script>

  <script>
  // Minimal, style-free renderer. Renders semantic lists and uses data-* attributes.
  (function(){
    const state = JSON.parse(document.getElementById('state-data').textContent || '{"items":[]}');
    const SHELF_COUNT = __SHELF_COUNT__;
    const SLOTS_PER_SHELF = __SLOTS_PER_SHELF__;

    function qs(id){ return document.getElementById(id); }

    function makeItemNode(item){
      const li = document.createElement('li');
      li.setAttribute('role','listitem');
      li.dataset.jdid = item.id || '';
      li.dataset.level = item.level || '';
      li.dataset.physical = String(item.physical_loc || 0);
      li.dataset.path = item.original_path || '';

      const idSpan = document.createElement('span');
      idSpan.dataset.part = 'id';
      idSpan.textContent = item.id || '';

      const titleSpan = document.createElement('span');
      titleSpan.dataset.part = 'title';
      titleSpan.textContent = item.title || item.original_path || '';

      const btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = 'Select';
      btn.addEventListener('click', () => {
        if (li.hasAttribute('data-selected')) li.removeAttribute('data-selected');
        else li.setAttribute('data-selected', '1');
      });

      li.appendChild(idSpan);
      li.appendChild(document.createTextNode(' '));
      li.appendChild(titleSpan);
      li.appendChild(document.createTextNode(' '));
      li.appendChild(btn);
      return li;
    }

    function renderMeta(){
      const metaList = qs('meta-list');
      metaList.innerHTML = '';
      const sysArea = state.items.find(i => i.level === 'area' && i.title && i.title.includes('System'));
      if (!sysArea) return;
      const children = state.items.filter(i => i.parent_path === sysArea.original_path);
      children.forEach(c => metaList.appendChild(makeItemNode(c)));
    }

    function renderPool(){
      const pool = qs('pool');
      pool.innerHTML = '';
      const sysArea = state.items.find(i => i.level === 'area' && i.title && i.title.includes('System'));
      const unassigned = state.items.filter(i => (i.level === 'category' || i.level === 'id') && i.physical_loc === 0 && (!sysArea || i.area !== sysArea.original_path));
      unassigned.forEach(u => pool.appendChild(makeItemNode(u)));
    }

    function renderShelves(){
      const shelfList = qs('shelf-list');
      shelfList.innerHTML = '';
      for (let s = 0; s < SHELF_COUNT; s++){
        const section = document.createElement('section');
        section.dataset.shelfIndex = String(s);
        const heading = document.createElement('h3');
        heading.textContent = `Shelf ${s+1}`;
        section.appendChild(heading);

        const ol = document.createElement('ol');
        ol.setAttribute('role','list');
        ol.dataset.slotCount = String(SLOTS_PER_SHELF);

        for (let r = 0; r < SLOTS_PER_SHELF; r++){
          const slotIndex = s * SLOTS_PER_SHELF + r;
          const li = document.createElement('li');
          li.dataset.slotIndex = String(slotIndex + 1);
          li.dataset.row = String(r + 1);
          const item = state.items.find(i => i.physical_loc === slotIndex + 1 && (i.level === 'category' || i.level === 'id'));
          if (item) li.appendChild(makeItemNode(item));
          ol.appendChild(li);
        }
        section.appendChild(ol);
        shelfList.appendChild(section);
      }
    }

    function updateStats(){
      qs('stat-areas').textContent = String(state.items.filter(i=>i.level==='area').length);
      qs('stat-cats').textContent = String(state.items.filter(i=>i.level==='category').length);
      qs('stat-ids').textContent = String(state.items.filter(i=>i.level==='id').length);
      qs('stat-files').textContent = String(state.items.filter(i=>i.is_file).length);
    }

    qs('exportBtn').addEventListener('click', async () => {
      const res = await fetch('/api/export', {
        method: 'POST',
        headers: {'Content-Type': 'application/json'},
        body: JSON.stringify(state)
      });
      const txt = await res.text();
      alert(txt);
    });

    renderMeta();
    renderPool();
    renderShelves();
    updateStats();
  })();
  </script>
</body>
</html>
"##;
    template
        .replace("JSON_DATA_HERE", &safe_json)
        .replace("__SHELF_COUNT__", &SHELF_COUNT.to_string())
        .replace("__SLOTS_PER_SHELF__", &SLOTS_PER_SHELF.to_string())
}
