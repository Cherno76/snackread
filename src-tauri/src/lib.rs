use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri::Emitter;
use std::fs;
use std::io::{Read, Write};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};
use std::cmp::Ordering;
use regex::Regex;
use base64::Engine;

mod db;

/// 把路径统一成正斜杠字符串（前端用 `/` 切分路径；macOS 上无副作用）
fn norm_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// 用户配置目录：macOS ~/Library/Application Support，Windows %APPDATA%
fn app_config_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    let base = android_home().join(".config");
    #[cfg(not(target_os = "android"))]
    let base = dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
    base.join("cshow-gui")
}

/// Android 应用内部存储根目录：/data/user/0/<包名>/files
/// （Tauri Android 运行时不保证设置 $HOME，因此给出确定性的兜底路径）
#[cfg(target_os = "android")]
fn android_home() -> PathBuf {
    std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/user/0/com.cherno.cshow_gui/files"))
}

#[derive(Serialize)]
struct FsEntry {
    name: String,
    path: String,
    is_dir: bool,
    dir_type: String, // 文件夹内容类型（图片/CBZ/CBR/EPUB/TXT/PDF），空=无；散装文件为空
    is_image: bool,
    is_pdf: bool,
    is_epub: bool,
    is_txt: bool,
    is_ebook: bool,
    auto_ebook: bool,
    is_hidden: bool,
    size: u64,
    modified: u64,
}

/// 文件夹内容类型：递归扫描书文件夹与卷子文件夹内的文件（带深度/数量上限），
/// 用于给文件夹书（漫画等）显示类型胶囊。优先级：CBZ > CBR > 图片 > EPUB > TXT > PDF
fn dir_content_type(dir: &Path) -> String {
    let mut found = [false; 6]; // cbz, cbr, img, epub, txt, pdf
    let mut scanned = 0usize;
    fn scan(dir: &Path, depth: usize, found: &mut [bool; 6], scanned: &mut usize) {
        if depth > 3 || *scanned > 500 {
            return;
        }
        if let Ok(rd) = fs::read_dir(dir) {
            for item in rd.flatten() {
                if *scanned > 500 {
                    return;
                }
                let p = item.path();
                if p.file_name()
                    .map(|n| n.to_string_lossy().starts_with('.'))
                    .unwrap_or(false)
                {
                    continue;
                }
                *scanned += 1;
                if p.is_dir() {
                    scan(&p, depth + 1, found, scanned);
                } else if is_image(&p) {
                    found[2] = true;
                } else if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                    match ext.to_ascii_lowercase().as_str() {
                        "cbz" => found[0] = true,
                        "cbr" => found[1] = true,
                        "epub" => found[3] = true,
                        "txt" => found[4] = true,
                        "pdf" => found[5] = true,
                        _ => {}
                    }
                }
            }
        }
    }
    scan(dir, 0, &mut found, &mut scanned);
    if found[0] {
        return "CBZ".into();
    }
    if found[1] {
        return "CBR".into();
    }
    if found[2] {
        return "图片".into();
    }
    if found[3] {
        return "EPUB".into();
    }
    if found[4] {
        return "TXT".into();
    }
    if found[5] {
        return "PDF".into();
    }
    String::new()
}

fn dir_is_ebook(conn: &rusqlite::Connection, dir: &Path) -> bool {
    db::get_book(conn, &norm_path(dir))
        .ok()
        .flatten()
        .map(|b| b.is_ebook)
        .unwrap_or(false)
}

/// 从目录向上找到最近一个「电子书根」：书库根 或 标记为电子书的目录（含自身）
#[tauri::command]
fn ebook_root(state: tauri::State<'_, db::Db>, dir: String) -> Option<String> {
    let conn = state.0.lock().unwrap();
    let mut p = PathBuf::from(&dir);
    loop {
        let np = norm_path(&p);
        if db::get_library(&conn, &np).ok().flatten().is_some() || dir_is_ebook(&conn, &p) {
            return Some(np);
        }
        if !p.pop() {
            return None;
        }
    }
}

fn path_is_hidden(conn: &rusqlite::Connection, path: &Path) -> bool {
    let np = norm_path(path);
    if path.is_dir() {
        if let Ok(Some(lib)) = db::get_library(conn, &np) {
            return lib.hidden;
        }
        db::get_book(conn, &np).ok().flatten().map(|b| b.hidden).unwrap_or(false)
    } else {
        db::get_book(conn, &np).ok().flatten().map(|b| b.hidden).unwrap_or(false)
    }
}

fn is_image(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some(
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "avif"
                | "svg" | "heic" | "heif" | "jxl" | "qoi"
        )
    )
}

fn recent_read_time(conn: &rusqlite::Connection, p: &Path) -> u64 {
    db::get_book(conn, &norm_path(p))
        .ok()
        .flatten()
        .map(|b| b.last_read_at)
        .unwrap_or(0)
}

/// 自然排序：数字段按数值比较（卷2 < 卷10）
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x.is_ascii_digit() && y.is_ascii_digit() {
                    let mut sa = String::new();
                    let mut sb = String::new();
                    while let Some(&c) = ai.peek() {
                        if c.is_ascii_digit() { sa.push(c); ai.next(); } else { break; }
                    }
                    while let Some(&c) = bi.peek() {
                        if c.is_ascii_digit() { sb.push(c); bi.next(); } else { break; }
                    }
                    let ord = num_cmp(&sa, &sb);
                    if ord != Ordering::Equal {
                        return ord;
                    }
                } else {
                    let xl = x.to_lowercase().next().unwrap_or(x);
                    let yl = y.to_lowercase().next().unwrap_or(y);
                    if xl != yl {
                        return xl.cmp(&yl);
                    }
                    ai.next();
                    bi.next();
                }
            }
        }
    }
}

fn num_cmp(a: &str, b: &str) -> Ordering {
    let ta = a.trim_start_matches('0');
    let tb = b.trim_start_matches('0');
    let ea = if ta.is_empty() { "0" } else { ta };
    let eb = if tb.is_empty() { "0" } else { tb };
    ea.len().cmp(&eb.len()).then_with(|| ea.cmp(eb))
}

fn is_favorite_dir(conn: &rusqlite::Connection, dir: &Path) -> bool {
    db::get_library(conn, &norm_path(dir)).ok().flatten().is_some()
}

/// 书库（收藏）文件夹的下一级子文件夹自动标记为电子书
fn ensure_ebook_marks(conn: &rusqlite::Connection, parent: &Path) {
    if let Ok(rd) = fs::read_dir(parent) {
        for item in rd.flatten() {
            let p = item.path();
            if !p.is_dir() {
                continue;
            }
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let np = norm_path(&p);
            if dir_is_ebook(conn, &p) {
                continue; // 已标记的跳过
            }
            let _ = db::ensure_book(conn, &np, "dir");
            let _ = db::set_book_is_ebook(conn, &np, true);
        }
    }
}

#[tauri::command]
fn list_dir(state: tauri::State<'_, db::Db>, path: String) -> Result<Vec<FsEntry>, String> {
    let conn = state.0.lock().unwrap();
    let parent_fav = is_favorite_dir(&conn, Path::new(&path));
    if parent_fav {
        ensure_ebook_marks(&conn, Path::new(&path));
    }
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for item in fs::read_dir(&path).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let p = item.path();
        let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let meta = item.metadata().ok();
        let size = if is_dir {
            0
        } else {
            meta.as_ref().map(|m| m.len()).unwrap_or(0)
        };
        let modified = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let entry = FsEntry {
            name: name.clone(),
            path: norm_path(&p),
            is_dir,
            dir_type: if is_dir { dir_content_type(&p) } else { String::new() },
            is_image: !is_dir && is_image(&p),
            is_pdf: !is_dir
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("pdf"))
                    .unwrap_or(false),
            is_epub: !is_dir
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("epub"))
                    .unwrap_or(false),
            is_txt: !is_dir
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("txt"))
                    .unwrap_or(false),
            is_ebook: is_dir && dir_is_ebook(&conn, &p),
            auto_ebook: is_dir && parent_fav,
            is_hidden: path_is_hidden(&conn, &p),
            size,
            modified,
        };
        if is_dir {
            dirs.push(entry);
        } else {
            files.push(entry);
        }
    }
    dirs.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    files.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    let mut all = dirs;
    all.extend(files);
    // 收藏处于 eye-off 时：从列表中隐藏标记为 eye-off 的子文件夹
    if is_favorite_dir(&conn, Path::new(&path)) && path_is_hidden(&conn, Path::new(&path)) {
        all.retain(|e| !e.is_hidden);
    }
    // 当前文件夹是个人收藏时：子文件夹按最近阅读时间排序
    if is_favorite_dir(&conn, Path::new(&path)) {
        all.sort_by(|a, b| {
            let ta = recent_read_time(&conn, Path::new(&a.path));
            let tb = recent_read_time(&conn, Path::new(&b.path));
            tb.cmp(&ta).then_with(|| natural_cmp(&a.name, &b.name))
        });
    }
    Ok(all)
}

#[tauri::command]
fn save_position(state: tauri::State<'_, db::Db>, dir: String, name: String, page: u32) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_dir_state(&conn, &dir, &name, page)
}

#[tauri::command]
fn read_position(state: tauri::State<'_, db::Db>, dir: String) -> Result<String, String> {
    let conn = state.0.lock().unwrap();
    match db::get_dir_state(&conn, &dir)? {
        Some((current, page)) => Ok(serde_json::json!({ "current": current, "page": page }).to_string()),
        None => Ok(String::new()),
    }
}

/// 切换电子书标记，返回切换后的状态
#[tauri::command]
fn toggle_ebook(state: tauri::State<'_, db::Db>, dir: String) -> Result<bool, String> {
    let conn = state.0.lock().unwrap();
    let on = dir_is_ebook(&conn, Path::new(&dir));
    db::ensure_book(&conn, &dir, "dir")?;
    db::set_book_is_ebook(&conn, &dir, !on)?;
    Ok(!on)
}

/// 简单的稳定哈希（FNV-1a + 固定盐），用于本地 eye 密码校验
fn eye_password_hash(s: &str) -> String {
    const SALT: &[u8] = b"cshow-gui-eye-v1";
    let mut h: u64 = 0xcbf29ce484222325;
    for b in SALT.iter().chain(s.as_bytes()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

/// 切换缩略图显隐（eye/eye-off），返回切换后的隐藏状态。
/// 传 password：书库级切换（仅目录）——隐藏时设置密码，取消隐藏时校验密码并清除。
/// 不传 password：普通切换（列表/单书 eye 等）。散装文件只做普通切换。
#[tauri::command]
fn toggle_eye(state: tauri::State<'_, db::Db>, path: String, password: Option<String>) -> Result<bool, String> {
    let conn = state.0.lock().unwrap();
    let target = Path::new(&path);
    let np = norm_path(target);

    if target.is_dir() {
        // 书库级（在 libraries 表）
        if db::get_library(&conn, &np)?.is_some() {
            let hidden = db::get_library(&conn, &np)?.map(|l| l.hidden).unwrap_or(false);
            if let Some(pwd) = password {
                if !hidden {
                    // eye → eye-off：设置密码后隐藏（密码可留空 = 不设密码）
                    let stored = if pwd.is_empty() {
                        None
                    } else {
                        Some(eye_password_hash(&pwd))
                    };
                    db::set_library_hidden(&conn, &np, true)?;
                    db::set_library_eye_password(&conn, &np, stored.as_deref())?;
                } else {
                    // eye-off → eye：校验密码，正确则切换并清除密码
                    let stored = db::get_library_eye_password(&conn, &np)?.unwrap_or_default();
                    if stored.is_empty() || stored != eye_password_hash(&pwd) {
                        return Err("密码错误".into());
                    }
                    db::set_library_hidden(&conn, &np, false)?;
                    db::set_library_eye_password(&conn, &np, None)?;
                }
                return Ok(!hidden);
            }
            db::set_library_hidden(&conn, &np, !hidden)?;
            return Ok(!hidden);
        }
        // 目录书
        let hidden = db::get_book(&conn, &np)?.map(|b| b.hidden).unwrap_or(false);
        db::ensure_book(&conn, &np, "dir")?;
        db::set_book_hidden(&conn, &np, !hidden)?;
        Ok(!hidden)
    } else {
        // 散装文件：切换自身隐藏状态（不涉及密码）
        let hidden = db::get_book(&conn, &np)?.map(|b| b.hidden).unwrap_or(false);
        let kind = match target.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("pdf") => "pdf",
            _ => "epub",
        };
        db::ensure_book(&conn, &np, kind)?;
        db::set_book_hidden(&conn, &np, !hidden)?;
        Ok(!hidden)
    }
}

fn cwd_file() -> PathBuf {
    app_config_dir().join("cwd")
}

const DEFAULT_WIN_W: f64 = 1350.0;
const DEFAULT_WIN_H: f64 = 845.0;

fn window_state_file() -> PathBuf {
    app_config_dir().join("window.json")
}

fn reader_theme_file() -> PathBuf {
    app_config_dir().join("reader-theme")
}

fn save_window_size(conn: &rusqlite::Connection, w: f64, h: f64) {
    let _ = db::set_app_state(conn, "window", &serde_json::json!({ "width": w, "height": h }).to_string());
}

fn load_window_size(conn: &rusqlite::Connection) -> Option<(f64, f64)> {
    db::get_app_state(conn, "window")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| Some((v.get("width")?.as_f64()?, v.get("height")?.as_f64()?)))
}

fn favorites_file() -> PathBuf {
    app_config_dir().join("favorites.json")
}

/// 一个书库的持久化数据：路径 + 别名 + 图标（数组顺序即显示顺序）
#[derive(Serialize, serde::Deserialize, Clone, Default)]
struct LibraryEntry {
    path: String,
    #[serde(default)]
    alias: String,
    #[serde(default)]
    icon: String,
}

/// 读取旧版 favorites.json（仅迁移用），兼容旧版纯路径字符串数组格式
fn read_favorites_file() -> Vec<LibraryEntry> {
    let Ok(s) = fs::read_to_string(favorites_file()) else { return Vec::new() };
    if let Ok(list) = serde_json::from_str::<Vec<LibraryEntry>>(&s) {
        return list;
    }
    if let Ok(paths) = serde_json::from_str::<Vec<String>>(&s) {
        return paths
            .into_iter()
            .map(|path| LibraryEntry { path, ..Default::default() })
            .collect();
    }
    Vec::new()
}


#[derive(Serialize)]
struct FavoriteEntry {
    path: String,
    alias: String,
    icon: String,
    hidden: bool,
    has_password: bool,
}

#[tauri::command]
fn list_favorites(state: tauri::State<'_, db::Db>) -> Vec<FavoriteEntry> {
    let conn = state.0.lock().unwrap();
    db::list_libraries(&conn)
        .unwrap_or_default()
        .into_iter()
        .map(|l| FavoriteEntry {
            hidden: l.hidden,
            has_password: l.has_password,
            path: l.path.replace('\\', "/"),
            alias: l.alias,
            icon: l.icon,
        })
        .collect()
}

// ---- 书籍元数据（标题/作者/评分/标签）----

/// 一本书的自定义元数据（标题/作者/评分/标签/备注）
#[derive(Serialize, serde::Deserialize, Clone, Default)]
struct BookMeta {
    #[serde(default)]
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    rating: f64, // 0..5，0 表示未评分
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    note: String,
}

fn meta_from_json(v: Option<&serde_json::Value>) -> BookMeta {
    let Some(v) = v else { return BookMeta::default() };
    BookMeta {
        title: v.get("title").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        author: v.get("author").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        rating: v.get("rating").and_then(|x| x.as_f64()).unwrap_or(0.0),
        tags: v.get("tags")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        note: v.get("note").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    }
}

fn entry_kind(p: &Path) -> &'static str {
    if p.is_dir() {
        "dir"
    } else {
        match p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("pdf") => "pdf",
            _ => "epub",
        }
    }
}

fn book_meta_from_row(row: &db::BookRow) -> BookMeta {
    BookMeta {
        title: row.title.clone(),
        author: row.author.clone(),
        rating: row.rating,
        tags: serde_json::from_str(&row.tags).unwrap_or_default(),
        note: row.note.clone(),
    }
}

fn read_reading_time(conn: &rusqlite::Connection, path: &Path) -> u64 {
    db::get_book(conn, &norm_path(path))
        .ok()
        .flatten()
        .map(|b| b.read_time)
        .unwrap_or(0)
}

/// 累加某本书的阅读时长（秒），返回累计后的总秒数
#[tauri::command]
fn add_reading_time(state: tauri::State<'_, db::Db>, path: String, seconds: u64) -> Result<u64, String> {
    let conn = state.0.lock().unwrap();
    let kind = entry_kind(Path::new(&path));
    db::ensure_book(&conn, &path, kind)?;
    db::add_read_time(&conn, &path, seconds)
}

// ---- 内置元数据预置库：新书（完全无元数据）按书名自动匹配填充 ----

const META_PRESETS_JSON: &str = include_str!("meta_presets.json");

#[derive(Deserialize, Clone)]
struct MetaPreset {
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    rating: f64,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    note: String,
    #[serde(default)]
    name: String,
}

/// 规范化书名：先转简体，再去括号内容、去空白、转小写（用于预置库匹配）。
/// 《》是书名号，里面的才是书名本体：只去掉书名号本身，保留内容。
fn norm_title_key(s: &str) -> String {
    let s = zhconv::zhconv(s, zhconv::Variant::ZhCN);
    let mut out = String::new();
    let mut depth = 0i32;
    for c in s.trim().chars() {
        match c {
            '[' | '（' | '(' => depth += 1,
            ']' | '）' | ')' => depth = (depth - 1).max(0),
            '《' | '》' => {} // 书名号只作标记，里面的书名要保留
            _ if depth == 0 => out.push(c.to_ascii_lowercase()),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn meta_presets() -> &'static HashMap<String, MetaPreset> {
    static MAP: OnceLock<HashMap<String, MetaPreset>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m: HashMap<String, MetaPreset> = HashMap::new();
        if let Ok(list) = serde_json::from_str::<Vec<MetaPreset>>(META_PRESETS_JSON) {
            for p in list {
                let k = norm_title_key(&p.title);
                if !k.is_empty() {
                    m.entry(k).or_insert_with(|| p.clone());
                }
                if !p.name.is_empty() {
                    let stem = Path::new(&p.name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let k2 = norm_title_key(&stem);
                    if !k2.is_empty() {
                        m.entry(k2).or_insert_with(|| p.clone());
                    }
                }
            }
        }
        m
    })
}

/// 新书自动填元数据：仅当书完全没有元数据（title/author/rating/tags/note 全空）时，
/// 当前元数据是否恰好等于某个预置条目（说明是旧版本自动填充写入的）。
/// 旧版把书名号里的书名剥掉，导致多本同作者的书全被填成同一个预置，
/// 这种“预置填充”元数据允许重新匹配纠正。
fn is_preset_fill(b: &db::BookRow, presets: &HashMap<String, MetaPreset>) -> bool {
    presets.values().any(|p| {
        p.title == b.title
            && p.author == b.author
            && (p.rating - b.rating).abs() < 0.0001
            && serde_json::to_string(&p.tags).map(|t| t == b.tags).unwrap_or(false)
            && p.note == b.note
    })
}

/// 按规范化文件名匹配内置预置库，填入预置数据（不含封面）。
/// 仅对完全没有元数据的书生效；若元数据恰好等于某个预置条目（旧版误填），
/// 也允许重新匹配纠正，匹配不上时清空以免继续显示错误的预置数据。
fn maybe_fill_preset_meta(conn: &rusqlite::Connection, path: &str) {
    let (kind, is_ebook, hidden, read_time, last_vol, last_at, rematch) =
        match db::get_book(conn, path).ok().flatten() {
            Some(b) => {
                let empty = b.title.trim().is_empty()
                    && b.author.trim().is_empty()
                    && b.rating <= 0.0
                    && (b.tags == "[]" || b.tags.trim().is_empty())
                    && b.note.trim().is_empty();
                if !empty {
                    // 非空元数据：仅当恰好等于某个预置条目（旧版自动填充）时才允许重匹配
                    if !is_preset_fill(&b, &meta_presets()) {
                        return;
                    }
                    (b.kind, b.is_ebook, b.hidden, b.read_time, b.last_read_volume, b.last_read_at, true)
                } else {
                    (b.kind, b.is_ebook, b.hidden, b.read_time, b.last_read_volume, b.last_read_at, false)
                }
            }
            None => {
                // 还没有行：按文件类型推断（仅书类文件/目录才建行）
                let ext = Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                let kind = match ext.as_str() {
                    "epub" => "epub",
                    "txt" => "txt",
                    "pdf" => "pdf",
                    _ => "dir",
                };
                (kind.to_string(), kind != "dir", false, 0u64, String::new(), 0u64, false)
            }
        };
    let stem = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let key = norm_title_key(&stem);
    let Some(preset) = meta_presets().get(&key) else {
        // 旧版误填的书找不到正确预置：清空元数据，避免继续显示错误的预置数据
        if rematch {
            let _ = db::upsert_book(
                conn,
                path,
                &kind,
                is_ebook,
                hidden,
                "",
                "",
                0.0,
                "[]",
                "",
                read_time,
                if last_vol.is_empty() { None } else { Some(last_vol.as_str()) },
                last_at,
            );
        }
        return;
    };
    let tags = serde_json::to_string(&preset.tags).unwrap_or_else(|_| "[]".into());
    let _ = db::upsert_book(
        conn,
        path,
        &kind,
        is_ebook,
        hidden,
        &preset.title,
        &preset.author,
        preset.rating,
        &tags,
        &preset.note,
        read_time,
        if last_vol.is_empty() { None } else { Some(last_vol.as_str()) },
        last_at,
    );
}

#[tauri::command]
fn get_book_meta(state: tauri::State<'_, db::Db>, path: String) -> BookMeta {
    let conn = state.0.lock().unwrap();
    maybe_fill_preset_meta(&conn, &path);
    db::get_book(&conn, &path)
        .ok()
        .flatten()
        .map(|r| book_meta_from_row(&r))
        .unwrap_or_default()
}

#[tauri::command]
fn set_book_meta(
    state: tauri::State<'_, db::Db>,
    path: String,
    title: String,
    author: String,
    rating: f64,
    tags: Vec<String>,
    note: String,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    let rating = rating.clamp(0.0, 5.0);
    let tags: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    let note = note.trim().to_string();
    let kind = entry_kind(Path::new(&path));
    db::ensure_book(&conn, &path, kind)?;
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string());
    db::set_book_meta(&conn, &path, &title, &author, rating, &tags_json, &note)
}

// ---- AI 元数据（DeepSeek API，纯 AI 填入） ----

/// AI 获取的结果：只回填表单，不直接写库（用户确认后点保存）。
#[derive(Serialize, Clone, Default)]
struct SmartMeta {
    title: String,
    author: String,
    rating: f64, // 五星值（0..5），0 表示未获取到评分
    rating_note: String,
    tags: Vec<String>,
    note: String,
    message: String,
}

const DEEPSEEK_API_URL: &str = "https://api.deepseek.com/chat/completions";
const DEEPSEEK_MODEL: &str = "deepseek-v4-flash";
const DEEPSEEK_KEY_STATE: &str = "deepseek_api_key";

const LLM_META_SYSTEM: &str = "你是电子书元数据助手。只输出 JSON，不要任何解释、Markdown 或额外文字。";

const LLM_META_TASK: &str = r#"请根据书名和作者（必要时结合你对这本书的知识）输出该书的元数据 JSON，字段与规则如下：
{
  "title": "书名",
  "author": "作者",
  "platform": "最初发表平台，如 起点中文网；不确定则为 null",
  "platform_rating": 9.8,
  "platform_rating_count": "评分人数，如 7.3万人评分；没有则为 null",
  "fallback_rating": 7.2,
  "fallback_rating_source": "豆瓣",
  "tags": ["最多8个"],
  "core_setting": "一句话核心设定",
  "synopsis": "80~150字故事梗概",
  "publish_start": "YYYY-MM-DD",
  "publish_end": "YYYY-MM-DD 或 null",
  "status": "已完结/连载中/未知"
}
规则：
1. 评分优先取发表平台（platform_rating，10分制），平台没有时用其他来源（fallback_rating，10分制）；都没有则 null。
2. 不确定的字段一律 null，绝对不要编造。
3. tags 最多 8 个。
4. core_setting 与 synopsis 可以使用 Markdown 排版（如 **加粗**、要点换行），字符串内换行用 \n，保持 JSON 合法。"#;

#[tauri::command]
fn get_deepseek_key(state: tauri::State<'_, db::Db>) -> String {
    let conn = state.0.lock().unwrap();
    db::get_app_state(&conn, DEEPSEEK_KEY_STATE)
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[tauri::command]
fn set_deepseek_key(state: tauri::State<'_, db::Db>, key: String) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_app_state(&conn, DEEPSEEK_KEY_STATE, key.trim())
}

#[tauri::command]
async fn smart_fetch_meta(
    state: tauri::State<'_, db::Db>,
    path: String,
) -> Result<SmartMeta, String> {
    let (db_title, db_author, api_key) = {
        let conn = state.0.lock().unwrap();
        let b = db::get_book(&conn, &path).ok().flatten();
        let key = db::get_app_state(&conn, DEEPSEEK_KEY_STATE)
            .ok()
            .flatten()
            .unwrap_or_default();
        (
            b.as_ref().map(|b| b.title.clone()).unwrap_or_default(),
            b.as_ref().map(|b| b.author.clone()).unwrap_or_default(),
            key,
        )
    };
    if api_key.trim().is_empty() {
        return Err("未设置 DeepSeek API Key：请先打开顶栏「书库管理」，在「AI 设置」里填写后再试".into());
    }
    let p = path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        llm_fetch_impl(Path::new(&p), &db_title, &db_author, &api_key)
    })
    .await
    .map_err(|e| format!("AI 任务失败: {e}"))?
}

/// 把评分换算为五星值并保留 1 位小数（10 分制 → 5 星制）
fn to_five_star(value: f64, scale: f64) -> f64 {
    if value <= 0.0 || scale <= 0.0 {
        return 0.0;
    }
    let five = value / scale * 5.0;
    (five * 10.0).round() / 10.0
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut n = 0usize;
    for c in s.chars() {
        if n >= max {
            out.push('…');
            break;
        }
        out.push(c);
        n += 1;
    }
    out
}

/// 从文件名猜测书名/作者，如 "大王饶命 (会说话的肘子) (Z-Library).epub"
fn guess_title_author_from_filename(name: &str) -> (String, String) {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name).trim();
    let mut title = String::new();
    let mut author = String::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for c in stem.chars() {
        match c {
            '(' | '（' => {
                if depth == 0 {
                    title.push_str(cur.trim());
                    title.push(' ');
                }
                cur.clear();
                depth += 1;
            }
            ')' | '）' if depth > 0 => {
                let part = cur.trim();
                let lower = part.to_lowercase();
                let junk = lower.contains("z-library")
                    || lower.starts_with("www.")
                    || lower.starts_with("txt")
                    || lower.starts_with("epub")
                    || lower.contains("共") && lower.contains("册")
                    || part.chars().all(|c| c.is_ascii_digit());
                if !part.is_empty() && author.is_empty() && !junk {
                    author = part.to_string();
                }
                cur.clear();
                depth -= 1;
            }
            _ => cur.push(c),
        }
    }
    title.push_str(cur.trim());
    let title = title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    (title, author)
}

/// 从 container.xml 提取 OPF 相对路径
fn opf_rel_from_container(container: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(container).ok()?;
    for node in doc.descendants() {
        if node.has_tag_name("rootfile") {
            if let Some(fp) = node.attribute("full-path") {
                return Some(fp.to_string());
            }
        }
    }
    None
}

/// 读取 EPUB（.epub 压缩包或解包目录）的 container/opf 相对路径/opf 文本
fn read_container_and_opf(path: &Path) -> Option<(String, String, String)> {
    let container = if path.is_dir() {
        fs::read_to_string(path.join("META-INF/container.xml")).ok()?
    } else {
        let file = fs::File::open(path).ok()?;
        let mut zip = zip::ZipArchive::new(file).ok()?;
        let mut c = String::new();
        zip.by_name("META-INF/container.xml")
            .ok()?
            .read_to_string(&mut c)
            .ok()?;
        c
    };
    let opf_rel = opf_rel_from_container(&container)?;
    let opf = if path.is_dir() {
        fs::read_to_string(path.join(&opf_rel)).ok()?
    } else {
        let file = fs::File::open(path).ok()?;
        let mut zip = zip::ZipArchive::new(file).ok()?;
        let mut o = String::new();
        zip.by_name(&opf_rel).ok()?.read_to_string(&mut o).ok()?;
        o
    };
    Some((container, opf_rel, opf))
}

struct OpfBasic {
    title: String,
    authors: Vec<String>,
    lang: String,
    spine: Vec<String>,
}

fn parse_opf_basic(opf: &str) -> OpfBasic {
    let mut out = OpfBasic {
        title: String::new(),
        authors: Vec::new(),
        lang: String::new(),
        spine: Vec::new(),
    };
    let mut manifest: Vec<(String, String)> = Vec::new();
    if let Ok(doc) = roxmltree::Document::parse(opf) {
        for node in doc.descendants() {
            match node.tag_name().name() {
                "title" => {
                    if out.title.is_empty() {
                        out.title = node.text().unwrap_or("").trim().to_string();
                    }
                }
                "creator" => {
                    let t = node.text().unwrap_or("").trim().to_string();
                    if !t.is_empty() && !out.authors.contains(&t) {
                        out.authors.push(t);
                    }
                }
                "language" => {
                    if out.lang.is_empty() {
                        out.lang = node.text().unwrap_or("").trim().to_string();
                    }
                }
                "item" => {
                    let id = node.attribute("id").unwrap_or("").to_string();
                    let href = node.attribute("href").unwrap_or("").to_string();
                    manifest.push((id, href));
                }
                "itemref" => {
                    let idref = node.attribute("idref").unwrap_or("").to_string();
                    if let Some((_, href)) = manifest.iter().find(|(id, _)| *id == idref) {
                        out.spine.push(href.split('#').next().unwrap_or("").to_string());
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// 解析模型输出的元数据 JSON → SmartMeta（评分：发表平台优先，其余兜底，统一转五星）
fn parse_llm_meta(v: &serde_json::Value, fallback_title: &str, fallback_author: &str) -> SmartMeta {
    let get_str = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let get_f64 = |k: &str| v.get(k).and_then(|x| x.as_f64()).filter(|x| *x > 0.0);

    let mut meta = SmartMeta::default();
    meta.title = get_str("title").unwrap_or_else(|| fallback_title.to_string());
    meta.author = get_str("author").unwrap_or_else(|| fallback_author.to_string());

    let platform = get_str("platform");
    let platform_rating = get_f64("platform_rating");
    let fallback_source = get_str("fallback_rating_source");
    let fallback_rating = get_f64("fallback_rating");

    // 评分：发表平台优先，其他兜底；记录全部来源便于人工核对
    let mut rating_parts: Vec<(String, f64, f64)> = Vec::new();
    if let Some(r) = platform_rating {
        rating_parts.push((
            platform.clone().unwrap_or_else(|| "发表平台".to_string()),
            r,
            10.0,
        ));
    }
    if let Some(r) = fallback_rating {
        rating_parts.push((
            fallback_source.unwrap_or_else(|| "其他来源".to_string()),
            r,
            10.0,
        ));
    }
    let rating = rating_parts
        .first()
        .map(|(_, r, s)| to_five_star(*r, *s))
        .unwrap_or(0.0);
    meta.rating = rating;
    if !rating_parts.is_empty() {
        meta.rating_note = rating_parts
            .iter()
            .map(|(src, r, s)| format!("{src} {r:.1}/{s:.0} → {:.1}★", to_five_star(*r, *s)))
            .collect::<Vec<_>>()
            .join("；");
    }

    // 标签：去重、限 8 个
    if let Some(arr) = v.get("tags").and_then(|x| x.as_array()) {
        for t in arr.iter().filter_map(|x| x.as_str()) {
            let t = t.trim().to_string();
            if !t.is_empty() && !meta.tags.contains(&t) {
                meta.tags.push(t);
            }
            if meta.tags.len() >= 8 {
                break;
            }
        }
    }

    // 备注：平台 / 设定 / 梗概 / 评分
    let mut note_parts: Vec<String> = Vec::new();
    let publish_start = get_str("publish_start");
    let publish_end = get_str("publish_end");
    let status = get_str("status");
    if let Some(p) = platform {
        let mut pl = p.clone();
        let mut extra = Vec::new();
        if let Some(s) = &publish_start {
            extra.push(s.clone());
        }
        if let Some(e) = &publish_end {
            extra.push(format!("至 {e}"));
        }
        if let Some(st) = &status {
            extra.push(st.clone());
        }
        if !extra.is_empty() {
            pl.push_str(&format!("（{}）", extra.join("，")));
        }
        note_parts.push(format!("【发表平台】{pl}"));
    }
    if let Some(c) = get_str("core_setting") {
        note_parts.push(format!("【核心设定】{c}"));
    }
    if let Some(s) = get_str("synopsis") {
        note_parts.push(format!("【故事梗概】{}", truncate_chars(&s, 600)));
    }
    if !meta.rating_note.is_empty() {
        note_parts.push(format!("【评分】{}", meta.rating_note));
    }
    meta.note = note_parts.join("\n");

    let mut msg = format!("AI 生成完毕（{DEEPSEEK_MODEL}）");
    if rating <= 0.0 {
        msg.push_str("，模型未给出评分");
    }
    msg.push_str("，请人工确认后保存");
    meta.message = msg;
    meta
}

fn llm_fetch_impl(
    path: &Path,
    db_title: &str,
    db_author: &str,
    api_key: &str,
) -> Result<SmartMeta, String> {
    // 查询输入：库里已有 > OPF > 文件名
    let fname = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let (guess_t, guess_a) = guess_title_author_from_filename(&fname);
    let mut title = db_title.trim().to_string();
    let mut author = db_author.trim().to_string();
    if let Some((_, _, opf)) = read_container_and_opf(path) {
        let basic = parse_opf_basic(&opf);
        if title.is_empty() {
            title = basic.title;
        }
        if author.is_empty() {
            author = basic.authors.join("、");
        }
    }
    if title.is_empty() {
        title = guess_t;
    }
    if author.is_empty() {
        author = guess_a;
    }
    if title.is_empty() {
        return Err("无法确定书名：请先在表单里填写书名，再点「AI填入」".into());
    }

    let body = serde_json::json!({
        "model": DEEPSEEK_MODEL,
        "messages": [
            {"role": "system", "content": LLM_META_SYSTEM},
            {"role": "user", "content": format!("{}\n\n书名：{} 作者：{}", LLM_META_TASK, title, author)},
        ],
        "temperature": 0.2,
        "response_format": {"type": "json_object"},
    });

    let ua = format!("cshow-gui/{DEEPSEEK_MODEL}");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .user_agent(&ua)
        .build();
    let resp = agent
        .post(DEEPSEEK_API_URL)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", api_key.trim()))
        .send_json(&body)
        .map_err(|e| format!("DeepSeek API 请求失败: {e}"))?;
    let text = resp
        .into_string()
        .map_err(|e| format!("读取响应失败: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("响应不是合法 JSON: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!("DeepSeek API 错误: {err}"));
    }
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
        .ok_or("响应中没有模型输出内容")?;
    let meta_v: serde_json::Value = serde_json::from_str(content.trim())
        .map_err(|e| format!("模型输出不是合法 JSON: {e}\n原始输出：{}", truncate_chars(content, 200)))?;
    Ok(parse_llm_meta(&meta_v, &title, &author))
}
// ---- 修书：目录错位 EPUB 重构（一文件多章 → 每章一文件） ----

#[derive(Serialize, Clone)]
struct TocFixReport {
    needs_fix: bool,
    total: usize,      // 带锚点的目录条目数
    mispointed: usize, // 锚点不在引用文件里的条目数
    chapters: usize,   // 修复后的章节文件数（未修复为 0）
    message: String,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn collect_html_anchors(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for attr in ["id", "name"] {
        let needle = format!("{attr}=\"");
        let mut start = 0usize;
        while let Some(p) = html[start..].find(&needle) {
            let s = start + p + needle.len();
            match html[s..].find('"') {
                Some(e) => {
                    let a = &html[s..s + e];
                    if !a.is_empty() {
                        out.push(a.to_string());
                    }
                    start = s + e;
                }
                None => break,
            }
        }
    }
    out
}

fn zip_read_text(zip: &mut zip::ZipArchive<fs::File>, name: &str) -> Result<String, String> {
    let mut f = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| format!("{name}: {e}"))?;
    Ok(s)
}

/// 修书用 OPF 解析：元数据、manifest(id→href/media/props)、spine、ncx
struct OpfSimple {
    title: String,
    creator: String,
    lang: String,
    identifier: String,
    manifest: HashMap<String, (String, String, String)>, // id -> (href, media, props)
    manifest_order: Vec<String>,
    spine: Vec<String>, // href（相对 OPF 目录）
    ncx_href: Option<String>,
}

fn parse_opf_simple(opf: &str) -> OpfSimple {
    let mut manifest: HashMap<String, (String, String, String)> = HashMap::new();
    let mut manifest_order: Vec<String> = Vec::new();
    let mut spine_ids: Vec<String> = Vec::new();
    let mut ncx_id = String::new();
    let mut meta = (String::new(), String::new(), String::new(), String::new());
    if let Ok(doc) = roxmltree::Document::parse(opf) {
        for node in doc.descendants() {
            match node.tag_name().name() {
                "title" => {
                    if meta.0.is_empty() {
                        meta.0 = node.text().unwrap_or("").trim().to_string();
                    }
                }
                "creator" => {
                    if meta.1.is_empty() {
                        meta.1 = node.text().unwrap_or("").trim().to_string();
                    }
                }
                "language" => {
                    if meta.2.is_empty() {
                        meta.2 = node.text().unwrap_or("").trim().to_string();
                    }
                }
                "identifier" => {
                    if meta.3.is_empty() {
                        meta.3 = node.text().unwrap_or("").trim().to_string();
                    }
                }
                "item" => {
                    let id = node.attribute("id").unwrap_or("").to_string();
                    let href = node.attribute("href").unwrap_or("").to_string();
                    if !id.is_empty() && !href.is_empty() {
                        let media = node.attribute("media-type").unwrap_or("").to_string();
                        let props = node.attribute("properties").unwrap_or("").to_string();
                        manifest.entry(id.clone()).or_insert((href, media, props));
                        manifest_order.push(id);
                    }
                }
                "spine" => {
                    ncx_id = node.attribute("toc").unwrap_or("").to_string();
                }
                "itemref" => {
                    let idref = node.attribute("idref").unwrap_or("").to_string();
                    if !idref.is_empty() {
                        spine_ids.push(idref);
                    }
                }
                _ => {}
            }
        }
    }
    let spine: Vec<String> = spine_ids
        .iter()
        .filter_map(|id| manifest.get(id).map(|x| x.0.clone()))
        .collect();
    let ncx_href = manifest.get(&ncx_id).map(|x| x.0.clone());
    OpfSimple {
        title: meta.0,
        creator: meta.1,
        lang: meta.2,
        identifier: meta.3,
        manifest,
        manifest_order,
        spine,
        ncx_href,
    }
}

/// 分析 EPUB 目录：统计带锚点条目里锚点不在引用文件的错位数
fn analyze_epub_toc(src: &Path) -> Result<TocFixReport, String> {
    let file = fs::File::open(src).map_err(|e| format!("打开失败: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("不是有效的 EPUB: {e}"))?;
    let container = zip_read_text(&mut zip, "META-INF/container.xml")
        .map_err(|e| format!("缺少 container.xml: {e}"))?;
    let opf_rel = opf_rel_from_container(&container).ok_or("EPUB 中未找到 OPF")?;
    let opf_dir = parent_dir(&opf_rel);
    let opf = zip_read_text(&mut zip, &opf_rel).map_err(|e| format!("OPF 读取失败: {e}"))?;
    let simple = parse_opf_simple(&opf);
    if simple.spine.is_empty() {
        return Ok(TocFixReport {
            needs_fix: false,
            total: 0,
            mispointed: 0,
            chapters: 0,
            message: "没有可读章节（可能是图像书或空书）".into(),
        });
    }
    let ncx_text = match &simple.ncx_href {
        Some(h) => zip_read_text(&mut zip, &join_rel(&opf_dir, h)).unwrap_or_default(),
        None => String::new(),
    };
    let links = extract_toc_links(&ncx_text);

    // 锚点 → 实际所在章节
    let mut anchor_ch: HashMap<String, usize> = HashMap::new();
    for (i, href) in simple.spine.iter().enumerate() {
        let abs = join_rel(&opf_dir, href);
        if let Ok(html) = zip_read_text(&mut zip, &abs) {
            for a in collect_html_anchors(&html) {
                anchor_ch.entry(a).or_insert(i);
            }
        }
    }

    let mut total = 0usize;
    let mut mispointed = 0usize;
    for (_, src) in &links {
        let Some((clean, anchor)) = src.split_once('#') else { continue };
        if anchor.is_empty() {
            continue;
        }
        let abs = join_rel(&opf_dir, clean);
        let Some(ch) = simple.spine.iter().position(|s| s == &abs) else {
            continue;
        };
        total += 1;
        let ok = anchor_ch.get(anchor).map(|&real| real == ch).unwrap_or(false);
        if !ok {
            mispointed += 1;
        }
    }

    let needs_fix = mispointed >= 3 && mispointed * 2 >= total.max(1);
    let message = if needs_fix {
        format!("发现 {mispointed}/{total} 条目录指向错误文件，可重写为每章一文件（原文件自动备份）")
    } else if total == 0 {
        "未找到带锚点的目录条目".to_string()
    } else {
        "目录结构正常，无需修复".to_string()
    };
    Ok(TocFixReport {
        needs_fix,
        total,
        mispointed,
        chapters: 0,
        message,
    })
}

fn extract_zip_to(src: &Path, dir: &Path) -> Result<(), String> {
    let file = fs::File::open(src).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(out_path) = entry.enclosed_name() else { continue };
        let out_path = dir.join(out_path);
        if entry.is_dir() {
            let _ = fs::create_dir_all(&out_path);
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn extract_body(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let Some(b) = lower.find("<body") else { return html.to_string() };
    let Some(gt) = html[b..].find('>') else { return html.to_string() };
    let start = b + gt + 1;
    let Some(be) = lower[start..].find("</body>") else { return html[start..].to_string() };
    html[start..start + be].to_string()
}

/// 按“目录引用的锚点”切分正文：返回 (前导无锚点内容, [(锚点, 内容)])
fn split_body_by_anchors(body: &str, wanted: &HashSet<String>) -> (String, Vec<(String, String)>) {
    let mut hits: Vec<(String, usize)> = Vec::new();
    let mut start = 0usize;
    while let Some(p) = body[start..].find("id=\"") {
        let i = start + p;
        let id_start = i + "id=\"".len();
        match body[id_start..].find('"') {
            Some(e) => {
                let id = &body[id_start..id_start + e];
                if wanted.contains(id) {
                    let span = body[..i].rfind("<span").unwrap_or(i);
                    hits.push((id.to_string(), span));
                }
                start = id_start + e;
            }
            None => break,
        }
    }
    hits.sort_by_key(|(_, s)| *s);
    if hits.is_empty() {
        return (body.to_string(), Vec::new());
    }
    let lead = body[..hits[0].1].to_string();
    let mut chunks = Vec::new();
    for k in 0..hits.len() {
        let s = hits[k].1;
        let e = if k + 1 < hits.len() { hits[k + 1].1 } else { body.len() };
        chunks.push((hits[k].0.clone(), body[s..e].to_string()));
    }
    (lead, chunks)
}

const FIX_WRAP_HEAD: &str = "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd\">\n<html xmlns=\"http://www.w3.org/1999/xhtml\">\n<head><title></title><meta charset=\"utf-8\"/></head>\n<body>\n";
const FIX_WRAP_TAIL: &str = "\n</body>\n</html>\n";

/// 重构坏 EPUB：按目录锚点切分为每章一文件，重写 OPF/NCX，备份原文件后替换
fn rebuild_broken_epub(src: &Path) -> Result<TocFixReport, String> {
    rebuild_broken_epub_in(src, &work_dir().join("backups"))
}

fn rebuild_broken_epub_in(src: &Path, backup_root: &Path) -> Result<TocFixReport, String> {
    let report = analyze_epub_toc(src)?;
    if !report.needs_fix {
        return Ok(report);
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let work = std::env::temp_dir().join(format!("cshow-fix-{}-{stamp}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    extract_zip_to(src, &work).map_err(|e| format!("解包失败: {e}"))?;

    let container = fs::read_to_string(work.join("META-INF/container.xml"))
        .map_err(|e| format!("container.xml: {e}"))?;
    let opf_rel = opf_rel_from_container(&container).ok_or("未找到 OPF")?;
    let opf_dir = parent_dir(&opf_rel);
    let opf = fs::read_to_string(work.join(&opf_rel)).map_err(|e| format!("OPF: {e}"))?;
    let simple = parse_opf_simple(&opf);
    let opf_dir_abs = work.join(&opf_dir);
    fs::create_dir_all(&opf_dir_abs).map_err(|e| e.to_string())?;

    let ncx_abs = simple.ncx_href.as_ref().map(|h| join_rel(&opf_dir, h));
    let ncx_text = ncx_abs
        .as_ref()
        .map(|p| fs::read_to_string(work.join(p)).ok())
        .flatten()
        .unwrap_or_default();
    let links = extract_toc_links(&ncx_text);
    let mut wanted: HashSet<String> = HashSet::new();
    for (_, src) in &links {
        if let Some((_, a)) = src.split_once('#') {
            if !a.is_empty() {
                wanted.insert(a.to_string());
            }
        }
    }

    // 逐 spine 文件切分
    let mut chapter_files: Vec<(String, Option<String>)> = Vec::new();
    let mut anchor2file: HashMap<String, String> = HashMap::new();
    let mut file0: HashMap<String, String> = HashMap::new();
    for href in &simple.spine {
        let abs = join_rel(&opf_dir, href);
        let p = work.join(&abs);
        let Ok(raw) = fs::read_to_string(&p) else { continue };
        let body = extract_body(&raw);
        let (lead, chunks) = split_body_by_anchors(&body, &wanted);
        let mut pieces: Vec<(Option<String>, String)> = Vec::new();
        if !lead.trim().is_empty() {
            pieces.push((None, lead));
        }
        pieces.extend(chunks.into_iter().map(|(a, c)| (Some(a), c)));
        if pieces.is_empty() {
            pieces.push((None, body));
        }
        let mut first: Option<String> = None;
        for (a, content) in pieces {
            let name = format!("chapter{:04}.xhtml", chapter_files.len());
            let out = opf_dir_abs.join(&name);
            let full = format!("{FIX_WRAP_HEAD}{}{FIX_WRAP_TAIL}", content.trim_end());
            fs::write(&out, full).map_err(|e| format!("写章节失败: {e}"))?;
            if first.is_none() {
                first = Some(name.clone());
            }
            if let Some(aid) = &a {
                anchor2file.insert(aid.clone(), name.clone());
            }
            chapter_files.push((name, a));
        }
        if let Some(f) = first {
            file0.insert(href.clone(), f);
        }
        let _ = fs::remove_file(&p);
    }
    if chapter_files.is_empty() {
        let _ = fs::remove_dir_all(&work);
        return Err("没有可切分的章节".into());
    }

    // 新 OPF：章节 + 保留的非文本项（图片等）+ ncx
    let mut manifest_lines: Vec<String> = Vec::new();
    for (i, (name, _)) in chapter_files.iter().enumerate() {
        manifest_lines.push(format!(
            "<item id=\"c{i}\" href=\"{name}\" media-type=\"application/xhtml+xml\"/>"
        ));
    }
    for id in &simple.manifest_order {
        let Some((href, media, props)) = simple.manifest.get(id) else { continue };
        if simple.spine.contains(href) || simple.ncx_href.as_deref() == Some(href.as_str()) {
            continue;
        }
        let mut attrs = format!("id=\"{id}\" href=\"{href}\" media-type=\"{media}\"");
        if !props.is_empty() {
            attrs.push_str(&format!(" properties=\"{props}\""));
        }
        manifest_lines.push(format!("<item {attrs}/>"));
    }
    manifest_lines.push("<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>".into());
    let spine_lines: Vec<String> = (0..chapter_files.len())
        .map(|i| format!("<itemref idref=\"c{i}\"/>"))
        .collect();
    let opf_new = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<package xmlns=\"http://www.idpf.org/2007/opf\" unique-identifier=\"bookid\" version=\"2.0\">\n\
  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:opf=\"http://www.idpf.org/2007/opf\">\n\
    <dc:title>{}</dc:title>\n\
    <dc:creator opf:role=\"aut\">{}</dc:creator>\n\
    <dc:language>{}</dc:language>\n\
    <dc:identifier id=\"bookid\">{}</dc:identifier>\n\
    <meta name=\"cover\" content=\"cover-image\"/>\n\
  </metadata>\n\
  <manifest>\n{}\n  </manifest>\n\
  <spine toc=\"ncx\">\n{}\n  </spine>\n\
</package>\n",
        xml_escape(&simple.title),
        xml_escape(&simple.creator),
        xml_escape(&simple.lang),
        xml_escape(&simple.identifier),
        manifest_lines
            .iter()
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
        spine_lines
            .iter()
            .map(|l| format!("    {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    fs::write(work.join(&opf_rel), opf_new).map_err(|e| format!("写 OPF 失败: {e}"))?;

    // 新 NCX：目录 1:1 指向新章节文件
    let mut ncx_lines: Vec<String> = Vec::new();
    for (i, (label, src)) in links.iter().enumerate() {
        let target = if let Some((tf, a)) = src.split_once('#') {
            anchor2file
                .get(a)
                .map(|f| format!("{f}#{a}"))
                .or_else(|| file0.get(tf).cloned())
        } else {
            file0.get(src.as_str()).cloned()
        };
        if let Some(t) = target {
            ncx_lines.push(format!(
                "    <navPoint id=\"n{}\" playOrder=\"{}\">\n      <navLabel><text>{}</text></navLabel>\n      <content src=\"{}\"/>\n    </navPoint>",
                i + 1,
                i + 1,
                xml_escape(label),
                t
            ));
        }
    }
    let ncx_new = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
  <head><meta name=\"dtb:uid\" content=\"bookid\"/></head>\n\
  <docTitle><text>{}</text></docTitle>\n\
  <navMap>\n{}\n  </navMap>\n\
</ncx>\n",
        xml_escape(&simple.title),
        ncx_lines.join("\n")
    );
    let ncx_path = ncx_abs
        .as_ref()
        .map(|p| work.join(p))
        .unwrap_or_else(|| opf_dir_abs.join("toc.ncx"));
    fs::write(&ncx_path, ncx_new).map_err(|e| format!("写 NCX 失败: {e}"))?;

    // 打包（mimetype 必须第一个且不压缩）
    let tmp_epub = work.join("__fixed.epub");
    let out_file = fs::File::create(&tmp_epub).map_err(|e| e.to_string())?;
    let mut zw = zip::ZipWriter::new(out_file);
    let stored = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("mimetype", stored).map_err(|e| e.to_string())?;
    zw.write_all(b"application/epub+zip").map_err(|e| e.to_string())?;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut stack = vec![work.clone()];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    paths.push(p);
                }
            }
        }
    }
    paths.sort();
    for p in paths {
        let rel = p
            .strip_prefix(&work)
            .map_err(|_| "路径错误".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "mimetype" || rel == "__fixed.epub" {
            continue;
        }
        zw.start_file(&rel, deflated).map_err(|e| e.to_string())?;
        let mut f = fs::File::open(&p).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
    }
    zw.finish().map_err(|e| e.to_string())?;

    // 替换前校验：新书目录应已 1:1 正确；校验不过则放弃替换，原文件不动
    let check = analyze_epub_toc(&tmp_epub).map_err(|e| format!("修复后校验失败: {e}"))?;
    if check.needs_fix || check.mispointed != 0 {
        let _ = fs::remove_dir_all(&work);
        return Err(format!(
            "修复后目录仍异常（错位 {} 条），已放弃替换，原文件未改动",
            check.mispointed
        ));
    }

    // 备份并替换
    let backups = backup_root.to_path_buf();
    fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
    let stem = src
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("book.epub");
    let bak = backups.join(format!("{stem}.orig.epub"));
    fs::copy(src, &bak).map_err(|e| format!("备份失败: {e}"))?;
    fs::copy(&tmp_epub, src).map_err(|e| format!("替换失败: {e}"))?;
    let _ = fs::remove_dir_all(&work);

    Ok(TocFixReport {
        needs_fix: true,
        total: report.total,
        mispointed: report.mispointed,
        chapters: chapter_files.len(),
        message: format!(
            "已修复：重写为 {} 个章节文件（原文件已备份到 backups/），重新打开本书生效",
            chapter_files.len()
        ),
    })
}

#[tauri::command]
fn check_epub_toc(path: String) -> Result<TocFixReport, String> {
    analyze_epub_toc(Path::new(&path))
}

#[tauri::command]
async fn fix_epub_toc(path: String) -> Result<TocFixReport, String> {
    let p = PathBuf::from(&path);
    tauri::async_runtime::spawn_blocking(move || rebuild_broken_epub(&p))
        .await
        .map_err(|e| format!("修复任务失败: {e}"))?
}

/// 批量读取元数据（书库网格一次取齐）
#[tauri::command]
fn list_book_meta(state: tauri::State<'_, db::Db>, paths: Vec<String>) -> Vec<serde_json::Value> {
    let conn = state.0.lock().unwrap();
    for p in &paths {
        maybe_fill_preset_meta(&conn, p);
    }
    db::list_book_meta(&conn, &paths).unwrap_or_default()
}

// ---- 阅读统计 ----

#[derive(Serialize)]
struct RecentBook {
    path: String,
    name: String,
    is_dir: bool,
    last_read_at: u64,
    finished: bool,
    read_time: u64,
}

#[derive(Serialize)]
struct ReadingStats {
    total_books: usize,
    finished_books: usize,
    started_books: usize,
    total_read_time: u64,
    recent: Vec<RecentBook>,
}

/// 目录书：所有分卷是否都读完
fn dir_all_finished(conn: &rusqlite::Connection, dir: &str) -> bool {
    let root = Path::new(dir);
    let mut vols: Vec<String> = Vec::new();
    let mut files = 0usize;
    let mut subdirs = 0usize;
    if let Ok(rd) = fs::read_dir(root) {
        for item in rd.flatten() {
            let p = item.path();
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                subdirs += 1;
                vols.push(norm_path(&p));
            } else {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if ext == "epub" || ext == "pdf" {
                    files += 1;
                    vols.push(norm_path(&p));
                }
            }
        }
    }
    if files == 0 {
        if subdirs == 0 {
            vols = vec![dir.to_string()];
        }
    } else {
        vols.retain(|v| Path::new(v).is_file());
    }
    !vols.is_empty()
        && vols.iter().all(|vp| {
            db::get_position(conn, vp)
                .ok()
                .flatten()
                .map(|pos| pos.finished)
                .unwrap_or(false)
        })
}

/// 一本书的进度：返回 (最近阅读时间, 是否全书读完)
fn book_progress(conn: &rusqlite::Connection, p: &Path, is_dir: bool) -> (u64, bool) {
    let np = norm_path(p);
    let row = db::get_book(conn, &np).ok().flatten();
    let last = row.as_ref().map(|b| b.last_read_at).unwrap_or(0);
    let finished = if is_dir {
        dir_all_finished(conn, &np)
    } else {
        db::get_position(conn, &np)
            .ok()
            .flatten()
            .map(|pos| pos.finished)
            .unwrap_or(false)
    };
    (last, finished)
}

/// 跨所有书库聚合阅读统计：读完/在读本数、最近阅读列表
#[tauri::command]
fn reading_stats(state: tauri::State<'_, db::Db>) -> ReadingStats {
    let conn = state.0.lock().unwrap();
    let libs = db::list_libraries(&conn).unwrap_or_default();
    let mut books: Vec<(String, String, bool)> = Vec::new(); // (path, name, is_dir)
    for lib in libs {
        let root = PathBuf::from(&lib.path);
        let Ok(rd) = fs::read_dir(&root) else { continue };
        for item in rd.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let p = item.path();
            if path_is_hidden(&conn, &p) {
                continue; // 已隐藏（eye-off）的书不列入统计
            }
            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                if dir_is_ebook(&conn, &p) {
                    books.push((norm_path(&p), name, true));
                }
            } else {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if ext == "epub" || ext == "pdf" {
                    books.push((norm_path(&p), name, false));
                }
            }
        }
    }
    books.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    books.dedup_by(|a, b| a.0 == b.0);

    let mut recent: Vec<RecentBook> = Vec::new();
    let mut finished_books = 0usize;
    let mut started_books = 0usize;
    let mut total_read_time = 0u64;
    for (path, name, is_dir) in &books {
        // 预置库/手动元数据优先：统计展示用准确书名，而不是文件名
        maybe_fill_preset_meta(&conn, path);
        let meta_title = db::get_book(&conn, path)
            .ok()
            .flatten()
            .map(|b| b.title.trim().to_string())
            .filter(|t| !t.is_empty());
        let (last_read_at, finished) = book_progress(&conn, Path::new(path), *is_dir);
        let read_time = read_reading_time(&conn, Path::new(path));
        total_read_time += read_time;
        if last_read_at > 0 {
            started_books += 1;
        }
        if finished {
            finished_books += 1;
        }
        recent.push(RecentBook {
            path: path.clone(),
            name: meta_title.unwrap_or_else(|| name.clone()),
            is_dir: *is_dir,
            last_read_at,
            finished,
            read_time,
        });
    }
    recent.retain(|r| r.last_read_at > 0 || r.finished);
    recent.sort_by(|a, b| b.last_read_at.cmp(&a.last_read_at));
    recent.truncate(50);
    ReadingStats {
        total_books: books.len(),
        finished_books,
        started_books,
        total_read_time,
        recent,
    }
}

/// 切换收藏状态（前端通过 list_favorites 刷新）
#[tauri::command]
fn toggle_favorite(state: tauri::State<'_, db::Db>, path: String) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    if db::get_library(&conn, &path)?.is_some() {
        // 移除书库：级联删除书籍/位置/设置/目录状态，并清理缩略图与 EPUB 解包缓存
        let affected = db::delete_library_cascade(&conn, &path)?;
        for p in affected {
            let pp = Path::new(&p);
            let thumb = thumb_cache_dir().join(format!("{}.png", thumb_key(pp)));
            let _ = fs::remove_file(&thumb);
            if pp
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("epub"))
                .unwrap_or(false)
            {
                let epub = epub_cache_dir().join(epub_cache_key(pp));
                if epub.exists() {
                    let _ = fs::remove_dir_all(&epub);
                }
            }
        }
        Ok(())
    } else {
        ensure_ebook_marks(&conn, Path::new(&path)); // 新书库的子文件夹自动标记为电子书
        let sort_order = db::list_libraries(&conn)?.len() as i64;
        db::upsert_library(&conn, &path, "", "", sort_order, false, None)
    }
}

/// 设置书库别名与图标
#[tauri::command]
fn set_library_meta(state: tauri::State<'_, db::Db>, path: String, alias: String, icon: String) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_library_meta(&conn, &path, alias.trim(), icon.trim())
}

/// 按传入顺序重排书库（未知路径忽略，缺失的追加到末尾）
#[tauri::command]
fn reorder_libraries(state: tauri::State<'_, db::Db>, paths: Vec<String>) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    let libs = db::list_libraries(&conn)?;
    let mut order: Vec<String> = paths.clone();
    for l in &libs {
        if !order.contains(&l.path) {
            order.push(l.path.clone());
        }
    }
    db::reorder_libraries(&conn, &order)
}

#[tauri::command]
fn save_cwd(state: tauri::State<'_, db::Db>, path: String) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_app_state(&conn, "cwd", &path)
}

#[tauri::command]
fn initial_dir(state: tauri::State<'_, db::Db>) -> String {
    // 显式传参优先；否则回到上次关闭时的目录；再否则用户主目录
    if let Some(p) = std::env::args().nth(1) {
        return p.replace('\\', "/");
    }
    let conn = state.0.lock().unwrap();
    if let Ok(Some(saved)) = db::get_app_state(&conn, "cwd") {
        let saved = saved.trim().to_string();
        if !saved.is_empty() && Path::new(&saved).is_dir() {
            // Android：旧版可能把目录存在了应用私有空间（出不去也到不了手机存储），
            // 只恢复手机存储里的路径，否则回到存储根目录
            #[cfg(target_os = "android")]
            if !saved.starts_with("/storage/emulated/") {
                return android_default_start_dir();
            }
            return saved.replace('\\', "/");
        }
    }
    #[cfg(target_os = "android")]
    return android_default_start_dir();
    #[cfg(not(target_os = "android"))]
    dirs::home_dir()
        .map(|p| norm_path(&p))
        .unwrap_or_else(|| "/".into())
}

/// Android 默认起始目录：手机存储根目录（书库/书都放在这里）
#[cfg(target_os = "android")]
fn android_default_start_dir() -> String {
    let pub_root = PathBuf::from("/storage/emulated/0");
    if pub_root.is_dir() {
        norm_path(&pub_root)
    } else {
        norm_path(&android_home())
    }
}

#[tauri::command]
fn image_dims(paths: Vec<String>) -> Vec<Option<(u32, u32)>> {
    paths
        .iter()
        .map(|p| image::image_dimensions(p).ok())
        .collect()
}


/// 当前打开的书的解包根目录（book:// 协议的服务根）
struct BookState(Mutex<Option<PathBuf>>);

fn workdir_file() -> PathBuf {
    app_config_dir().join("workdir")
}

fn configured_work_dir() -> Option<PathBuf> {
    let s = fs::read_to_string(workdir_file()).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn default_work_dir() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        android_home().join("cshow-work")
    }
    #[cfg(not(target_os = "android"))]
    dirs::document_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("cshow-work")
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 首次启动时把旧的缓存目录（macOS ~/Library/Caches/cshow-gui）迁移到工作目录（一次性）
fn migrate_old_cache(work: &Path) {
    let old = dirs::cache_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join("cshow-gui");
    if !old.join("epub").is_dir() && !old.join("thumbs").is_dir() {
        return;
    }
    if work.join("epub").exists() || work.join("thumbs").exists() {
        return;
    }
    let _ = fs::create_dir_all(work);
    for sub in ["epub", "thumbs"] {
        let src = old.join(sub);
        if !src.is_dir() {
            continue;
        }
        // 同卷直接改名（秒级）；跨卷则复制后删除
        if fs::rename(&src, work.join(sub)).is_err() {
            let _ = copy_dir_all(&src, &work.join(sub));
            let _ = fs::remove_dir_all(&src);
        }
    }
}

/// 清理孤儿 EPUB 缓存：只保留书库中当前书对应的缓存目录，
/// 删除旧缓存版本（v1/v2/v3…）、已移除的书、文件已变化的残留，防止缓存膨胀。
/// 缓存均可按需重建，删除失败（只读等）时跳过。
fn cleanup_orphan_caches(conn: &rusqlite::Connection, work: &Path) {
    let Ok(paths) = db::list_epub_paths(conn) else { return };
    if paths.is_empty() {
        return;
    }
    let keep: HashSet<String> = paths
        .iter()
        .map(|p| epub_cache_key(Path::new(p)))
        .collect();
    let epub_dir = work.join("epub");
    let Ok(rd) = fs::read_dir(&epub_dir) else { return };
    let mut removed = 0usize;
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) && fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        log::info!("清理孤儿 EPUB 缓存目录 {removed} 个");
    }
}

static WORK_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// cshow 工作目录：EPUB 解包缓存与缩略图都放这里（默认 ~/Documents/cshow-work，自动创建）
fn work_dir() -> PathBuf {
    if let Ok(g) = WORK_DIR.lock() {
        if let Some(d) = g.clone() {
            return d;
        }
    }
    let dir = configured_work_dir().unwrap_or_else(default_work_dir);
    migrate_old_cache(&dir);
    let _ = fs::create_dir_all(dir.join("epub"));
    let _ = fs::create_dir_all(dir.join("thumbs"));
    if let Ok(mut g) = WORK_DIR.lock() {
        *g = Some(dir.clone());
    }
    dir
}

#[tauri::command]
fn get_work_dir() -> String {
    norm_path(&work_dir())
}

/// Android 一次性导入：把 /sdcard/Download/mig-tmp 里暂存的迁移数据库与封面
/// 复制进工作目录（应用自身有存储权限，替代需要 adb run-as 的写入）。
/// 导入完成后删除暂存目录；没有暂存文件时为空操作。
#[cfg(target_os = "android")]
fn import_staged_migration(work: &Path) {
    let staged = PathBuf::from("/sdcard/Download/mig-tmp");
    let staged_db = staged.join("library.sqlite3");
    if staged_db.is_file() {
        let _ = fs::create_dir_all(work);
        if fs::copy(&staged_db, work.join("library.sqlite3")).is_ok() {
            // 新库是干净的单文件，删除旧 WAL/SHM 避免冲突
            let _ = fs::remove_file(work.join("library.sqlite3-wal"));
            let _ = fs::remove_file(work.join("library.sqlite3-shm"));
        }
    }
    let staged_covers = staged.join("covers");
    if staged_covers.is_dir() {
        let dst = work.join("covers");
        let _ = fs::create_dir_all(&dst);
        if let Ok(rd) = fs::read_dir(&staged_covers) {
            for e in rd.flatten() {
                let _ = fs::copy(e.path(), dst.join(e.file_name()));
            }
        }
    }
    let _ = fs::remove_dir_all(&staged);
}

#[tauri::command]
fn set_work_dir(path: String) -> Result<String, String> {
    let p = PathBuf::from(&path);
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    fs::write(workdir_file(), path).map_err(|e| e.to_string())?;
    if let Ok(mut g) = WORK_DIR.lock() {
        *g = None; // 下次调用重新解析并迁移
    }
    Ok(norm_path(&work_dir()))
}

fn mime_for(p: &Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        // 文本类型显式带 charset=utf-8：EPUB 章节经常不声明 XML/meta charset，
        // 不带 charset 时 WebView 嗅探可能按错误编码解析，英文书的弯引号/重音符号会变乱码
        Some("html" | "htm" | "xhtml") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("xml") => "text/xml; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// book:// 相对路径安全检查：拒绝 `..` / `.` 段（调用方已先做 URL 解码，%2e%2e 已还原成 ..），
/// 防止请求路径逃出解包根读取任意本地文件。
fn safe_book_rel(decoded: &str) -> Option<String> {
    let rel = decoded.trim_start_matches('/');
    if rel.split('/').any(|seg| seg == "." || seg == "..") {
        return None;
    }
    Some(rel.to_string())
}

#[derive(Serialize, serde::Deserialize, Clone)]
struct TocEntry {
    label: String,
    chapter: usize,
    /// NCX/nav 条目指向的文件内锚点（如 #filepos0000004886），用于定位文件内章节
    #[serde(default)]
    anchor: Option<String>,
}

#[derive(Serialize, serde::Deserialize)]
struct EpubMeta {
    base_dir: String,
    spine: Vec<String>,
    title: String,
    #[serde(default)]
    cover: Option<String>,
    /// 是否为文字为主的书（vs 以整页图片为主的图像书）
    #[serde(default)]
    text_book: bool,
    /// 平铺目录（label + 章节下标）
    #[serde(default)]
    toc: Vec<TocEntry>,
    /// 每章标题（与 spine 对齐，缺省用文件名兜底）
    #[serde(default)]
    chapter_titles: Vec<String>,
    /// 每章正文字符数（与 spine 对齐），用于计算全书阅读百分比
    #[serde(default)]
    chapter_lengths: Vec<usize>,
}

#[derive(Serialize)]
struct EbookVolume {
    kind: String, // "pdf" | "epub" | "imgdir"
    name: String,
    path: String,
    thumb: Option<String>,
    last_read: bool,
    saved_page: Option<u32>,
    total: Option<u32>,
    saved_mode: Option<String>,
    finished: bool,
    saved_progress: Option<f64>,
}

fn epub_cache_dir() -> PathBuf {
    work_dir().join("epub")
}

fn epub_cache_key(path: &Path) -> String {
    let mut h = DefaultHasher::new();
    // 缓存结构版本：解包产物（__cshow_spine.json 字段 / reader 文件）变更时递增，
    // 强制旧缓存按新逻辑重新解包
    // v5：目录条目保留文件内锚点（TocEntry.anchor），旧缓存需重新解包
    // v6：目录解析按锚点实际所在章节重定向（修正 NCX 全部指向首文件的坏书）
    // v7：锚点在引用文件内存在时不再重定向（p20 等页面锚点跨文件重复，避免误拽回首文件）
    "epub-cache-v7".hash(&mut h);
    // TXT 解析器产物版本：生成章节结构变更时递增，只强制 txt 缓存重建
    if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("txt"))
        .unwrap_or(false)
    {
        "txt-cache-v4".hash(&mut h);
    }
    path.hash(&mut h);
    if let Ok(m) = fs::metadata(path) {
        m.len().hash(&mut h);
        if let Ok(t) = m.modified() {
            if let Ok(d) = t.duration_since(UNIX_EPOCH) {
                d.as_nanos().hash(&mut h);
            }
        }
    }
    format!("{:016x}", h.finish())
}

fn thumb_cache_dir() -> PathBuf {
    // v2：封面检测逻辑升级后强制重新生成，避免旧的黑底占位图残留
    work_dir().join("thumbs/v2")
}

fn covers_dir() -> PathBuf {
    work_dir().join("covers")
}

fn cover_key(path: &Path) -> String {
    let mut h = DefaultHasher::new();
    "custom-cover-v1".hash(&mut h);
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn thumb_key(path: &Path) -> String {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(m) = fs::metadata(path) {
        m.len().hash(&mut h);
        if let Ok(t) = m.modified() {
            if let Ok(d) = t.duration_since(UNIX_EPOCH) {
                d.as_nanos().hash(&mut h);
            }
        }
    }
    format!("{:016x}", h.finish())
}

fn cached_thumb(path: &Path) -> Option<String> {
    let f = thumb_cache_dir().join(format!("{}.png", thumb_key(path)));
    if f.is_file() {
        Some(norm_path(&f))
    } else {
        None
    }
}

/// 把 EPUB 封面缩小生成缩略图缓存（宽 360px），返回缓存文件路径。
/// 避免每次进入 app 都重新加载原始大封面。
fn make_thumb_cache(src: &Path, cover_path: &Path) -> Option<String> {
    make_thumb_cache_in(src, cover_path, &thumb_cache_dir())
}

fn make_thumb_cache_in(src: &Path, cover_path: &Path, dir: &Path) -> Option<String> {
    let img = image::open(cover_path).ok()?;
    let w = 360u32;
    let h = ((img.height() as u64 * w as u64) / (img.width() as u64).max(1)).max(1) as u32;
    let thumb = img.resize(w, h, image::imageops::FilterType::Triangle);
    fs::create_dir_all(&dir).ok()?;
    let out = dir.join(format!("{}.png", thumb_key(src)));
    thumb.save(&out).ok()?;
    Some(norm_path(&out))
}

/// 保存缩略图到磁盘缓存，返回缓存文件路径
#[tauri::command]
fn save_thumb(path: String, data: Vec<u8>) -> Result<String, String> {
    let dir = thumb_cache_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(format!("{}.png", thumb_key(Path::new(&path))));
    fs::write(&out, data).map_err(|e| e.to_string())?;
    Ok(norm_path(&out))
}

/// 设置一本书的自定义封面（data 为图片文件字节），返回封面缓存路径
#[tauri::command]
fn set_book_cover(
    state: tauri::State<'_, db::Db>,
    path: String,
    name: String,
    data: String, // base64 图片字节（前端 FileReader 读取）
) -> Result<String, String> {
    let ext = Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"))
        .ok_or("仅支持 png/jpg/gif/webp/bmp 图片")?;
    if data.is_empty() {
        return Err("图片内容为空".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("图片数据解码失败：{e}"))?;
    if bytes.is_empty() {
        return Err("图片内容为空".into());
    }
    let norm = norm_path(Path::new(&path));
    let dir = covers_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let out = dir.join(format!("cover_{}.{}", cover_key(Path::new(&norm)), ext));
    fs::write(&out, &bytes).map_err(|e| e.to_string())?;
    let out_norm = norm_path(&out);
    let conn = state.0.lock().unwrap();
    let kind = if Path::new(&path).is_dir() {
        "dir".to_string()
    } else {
        Path::new(&path)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_else(|| "file".to_string())
    };
    db::ensure_book(&conn, &norm, &kind)?;
    db::set_book_cover(&conn, &norm, Some(&out_norm))?;
    Ok(out_norm)
}

/// 移除一本书的自定义封面（回到自动封面/文字封面）
#[tauri::command]
fn remove_book_cover(state: tauri::State<'_, db::Db>, path: String) -> Result<(), String> {
    let norm = norm_path(Path::new(&path));
    let conn = state.0.lock().unwrap();
    let prev = db::get_book(&conn, &norm)
        .ok()
        .flatten()
        .and_then(|b| b.cover);
    db::set_book_cover(&conn, &norm, None)?;
    if let Some(p) = prev {
        let _ = fs::remove_file(Path::new(&p));
    }
    Ok(())
}

/// 返回一本书的自定义封面路径（未设置或文件缺失返回 None）
#[tauri::command]
fn get_book_cover(state: tauri::State<'_, db::Db>, path: String) -> Result<Option<String>, String> {
    let conn = state.0.lock().unwrap();
    let norm = norm_path(Path::new(&path));
    Ok(db::get_book(&conn, &norm)
        .ok()
        .flatten()
        .and_then(|b| b.cover)
        .filter(|c| Path::new(c).is_file()))
}

/// 刷新一本书的缓存：删除该书所有分卷的缩略图缓存与 EPUB 解包缓存
#[tauri::command]
fn refresh_book_cache(state: tauri::State<'_, db::Db>, dir: String) -> Result<(), String> {
    let vols = ebook_volumes(state, dir)?;
    for v in vols {
        let p = Path::new(&v.path);
        // 缩略图缓存
        let thumb = thumb_cache_dir().join(format!("{}.png", thumb_key(p)));
        if thumb.is_file() {
            let _ = fs::remove_file(&thumb);
        }
        // EPUB 解包缓存（重新解包后封面/分页也会跟着重建）
        if v.kind == "epub" || v.kind == "txt" {
            let epub = epub_cache_dir().join(epub_cache_key(p));
            if epub.exists() {
                let _ = fs::remove_dir_all(&epub);
            }
        }
    }
    Ok(())
}

/// 电子书目录的分卷：顶层 epub/pdf 每个文件一卷；否则每个子文件夹一卷（纯图片电子书）
fn ebook_volumes_impl(conn: &rusqlite::Connection, dir: &Path) -> Result<Vec<EbookVolume>, String> {
    let root = dir.to_path_buf();
    let last_read = db::get_book(conn, &norm_path(&root))
        .ok()
        .flatten()
        .map(|b| b.last_read_volume)
        .unwrap_or_default();
    let pos_of = |vpath: &str| -> Option<db::PositionRow> {
        db::get_position(conn, vpath).ok().flatten()
    };
    let custom_cover = |vpath: &str| -> Option<String> {
        db::get_book(conn, vpath)
            .ok()
            .flatten()
            .and_then(|b| b.cover)
            .filter(|c| Path::new(c).is_file())
    };
    let mut book_files: Vec<(String, PathBuf, String)> = Vec::new();
    let mut subdirs: Vec<(String, PathBuf)> = Vec::new();
    for item in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let name = item.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let p = item.path();
        if p.is_dir() {
            subdirs.push((name, p));
            continue;
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext == "pdf" || ext == "epub" || ext == "txt" {
            book_files.push((name, p, ext));
        }
    }
    book_files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    subdirs.sort_by(|a, b| natural_cmp(&a.0, &b.0));

    let mut vols = Vec::new();
    if !book_files.is_empty() {
        for (name, p, ext) in book_files {
            let vpath = norm_path(&p);
            // 注册分卷书目记录：避免启动清理把未打开过的分卷解包缓存当孤儿删掉
            if ext == "epub" || ext == "txt" {
                let _ = db::ensure_book(conn, &vpath, if ext == "epub" { "epub" } else { "txt" });
            }
            let pos = pos_of(&vpath);
            let thumb = custom_cover(&vpath).or_else(|| {
                if ext == "epub" {
                    // 优先缩略图缓存；首次访问时把封面缩小生成缓存，避免每次加载原图
                    cached_thumb(&p).or_else(|| {
                        let cover = epub_cover_path(&p)?;
                        make_thumb_cache(&p, Path::new(&cover))
                    })
                } else {
                    cached_thumb(&p)
                }
            });
            vols.push(EbookVolume {
                kind: ext.clone(),
                name: name.clone(),
                path: vpath.clone(),
                thumb,
                last_read: last_read == vpath,
                saved_page: pos.as_ref().map(|p| p.page),
                total: pos.as_ref().map(|p| p.total),
                saved_mode: pos.as_ref().map(|p| p.mode.clone()),
                finished: pos.as_ref().map(|p| p.finished).unwrap_or(false),
                saved_progress: pos.as_ref().and_then(|p| p.progress),
            });
        }
    } else {
        for (name, p) in subdirs {
            let vpath = norm_path(&p);
            let pos = pos_of(&vpath);
            vols.push(EbookVolume {
                kind: "imgdir".into(),
                name: name.clone(),
                path: vpath.clone(),
                thumb: custom_cover(&vpath).or_else(|| cover_or_first_image(&p)),
                last_read: last_read == vpath,
                saved_page: pos.as_ref().map(|p| p.page),
                total: pos.as_ref().map(|p| p.total).or_else(|| count_images(&p)),
                saved_mode: pos.as_ref().map(|p| p.mode.clone()),
                finished: pos.as_ref().map(|p| p.finished).unwrap_or(false),
                saved_progress: pos.as_ref().and_then(|p| p.progress),
            });
        }
        if vols.is_empty() && count_images(&root).unwrap_or(0) > 0 {
            let vpath = norm_path(&root);
            let pos = pos_of(&vpath);
            let name = root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "本书".into());
            vols.push(EbookVolume {
                kind: "imgdir".into(),
                name,
                path: vpath.clone(),
                thumb: custom_cover(&vpath).or_else(|| cover_or_first_image(&root)),
                last_read: false,
                saved_page: pos.as_ref().map(|p| p.page),
                total: count_images(&root),
                saved_mode: pos.as_ref().map(|p| p.mode.clone()),
                finished: pos.as_ref().map(|p| p.finished).unwrap_or(false),
                saved_progress: pos.as_ref().and_then(|p| p.progress),
            });
        }
    }
    Ok(vols)
}

/// 电子书目录的分卷（Tauri 命令入口）
#[tauri::command]
fn ebook_volumes(state: tauri::State<'_, db::Db>, dir: String) -> Result<Vec<EbookVolume>, String> {
    let conn = state.0.lock().unwrap();
    ebook_volumes_impl(&conn, Path::new(&dir))
}

/// 重置一本书的阅读记录：清空所有分卷的位置/已读完标记、本书最近阅读标记与浏览位置
#[tauri::command]
fn reset_book_progress(state: tauri::State<'_, db::Db>, dir: String) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    let vols = ebook_volumes_impl(&conn, Path::new(&dir))?;
    for v in &vols {
        db::delete_position(&conn, &v.path)?;
    }
    let norm = norm_path(Path::new(&dir));
    db::set_last_read(&conn, &norm, None, 0)?;
    conn.execute("DELETE FROM dir_state WHERE path = ?1", [norm])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 记录某个分卷的最后阅读位置，并更新所属书籍的「最近阅读」
#[tauri::command]
fn save_volume_position(state: tauri::State<'_, db::Db>, ebook_dir: String, volume_path: String, kind: String, page: u32, total: u32, mode: String, finished: bool, progress: Option<f64>) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::upsert_position(&conn, &volume_path, &kind, page, total, &mode, finished, progress)?;
    // 最近阅读：散装文件（书库根下的 epub/pdf）记在文件自身，目录书记在书目录（ebook_dir）
    let is_lib = db::get_library(&conn, &ebook_dir)?.is_some();
    let (book_path, book_kind) = if is_lib {
        (volume_path.clone(), entry_kind(Path::new(&volume_path)))
    } else {
        (ebook_dir.clone(), "dir")
    };
    db::ensure_book(&conn, &book_path, book_kind)?;
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    db::set_last_read(&conn, &book_path, Some(&volume_path), now)
}

#[derive(Serialize, serde::Deserialize)]
struct BookSettings {
    read_mode: Option<String>, // "scroll" | "flip"；None = 本书未设置，前端按书类型给默认
    rtl: bool,
    double_page: Option<bool>, // None = 本书未设置
    font_size: Option<u32>,    // 单书字号（NULL = 未设置，前端回退全局默认）
    font_family: Option<String>, // 单书字体（NULL = 未设置）
    theme: Option<String>,     // 单书阅读背景（NULL = 未设置，前端回退全局默认）
}

#[tauri::command]
fn read_book_settings(state: tauri::State<'_, db::Db>, ebook_dir: String, volume: Option<String>) -> BookSettings {
    let conn = state.0.lock().unwrap();
    let scope = volume.unwrap_or(ebook_dir);
    match db::get_setting(&conn, &scope) {
        Ok(Some(s)) => BookSettings {
            read_mode: Some(s.read_mode),
            rtl: s.rtl,
            double_page: Some(s.double_page),
            font_size: s.font_size,
            font_family: s.font_family,
            theme: s.theme,
        },
        _ => BookSettings {
            read_mode: None,
            rtl: false,
            double_page: None,
            font_size: None,
            font_family: None,
            theme: None,
        },
    }
}

#[tauri::command]
fn write_book_settings(state: tauri::State<'_, db::Db>, ebook_dir: String, volume: Option<String>, read_mode: String, rtl: bool, double_page: bool, font_size: Option<u32>, font_family: Option<String>, theme: Option<String>) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    let scope = volume.unwrap_or(ebook_dir);
    db::upsert_setting(&conn, &scope, &read_mode, rtl, double_page, font_size, font_family, theme)
}

#[derive(Serialize)]
struct EpubPages {
    paths: Vec<String>,
    chapter_offsets: Vec<usize>,
}

/// 提取 EPUB 各章节里的图片路径（漫画分页用），未解包时先解包
#[tauri::command]
fn epub_pages(app: tauri::AppHandle, path: String) -> Result<EpubPages, String> {
    let src = PathBuf::from(&path);
    let base = epub_cache_dir().join(epub_cache_key(&src));
    let spine_file = base.join("__cshow_spine.json");
    if !spine_file.exists() {
        if base.exists() {
            let _ = fs::remove_dir_all(&base);
        }
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        unpack_epub(&app, &src, &base)?;
    }
    let data = fs::read_to_string(&spine_file).map_err(|e| e.to_string())?;
    let meta: EpubMeta = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    let mut chapter_offsets = Vec::new();
    for href in &meta.spine {
        chapter_offsets.push(paths.len());
        let p = base.join(href);
        if let Ok(html) = fs::read_to_string(&p) {
            let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| base.clone());
            for src in all_img_srcs(&html) {
                let src_clean = src.split('#').next().unwrap_or("").to_string();
                if src_clean.is_empty() {
                    continue;
                }
                let full = dir.join(&src_clean);
                if full.is_file() && is_image(&full) {
                    // 规范化路径（去掉 ../ 段），asset 协议拒绝含 .. 的路径
                    if let Ok(c) = full.canonicalize() {
                        paths.push(norm_path(&c));
                    }
                }
            }
        }
    }
    Ok(EpubPages {
        paths,
        chapter_offsets,
    })
}

fn cover_or_first_image(dir: &Path) -> Option<String> {
    let mut imgs = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for item in rd.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let p = item.path();
            if p.is_file() && is_image(&p) {
                imgs.push(p);
            }
        }
    }
    imgs.sort_by(|a, b| {
        let an = a
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bn = b
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        natural_cmp(&an, &bn)
    });
    for p in &imgs {
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if stem == "cover" {
            return Some(norm_path(p));
        }
    }
    imgs.first().map(|p| norm_path(p))
}

fn count_images(dir: &Path) -> Option<u32> {
    fs::read_dir(dir).ok().map(|rd| {
        rd.flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                !name.starts_with('.') && e.path().is_file() && is_image(&e.path())
            })
            .count() as u32
    })
}

/// 找封面图：优先用 OPF 声明的封面（已规范化为相对解包根），
/// 否则找 cover.* 图片，再解析 cover.html/xhtml 里的 <img> / SVG <image>
fn detect_cover(base: &Path, declared: Option<String>) -> Option<String> {
    if let Some(c) = declared {
        if !c.is_empty() {
            let p = base.join(&c);
            if p.is_file() && is_image(&p) {
                return Some(c);
            }
        }
    }
    if let Some(rel) = walk_find(base, base, 0, |p| {
        is_image(p)
            && p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("cover"))
                .unwrap_or(false)
    }) {
        return Some(rel);
    }
    if let Some(rel) = walk_find(base, base, 0, |p| {
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("xhtml"))
            .unwrap_or(false)
            && p.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("cover"))
                .unwrap_or(false)
    }) {
        let html_path = base.join(&rel);
        if let Ok(html) = fs::read_to_string(&html_path) {
            if let Some(src) = first_img_src(&html) {
                let full = html_path
                    .parent()
                    .map(|par| par.join(&src))
                    .unwrap_or_else(|| src.clone().into());
                if full.is_file() && is_image(&full) {
                    if let Ok(rel2) = full.strip_prefix(base) {
                        // 规范化相对路径（去掉 ../ 段），保证 book:///文件路径可用
                        let rel3 = rel2.to_string_lossy().into_owned();
                        return Some(join_rel("", &rel3));
                    }
                }
            }
        }
    }
    None
}

fn walk_find<F>(base: &Path, dir: &Path, depth: usize, pred: F) -> Option<String>
where
    F: Fn(&Path) -> bool + Copy,
{
    if depth > 8 {
        return None;
    }
    let rd = fs::read_dir(dir).ok()?;
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by(|a, b| {
        let an = a.file_name().to_string_lossy().into_owned();
        let bn = b.file_name().to_string_lossy().into_owned();
        natural_cmp(&an, &bn)
    });
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            if let Some(rel) = walk_find(base, &p, depth + 1, pred) {
                return Some(rel);
            }
        } else if pred(&p) {
            if let Ok(rel) = p.strip_prefix(base) {
                return Some(rel.to_string_lossy().into_owned());
            }
        }
    }
    None
}

/// 收集 HTML 中的图片引用：支持 `<img src>` 与 SVG `<image xlink:href|href>`。
/// 从原始文本取属性值（保留大小写），并跳过属性前带 `<` 的前缀，避免误匹配。
fn all_img_srcs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < lower.len() {
        let rest = &lower[idx..];
        let p_img = rest.find("<img");
        let p_image = rest.find("<image");
        let (pos, _tag_len) = match (p_img, p_image) {
            (Some(a), Some(b)) if a <= b => (a, 4),
            (Some(a), None) => (a, 4),
            (None, Some(b)) => (b, 6),
            (None, None) => break,
            (Some(_), Some(b)) => (b, 6),
        };
        let start = idx + pos;
        let tag = &html[start..];
        let Some(gt) = tag.find('>') else { break };
        let open_tag = &tag[..gt + 1];
        let val = attr_value(open_tag, "src")
            .or_else(|| attr_value(open_tag, "xlink:href"))
            .or_else(|| attr_value(open_tag, "href"));
        if let Some(v) = val {
            if !v.is_empty() {
                out.push(v);
            }
        }
        idx = start + 1;
    }
    out
}

fn first_img_src(html: &str) -> Option<String> {
    all_img_srcs(html).into_iter().next()
}

/// 章节高度测量脚本：无 `<` 字符，避免破坏 XHTML 解析
const HEIGHT_SCRIPT: &str = "<script>function cshowH(){var h=document.documentElement.scrollHeight;parent.postMessage({cshowH:h},\"*\")}onload=cshowH;setTimeout(cshowH,200);addEventListener(\"DOMContentLoaded\",cshowH);document.addEventListener(\"load\",cshowH,true);</script>";

fn inject_height_script(file: &Path) {
    let Ok(bytes) = fs::read(file) else { return };
    let lower = bytes.to_ascii_lowercase();
    let pos = lower
        .windows(7)
        .position(|w| w == b"</body>")
        .or_else(|| lower.windows(7).position(|w| w == b"</html>"));
    let insert_at = pos.unwrap_or(bytes.len());
    let mut out = Vec::with_capacity(bytes.len() + HEIGHT_SCRIPT.len());
    out.extend_from_slice(&bytes[..insert_at]);
    out.extend_from_slice(HEIGHT_SCRIPT.as_bytes());
    out.extend_from_slice(&bytes[insert_at..]);
    let _ = fs::write(file, out);
}

fn emit_epub_progress(app: &tauri::AppHandle, done: usize, total: usize) {
    let pct = if total == 0 {
        100
    } else {
        ((done * 100) / total) as u8
    };
    let _ = app.emit("epub-progress", serde_json::json!({ "percent": pct }));
}

// ---- 文字 EPUB：排版/分页脚本与目录解析 ----

const READER_CSS: &str = include_str!("../../ui/reader.css");
const READER_JS: &str = include_str!("../../ui/reader.js");
/// 章节页注入 reader 样式/脚本（绝对 book:// 地址，规避 EPUB <base> 标签的干扰）
#[cfg(target_os = "android")]
const READER_INJECT: &str = "<link rel=\"stylesheet\" href=\"http://book.localhost/__cshow_reader.css\"><script src=\"http://book.localhost/__cshow_reader.js\"></script>";
#[cfg(not(target_os = "android"))]
const READER_INJECT: &str = "<link rel=\"stylesheet\" href=\"book://localhost/__cshow_reader.css\"><script src=\"book://localhost/__cshow_reader.js\"></script>";

/// book:// 章节页的 CSP：默认禁内联脚本，仅放行两个特例——
/// 1) reader.js / 高度脚本等 book:// 同源外部脚本（'self'）；
/// 2) 图像书章节注入的高度测量脚本（下方 sha256 哈希精确匹配，其余内联脚本仍被拦截）。
/// 同时禁脚本外联与 object/表单，保留书内排版（内联样式）与图片/字体（含远程图）。
/// 注意：哈希按 CSP 规范只覆盖 <script> 内部内容（不含标签本身）；若修改 HEIGHT_SCRIPT，
/// 必须同步更新这里的 sha256（由 height_script_csp_hash 测试兜底）。
const BOOK_CSP: &str = "default-src 'self' data: blob:; script-src 'self' \
'sha256-8pNU3LyEUk8SdG36gez/jU6eIglaW7EO3WX+5UkmcdI='; \
style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; \
font-src 'self' data:; media-src 'self' data: blob: https: http:; \
connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";

/// 去掉 HTML 标签，仅保留文本（文字量估算与目录标签用）
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ => {
                if !in_tag {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// 提取 HTML 中独立段落的 (开标签起始, 段落结束, 去空白纯文本)。
/// 只匹配标签名为 p 的段落（排除 <pre 等以 p 开头的标签）。
fn paragraphs_with_text(html: &str) -> Vec<(usize, usize, String)> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < lower.len() {
        let Some(pos) = lower[idx..].find("<p") else { break };
        let start = idx + pos;
        let after = &lower[start + 2..];
        let name_end = after
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(after.len());
        if !after[..name_end].is_empty() {
            idx = start + 2;
            continue;
        }
        let Some(gt) = lower[start..].find('>') else { break };
        let content_start = start + gt + 1;
        let Some(pe) = lower[content_start..].find("</p") else { break };
        let content_end = content_start + pe;
        let text = strip_tags(&html[content_start..content_end]);
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        out.push((start, content_end + 4, compact));
        idx = content_end + 4;
    }
    out
}

/// 页码模式：去空白后形如 Page|123 / Page123 / page 123 等（PDF 转 EPUB 的页脚）
fn is_page_number(compact: &str) -> bool {
    let lower = compact.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("page") else { return false };
    let rest = rest.trim_start_matches(['|', '｜', '·', ':', '：', '-', '—', '–']);
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// 页眉/页脚判断：页码段落，或全书重复 >=5 次的"标题型"短段落（如"书名 – 作者"）。
/// 标题型要求足够长（>=20 字符）、不含引号（排除对话碎片）、不以句读结尾（排除完整短句），
/// 避免把 PDF 转 EPUB 常见的正文行碎片（"him."、"”"、"said Ron." 等）误当页眉。
fn is_header_like(text: &str, count: usize) -> bool {
    if is_page_number(text) {
        return true;
    }
    if text.len() >= 20 && count >= 5 {
        let has_quote = text.contains('"')
            || text.contains('“')
            || text.contains('”')
            || text.contains('\'')
            || text.contains('’')
            || text.contains('‘');
        let ends_punct = text.ends_with(['.', '!', '?', '。', '！', '？']);
        if !has_quote && !ends_punct {
            return true;
        }
    }
    false
}

/// 在段落开标签里注入隐藏标记（保留原有属性）
fn inject_hdr_mark(html: &str, p_start: usize) -> String {
    let rest = &html[p_start..];
    let Some(gt) = rest.find('>') else { return html.to_string() };
    let mut out = String::with_capacity(html.len() + 20);
    out.push_str(&html[..p_start + gt]);
    out.push_str(" data-cshow-hdr=\"\"");
    out.push_str(&html[p_start + gt..]);
    out
}

/// 从单个开标签文本中取属性值（属性名不区分大小写），如 <a href="x">
fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{attr}=");
    let pos = lower.find(&needle)?;
    let after = tag[pos + needle.len()..].trim_start();
    let q = after.chars().next()?;
    let val = if q == '"' || q == '\'' {
        let inner = &after[1..];
        let end = inner.find(q)?;
        inner[..end].to_string()
    } else {
        let end = after
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(after.len());
        after[..end].to_string()
    };
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

/// 把相对路径拼到基准目录上并规范化（处理 ./ 与 ../）
fn join_rel(base: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = base
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(seg),
        }
    }
    parts.join("/")
}

/// 相对路径的父目录（无 / 则为空串）
fn parent_dir(rel: &str) -> String {
    match rel.rfind('/') {
        Some(p) => rel[..p].to_string(),
        None => String::new(),
    }
}

/// 统计正文文字量（去标签、非空白字符数）与图片量，用于文字/图像书判定。
/// 图片量同时计入 <img>、SVG <image> 与内联 <svg>，避免纯 SVG 漫画被误判为文字书。
fn text_and_image_stats(html: &str) -> (usize, usize) {
    let text = strip_tags(html);
    let chars = text.chars().filter(|c| !c.is_whitespace()).count();
    let lower = html.to_ascii_lowercase();
    let imgs = lower.matches("<img").count()
        + lower.matches("<image").count()
        + lower.matches("<svg").count();
    (chars, imgs)
}

/// 从 nav(XHTML) 或 NCX 文本中提取平铺目录 (label, href)
fn extract_toc_links(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();
    // EPUB3 nav：<a href="...">label</a>
    let mut idx = 0usize;
    while idx < lower.len() {
        let Some(pos) = lower[idx..].find("<a") else { break };
        let start = idx + pos;
        let Some(gt) = text[start..].find('>') else { break };
        let tag = &text[start..start + gt + 1];
        if let Some(href) = attr_value(tag, "href") {
            let content_start = start + gt + 1;
            let Some(endrel) = lower[content_start..].find("</a") else { break };
            let content_end = content_start + endrel;
            let label = strip_tags(&text[content_start..content_end]).trim().to_string();
            if !label.is_empty() {
                out.push((label, href));
            }
            idx = content_end + 3;
        } else {
            idx = start + gt + 1;
        }
    }
    if !out.is_empty() {
        return out;
    }
    // EPUB2 NCX：<navPoint><navLabel><text>label</text></navLabel><content src="..."/>
    idx = 0;
    while idx < lower.len() {
        let Some(np) = lower[idx..].find("<navpoint") else { break };
        let np_start = idx + np;
        let Some(np_end_rel) = lower[np_start..].find("</navpoint>") else { break };
        let np_end = np_start + np_end_rel;
        let block = &text[np_start..np_end];
        let label = block
            .find("<text>")
            .and_then(|p| {
                let s = p + 6;
                block[s..]
                    .find("</text>")
                    .map(|e| strip_tags(&block[s..s + e]).trim().to_string())
            })
            .unwrap_or_default();
        let href = attr_value(block, "src").unwrap_or_default();
        if !label.is_empty() && !href.is_empty() {
            out.push((label, href));
        }
        idx = np_end + 10;
    }
    out
}

/// 解析目录：返回平铺条目（label + 章节下标）与每章标题（与 spine 对齐）
fn parse_toc(
    base: &Path,
    opf_dir: &str,
    items: &[(String, String, String)],
    spine: &[String],
    ncx_id: &str,
) -> (Vec<TocEntry>, Vec<String>) {
    // spine 由调用方统一规范化为相对解包根的路径（相对 OPF 目录的 href 已按 opf_dir 展开）
    let spine_rel: Vec<String> = spine.to_vec();

    let mut toc_file: Option<String> = None;
    for (_, href, props) in items {
        if props
            .split_whitespace()
            .any(|p| p.eq_ignore_ascii_case("nav"))
        {
            toc_file = Some(href.clone());
            break;
        }
    }
    if toc_file.is_none() {
        if !ncx_id.is_empty() {
            if let Some((_, href, _)) = items.iter().find(|(id, _, _)| id == ncx_id) {
                toc_file = Some(href.clone());
            }
        }
        if toc_file.is_none() {
            let d = base.join(opf_dir).join("toc.ncx");
            if d.is_file() {
                toc_file = Some(join_rel(opf_dir, "toc.ncx"));
            }
        }
    }

    // 全书文件内锚点 → 章节：修正 Z-Library 类坏 NCX——所有 navPoint 的 src 都指向
    // 第一个文件，但 filepos 锚点实际分散在各章节文件里；按锚点真实位置重定向目录跳转。
    // 页面锚点（p20/p34…）可能跨文件重复：只对“引用文件里不存在该锚点”的条目重定向，
    // 否则会把后续册的章节错误拽回第一个文件。
    let mut anchor_chapter: HashMap<String, usize> = HashMap::new();
    let mut chapter_anchors: Vec<HashSet<String>> = vec![HashSet::new(); spine.len()];
    for (idx, href) in spine.iter().enumerate() {
        if let Ok(html) = fs::read_to_string(base.join(href)) {
            for attr in ["id", "name"] {
                let needle = format!("{attr}=\"");
                let mut start = 0usize;
                while let Some(p) = html[start..].find(&needle) {
                    let s = start + p + needle.len();
                    match html[s..].find('"') {
                        Some(e) => {
                            let a = &html[s..s + e];
                            if !a.is_empty() {
                                chapter_anchors[idx].insert(a.to_string());
                                anchor_chapter.entry(a.to_string()).or_insert(idx);
                            }
                            start = s + e;
                        }
                        None => break,
                    }
                }
            }
        }
    }

    let mut links: Vec<(String, usize, Option<String>)> = Vec::new();
    if let Some(tf) = &toc_file {
        // NCX/nav 的 href 同样相对 OPF 目录，先规范化成相对解包根再读
        let tf_abs = join_rel(opf_dir, tf);
        let toc_dir = parent_dir(&tf_abs);
        if let Ok(text) = fs::read_to_string(base.join(&tf_abs)) {
            for (label, href) in extract_toc_links(&text) {
                let (clean, anchor) = match href.split_once('#') {
                    Some((c, a)) => (c.to_string(), Some(a.to_string())),
                    None => (href.clone(), None),
                };
                if clean.is_empty() {
                    continue;
                }
                let abs = join_rel(&toc_dir, &clean);
                if let Some(ch) = spine_rel.iter().position(|s| s == &abs) {
                    // 锚点不在引用文件里但确实存在于其他章节：重定向到实际章节
                    let mut target_ch = ch;
                    // 引用文件里就有该锚点 → 保留原章节（避免跨文件重复锚点被拽回首文件）
                    if let Some(a) = &anchor {
                        if !chapter_anchors[ch].contains(a) {
                            if let Some(&real_ch) = anchor_chapter.get(a) {
                                if real_ch != ch {
                                    target_ch = real_ch;
                                }
                            }
                        }
                    }
                    links.push((label, target_ch, anchor));
                }
            }
        }
    }

    let mut chapter_titles: Vec<String> = vec![String::new(); spine.len()];
    for (label, ch, _) in &links {
        if chapter_titles[*ch].is_empty() {
            chapter_titles[*ch] = label.clone();
        }
    }
    for (i, h) in spine.iter().enumerate() {
        if chapter_titles[i].is_empty() {
            chapter_titles[i] = h
                .rsplit('/')
                .next()
                .unwrap_or(h)
                .split('.')
                .next()
                .unwrap_or("")
                .to_string();
        }
    }

    let toc: Vec<TocEntry> = links
        .into_iter()
        .map(|(label, chapter, anchor)| TocEntry { label, chapter, anchor })
        .collect();
    (toc, chapter_titles)
}

/// 给文字书章节注入 reader 样式与脚本（绝对 book:// 地址，规避 EPUB <base> 标签的干扰）
fn inject_reader_files(file: &Path) {
    let Ok(bytes) = fs::read(file) else { return };
    let lower = bytes.to_ascii_lowercase();
    let pos = lower
        .windows(7)
        .position(|w| w == b"</body>")
        .or_else(|| lower.windows(7).position(|w| w == b"</html>"))
        .unwrap_or(bytes.len());
    let mut out = Vec::with_capacity(bytes.len() + READER_INJECT.len());
    out.extend_from_slice(&bytes[..pos]);
    out.extend_from_slice(READER_INJECT.as_bytes());
    out.extend_from_slice(&bytes[pos..]);
    let _ = fs::write(file, out);
}

fn unpack_epub(app: &tauri::AppHandle, src: &Path, base: &Path) -> Result<EpubMeta, String> {
    let file = fs::File::open(&src).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let total = zip.len();
    for i in 0..zip.len() {
        emit_epub_progress(app, i, total);
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(out_path) = entry.enclosed_name() else {
            continue;
        };
        let out_path = base.join(out_path);
        if entry.is_dir() {
            let _ = fs::create_dir_all(&out_path);
        } else {
            if let Some(parent) = out_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    emit_epub_progress(app, total, total);
    parse_unpacked_epub(base)
}

/// 解析已解包的 EPUB 目录：OPF → spine / 目录 / 封面 / 章节统计 / 文字书判定。
/// 关键约定：spine、NCX、封面等 manifest 里的 href 都是相对 OPF 所在目录的，
/// 这里统一规范化为相对解包根（OPF 可能位于子目录，如 OEBPS/），
/// 这样 book:// 协议与后续所有 base.join 都能正确命中文件。
fn parse_unpacked_epub(base: &Path) -> Result<EpubMeta, String> {
    let container = fs::read_to_string(base.join("META-INF/container.xml"))
        .map_err(|e| format!("container.xml: {e}"))?;
    let cdoc = roxmltree::Document::parse(&container).map_err(|e| e.to_string())?;
    let mut opf_rel = String::new();
    for node in cdoc.descendants() {
        if node.has_tag_name("rootfile") {
            if let Some(fp) = node.attribute("full-path") {
                opf_rel = fp.to_string();
                break;
            }
        }
    }
    if opf_rel.is_empty() {
        return Err("EPUB 中未找到 OPF 文件".into());
    }
    let opf_dir = parent_dir(&opf_rel);

    let opf = fs::read_to_string(base.join(&opf_rel)).map_err(|e| format!("opf: {e}"))?;
    let odoc = roxmltree::Document::parse(&opf).map_err(|e| e.to_string())?;
    let mut title = String::new();
    let mut manifest: Vec<(String, String)> = Vec::new();
    let mut spine: Vec<String> = Vec::new();
    let mut items: Vec<(String, String, String)> = Vec::new(); // id, href, properties
    let mut meta_cover_id = String::new();
    let mut ncx_id = String::new();
    for node in odoc.descendants() {
        match node.tag_name().name() {
            "title" => {
                if title.is_empty() {
                    title = node.text().unwrap_or("").trim().to_string();
                }
            }
            "item" => {
                let id = node.attribute("id").unwrap_or("").to_string();
                let href = node.attribute("href").unwrap_or("").to_string();
                let props = node.attribute("properties").unwrap_or("").to_string();
                items.push((id.clone(), href.clone(), props));
                manifest.push((
                    id,
                    href,
                ));
            }
            "meta" => {
                if node.attribute("name").unwrap_or("") == "cover" {
                    meta_cover_id = node.attribute("content").unwrap_or("").to_string();
                }
            }
            "spine" => {
                ncx_id = node.attribute("toc").unwrap_or("").to_string();
            }
            "itemref" => {
                let idref = node.attribute("idref").unwrap_or("").to_string();
                if let Some((_, href)) = manifest.iter().find(|(id, _)| *id == idref) {
                    spine.push(href.split('#').next().unwrap_or("").to_string());
                }
            }
            _ => {}
        }
    }
    if spine.is_empty() {
        return Err("EPUB 没有可读章节".into());
    }

    // spine 的 href 相对 OPF 目录，统一规范化为相对解包根（OPF 在子目录时展开为 OEBPS/Text/...）
    let spine: Vec<String> = spine.iter().map(|h| join_rel(&opf_dir, h)).collect();

    let declared = if !meta_cover_id.is_empty() {
        items
            .iter()
            .find(|(id, _, _)| *id == meta_cover_id)
            .map(|(_, href, _)| href.clone())
    } else {
        items
            .iter()
            .find(|(_, _, props)| {
                props
                    .split_whitespace()
                    .any(|x| x.eq_ignore_ascii_case("cover-image"))
            })
            .map(|(_, href, _)| href.clone())
            .or_else(|| {
                items
                    .iter()
                    .find(|(id, _, _)| id.eq_ignore_ascii_case("cover"))
                    .map(|(_, href, _)| href.clone())
            })
    }
    .map(|h| h.split('#').next().unwrap_or("").to_string())
    .filter(|h| !h.is_empty());
    let cover = detect_cover(base, declared.map(|h| join_rel(&opf_dir, &h)));

    // 判定文字书 / 图像书：按正文文字量与图片数的比例；同时收集每章字符数（用于阅读百分比）
    let mut total_chars = 0usize;
    let mut total_imgs = 0usize;
    let mut chapter_lengths: Vec<usize> = Vec::with_capacity(spine.len());
    for href in &spine {
        let p = base.join(href);
        let mut chars = 0usize;
        if let Ok(html) = fs::read_to_string(&p) {
            let (c, imgs) = text_and_image_stats(&html);
            chars = c;
            total_chars += c;
            total_imgs += imgs;
        }
        chapter_lengths.push(chars);
    }
    let text_book = total_imgs == 0
        || (total_chars >= 500 && total_chars >= total_imgs.saturating_mul(30));

    // 检测并标记固定页眉/页脚（PDF 转 EPUB 常见，如"P a g e | 123"、"书名 – 作者"每页重复）：
    // 1) 独立页码段落；2) 全书重复 >=5 次的独立短段落。
    // 标记后由 reader 隐藏，不污染正文。
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut chapter_paras: Vec<Vec<(usize, usize, String)>> = Vec::with_capacity(spine.len());
    for href in &spine {
        let p = base.join(href);
        let paras = fs::read_to_string(&p)
            .map(|html| paragraphs_with_text(&html))
            .unwrap_or_default();
        for (_, _, t) in &paras {
            if !t.is_empty() && t.len() <= 80 {
                *counts.entry(t.clone()).or_insert(0) += 1;
            }
        }
        chapter_paras.push(paras);
    }
    for (href, paras) in spine.iter().zip(&chapter_paras) {
        let p = base.join(href);
        let Ok(html) = fs::read_to_string(&p) else { continue };
        let mut out = html;
        for (start, _, text) in paras.iter().rev() {
            let is_hdr = !text.is_empty()
                && text.len() <= 80
                && is_header_like(text, counts.get(text).copied().unwrap_or(0));
            if is_hdr {
                out = inject_hdr_mark(&out, *start);
            }
        }
        let _ = fs::write(&p, out);
    }

    let (toc, chapter_titles) = parse_toc(base, &opf_dir, &items, &spine, &ncx_id);

    // 文字书：写入 reader 样式/脚本；图像书：仅注入高度测量脚本
    if text_book {
        let _ = fs::write(base.join("__cshow_reader.css"), READER_CSS);
        let _ = fs::write(base.join("__cshow_reader.js"), READER_JS);
    }
    for href in &spine {
        let p = base.join(href);
        if let Some(ext) = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
        {
            if matches!(ext.as_str(), "html" | "htm" | "xhtml") {
                if text_book {
                    inject_reader_files(&p);
                } else {
                    inject_height_script(&p);
                }
            }
        }
    }

    let meta = EpubMeta {
        base_dir: norm_path(base),
        spine,
        title,
        cover,
        text_book,
        toc,
        chapter_titles,
        chapter_lengths,
    };
    let spine_file = base.join("__cshow_spine.json");
    let _ = fs::write(
        &spine_file,
        serde_json::to_string(&meta).map_err(|e| e.to_string())?,
    );
    Ok(meta)
}

// ---- TXT 电子书：识别卷/章节结构，生成虚拟文字书缓存 ----

/// 按常见编码解码 txt 全文（UTF-8 BOM / UTF-16 / UTF-8 / GBK(GB18030) 兜底）
fn decode_txt(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let (le, body) = if bytes.starts_with(&[0xFF, 0xFE]) {
            (true, &bytes[2..])
        } else {
            (false, &bytes[2..])
        };
        let mut units = Vec::with_capacity(body.len() / 2);
        let mut it = body.chunks_exact(2);
        if le {
            for c in &mut it {
                units.push(u16::from_le_bytes([c[0], c[1]]));
            }
        } else {
            for c in &mut it {
                units.push(u16::from_be_bytes([c[0], c[1]]));
            }
        }
        return String::from_utf16_lossy(&units);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (decoded, _, _) = encoding_rs::GB18030.decode(bytes);
    decoded.into_owned()
}

/// 中文数字 → 数值（"一百二十三" → 123；纯阿拉伯数字直接解析）
fn cjk_num_value(s: &str) -> Option<usize> {
    if let Ok(n) = s.parse::<usize>() {
        return Some(n);
    }
    let digits = [
        ('零', 0), ('一', 1), ('二', 2), ('三', 3), ('四', 4),
        ('五', 5), ('六', 6), ('七', 7), ('八', 8), ('九', 9),
    ];
    let units = [('十', 10usize), ('百', 100), ('千', 1000)];
    let mut total = 0usize;
    let mut cur = 0usize;
    for ch in s.chars() {
        if let Some((_, d)) = digits.iter().find(|(c, _)| *c == ch) {
            cur = *d;
        } else if let Some((_, u)) = units.iter().find(|(c, _)| *c == ch) {
            total += (if cur == 0 { 1 } else { cur }) * u;
            cur = 0;
        } else {
            return None;
        }
    }
    Some(total + cur)
}

#[derive(Clone, Copy, PartialEq)]
enum TxtHeadingKind {
    Volume,
    Chapter,
}

struct TxtHeading {
    line: usize,
    raw: String,
    kind: TxtHeadingKind,
}

/// 扫描 txt 中的卷/章节标题行（整行匹配，长度受限避免误判正文句子）
fn scan_txt_headings(lines: &[&str]) -> Vec<TxtHeading> {
    let num = r"[0-9零一二三四五六七八九十百千]+";
    // 章节标题允许紧跟在“章”后（部分书无空格，如“第四章九天”），
    // 解析后统一补一个空格（label 存规范化标题）
    let chapter_re = Regex::new(&format!(r"^\s*第\s*({num})\s*章\s*[　:：]*\s*(.*)$")).unwrap();
    let volume_re = Regex::new(&format!(r"^\s*第\s*({num})\s*(卷|部|册|集)(?:[\s　]+.*)?$")).unwrap();
    // 行内章节标记：用于“第一卷 东林皆石 第一章 钟楼街的游行”这类卷章同行的书
    let inline_chapter_re = Regex::new(&format!(r"第\s*({num})\s*章\s*[　:：]*\s*(.*)$")).unwrap();
    let volume_dot_re = Regex::new(&format!(r"^[^\r\n。！？]{{1,24}}·第\s*({num})\s*(册|部|集)\s*$")).unwrap();
    let mut out = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.len() > 100 {
            continue;
        }
        if let Some(c) = chapter_re.captures(line) {
            if cjk_num_value(&c[1]).is_some() {
                let num_s = c[1].to_string();
                let title = c.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                let label = if title.is_empty() {
                    format!("第{num_s}章")
                } else {
                    format!("第{num_s}章 {title}")
                };
                out.push(TxtHeading { line: i, raw: label, kind: TxtHeadingKind::Chapter });
                continue;
            }
        }
        if let Some(c) = volume_re.captures(line) {
            if cjk_num_value(&c[1]).is_some() {
                // 卷标签：若同行还带章节（“第一卷 东林皆石 第一章 钟楼街的游行”），
                // 卷标签截到章节前，并同时识别出该章节（标题统一补空格）
                let mut vol_label = line.to_string();
                if let Some(cc) = inline_chapter_re.captures(line) {
                    let start = cc.get(0).map(|m| m.start()).unwrap_or(0);
                    vol_label = line[..start].trim().to_string();
                }
                out.push(TxtHeading { line: i, raw: vol_label, kind: TxtHeadingKind::Volume });
                if let Some(cc) = inline_chapter_re.captures(line) {
                    let num_s = cc[1].to_string();
                    let title = cc.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                    let ch_label = if title.is_empty() {
                        format!("第{num_s}章")
                    } else {
                        format!("第{num_s}章 {title}")
                    };
                    out.push(TxtHeading { line: i, raw: ch_label, kind: TxtHeadingKind::Chapter });
                }
                continue;
            }
        }
        if let Some(c) = volume_dot_re.captures(line) {
            if cjk_num_value(&c[1]).is_some() {
                out.push(TxtHeading { line: i, raw: line.to_string(), kind: TxtHeadingKind::Volume });
            }
        }
    }
    out
}

struct TxtVolume {
    label: String,
}

struct TxtChapter {
    file: String,
    title: String,
    start: usize,
    end: usize,
    chars: usize,
}

fn make_txt_chapter(lines: &[&str], h: &TxtHeading, end: usize, idx: usize) -> TxtChapter {
    let start = h.line + 1;
    let chars = lines[start..end.min(lines.len())]
        .iter()
        .map(|l| l.chars().filter(|c| !c.is_whitespace()).count())
        .sum();
    TxtChapter {
        file: format!("txt_chap_{:04}.xhtml", idx + 1),
        title: h.raw.clone(),
        start,
        end,
        chars,
    }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// 无任何章节标题的兜底：按约 5000 行切分，避免生成超大单章
fn fallback_txt_chapters(lines: &[&str]) -> Vec<TxtChapter> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut idx = 1usize;
    while start < lines.len() {
        let end = (start + 5000).min(lines.len());
        let chars = lines[start..end].iter().map(|l| l.chars().filter(|c| !c.is_whitespace()).count()).sum();
        out.push(TxtChapter {
            file: format!("txt_chap_{idx:04}.xhtml"),
            title: format!("（第{idx}部分）"),
            start,
            end,
            chars,
        });
        start = end;
        idx += 1;
    }
    out
}

/// 解析 txt：卷标记（第X卷/部/册/集、书名·第X册）分组章节，生成章节 XHTML + spine + 目录
fn parse_txt_book(src: &Path, base: &Path) -> Result<EpubMeta, String> {
    let bytes = fs::read(src).map_err(|e| e.to_string())?;
    let text = decode_txt(&bytes);
    let lines: Vec<&str> = text.split('\n').collect();

    let headings = scan_txt_headings(&lines);
    let mut volumes: Vec<TxtVolume> = Vec::new();
    let mut chapters: Vec<TxtChapter> = Vec::new();
    let mut toc: Vec<TocEntry> = Vec::new();
    if headings.is_empty() {
        chapters = fallback_txt_chapters(&lines);
        volumes.push(TxtVolume { label: String::new() });
        for (i, ch) in chapters.iter().enumerate() {
            toc.push(TocEntry { label: ch.title.clone(), chapter: i, anchor: None });
        }
    } else {
        volumes.push(TxtVolume { label: String::new() });
        let mut cur_vol = 0usize;
        let mut vol_chapter_counts = vec![0usize];
        let mut pending: Option<&TxtHeading> = None;
        for h in &headings {
            if h.kind == TxtHeadingKind::Volume {
                // 卷章同行且卷名与上一个卷相同（如“第一卷 东林皆石 第二章 …”每章都带卷前缀）：
                // 不重复建卷，只把上一章收尾（continue 前完成）
                if let Some(last) = volumes.last() {
                    if last.label == h.raw {
                        if let Some(p) = pending.take() {
                            let idx = chapters.len();
                            chapters.push(make_txt_chapter(&lines, p, h.line, idx));
                            toc.push(TocEntry { label: p.raw.clone(), chapter: idx, anchor: None });
                            vol_chapter_counts[cur_vol] += 1;
                        }
                        continue;
                    }
                }
                if let Some(p) = pending.take() {
                    let idx = chapters.len();
                    chapters.push(make_txt_chapter(&lines, p, h.line, idx));
                    toc.push(TocEntry { label: p.raw.clone(), chapter: idx, anchor: None });
                    vol_chapter_counts[cur_vol] += 1;
                }
                volumes.push(TxtVolume { label: h.raw.clone() });
                toc.push(TocEntry { label: h.raw.clone(), chapter: chapters.len(), anchor: None });
                vol_chapter_counts.push(0);
                cur_vol = volumes.len() - 1;
            } else {
                if let Some(p) = pending.take() {
                    let idx = chapters.len();
                    chapters.push(make_txt_chapter(&lines, p, h.line, idx));
                    toc.push(TocEntry { label: p.raw.clone(), chapter: idx, anchor: None });
                    vol_chapter_counts[cur_vol] += 1;
                }
                pending = Some(h);
            }
        }
        if let Some(p) = pending.take() {
            let idx = chapters.len();
            chapters.push(make_txt_chapter(&lines, p, lines.len(), idx));
            toc.push(TocEntry { label: p.raw.clone(), chapter: idx, anchor: None });
            vol_chapter_counts[cur_vol] += 1;
        }
        // 有显式卷标记且默认卷没有任何章节时，去掉空默认卷
        if volumes.len() > 1 && volumes[0].label.is_empty() && vol_chapter_counts[0] == 0 {
            volumes.remove(0);
        }
    }
    if chapters.is_empty() {
        return Err("TXT 没有可读章节".into());
    }

    fs::create_dir_all(base).map_err(|e| e.to_string())?;
    // 段落格式判定：绝大多数非空行以全角空格缩进开头 → 每行一段；
    // 否则按「空行分段 + 缩进开新段 + 续行合并」处理（兼容硬换行书）
    let non_empty: Vec<&str> = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let indent_cnt = non_empty
        .iter()
        .filter(|l| l.starts_with('\u{3000}') || l.starts_with(' ') || l.starts_with('\t'))
        .count();
    let indent_dominant = !non_empty.is_empty() && indent_cnt * 2 >= non_empty.len();
    let mut spine = Vec::with_capacity(chapters.len());
    let mut chapter_titles = Vec::with_capacity(chapters.len());
    let mut chapter_lengths = Vec::with_capacity(chapters.len());
    for ch in &chapters {
        let mut html = String::with_capacity(4096);
        html.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>");
        html.push_str(&escape_html(&ch.title));
        html.push_str("</title></head><body class=\"cshow-txt\">\n<h2>");
        html.push_str(&escape_html(&ch.title));
        html.push_str("</h2>\n");
        let mut para: Vec<&str> = Vec::new();
        let flush = |html: &mut String, para: &mut Vec<&str>| {
            if para.is_empty() {
                return;
            }
            html.push_str("<p>");
            for seg in para.drain(..) {
                html.push_str(&escape_html(seg));
            }
            html.push_str("</p>\n");
        };
        for raw in &lines[ch.start..ch.end.min(lines.len())] {
            let has_indent = raw.starts_with('\u{3000}') || raw.starts_with(' ') || raw.starts_with('\t');
            let line = raw.trim();
            if line.is_empty() {
                flush(&mut html, &mut para);
            } else if indent_dominant {
                // 每行一段：去行首缩进，立即成段
                flush(&mut html, &mut para);
                para.push(line);
                flush(&mut html, &mut para);
            } else if has_indent {
                // 缩进行开新段，续行并入当前段
                flush(&mut html, &mut para);
                para.push(line);
            } else {
                para.push(line);
            }
        }
        flush(&mut html, &mut para);
        // reader 脚本必须注入在 </body> 前：reader.js 在脚本执行时就读取 document.body，
        // 若放在 <head> 会拿到 null，翻页/背景/高度全部失效
        html.push_str(READER_INJECT);
        html.push_str("</body></html>\n");
        fs::write(base.join(&ch.file), html).map_err(|e| e.to_string())?;
        spine.push(ch.file.clone());
        chapter_titles.push(ch.title.clone());
        chapter_lengths.push(ch.chars);
    }

    // 书名/作者：首行《书名》 作者：xxx，缺失用文件名兜底
    let file_stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut title = file_stem.clone();
    for l in lines.iter().take(20) {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(c) = Regex::new(r"《([^》]+)》").unwrap().captures(l) {
            title = c[1].to_string();
        }
        if l.contains("内容简介") {
            break;
        }
    }

    let meta = EpubMeta {
        base_dir: norm_path(base),
        spine,
        title,
        cover: None,
        text_book: true,
        toc,
        chapter_titles,
        chapter_lengths,
    };
    let _ = fs::write(
        &base.join("__cshow_spine.json"),
        serde_json::to_string(&meta).map_err(|e| e.to_string())?,
    );
    Ok(meta)
}

#[tauri::command]
fn open_epub(app: tauri::AppHandle, path: String) -> Result<EpubMeta, String> {
    let src = PathBuf::from(&path);
    // 注册书目记录：让该 EPUB 的解包缓存在启动清理时被保留
    let is_txt = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("txt"))
        .unwrap_or(false);
    {
        let state = app.state::<db::Db>();
        let conn = state.0.lock().unwrap();
        let _ = db::ensure_book(&conn, &norm_path(&src), if is_txt { "txt" } else { "epub" });
        maybe_fill_preset_meta(&conn, &norm_path(&src));
    }
    let base = epub_cache_dir().join(epub_cache_key(&src));
    // 先更新 book:// 的服务根，再走缓存：否则第二次打开会拿到旧书内容
    *app.state::<BookState>().0.lock().unwrap() = Some(base.clone());
    let spine_file = base.join("__cshow_spine.json");
    if spine_file.exists() {
        let data = fs::read_to_string(&spine_file).map_err(|e| e.to_string())?;
        return serde_json::from_str(&data).map_err(|e| e.to_string());
    }
    if base.exists() {
        let _ = fs::remove_dir_all(&base);
    }
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    if is_txt {
        parse_txt_book(&src, &base)
    } else {
        unpack_epub(&app, &src, &base)
    }
}

/// 返回 EPUB 封面图片的完整路径（封面缺失/不存在则 None）
fn epub_cover_path(src: &Path) -> Option<String> {
    let base = epub_cache_dir().join(epub_cache_key(src));
    let sf = base.join("__cshow_spine.json");
    let declared = fs::read_to_string(&sf)
        .ok()
        .and_then(|s| serde_json::from_str::<EpubMeta>(&s).ok())
        .and_then(|m| m.cover);
    detect_cover(&base, declared)
        .map(|c| norm_path(&base.join(c)))
        .filter(|cp| Path::new(cp).is_file())
}

#[tauri::command]
fn epub_cover(app: tauri::AppHandle, path: String) -> Result<Option<String>, String> {
    let src = PathBuf::from(&path);
    let base = epub_cache_dir().join(epub_cache_key(&src));
    let spine_file = base.join("__cshow_spine.json");
    if !spine_file.exists() {
        if base.exists() {
            let _ = fs::remove_dir_all(&base);
        }
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        unpack_epub(&app, &src, &base)?;
    }
    Ok(epub_cover_path(&src))
}

#[tauri::command]
fn get_reader_theme(state: tauri::State<'_, db::Db>) -> String {
    let conn = state.0.lock().unwrap();
    db::get_app_state(&conn, "reader_theme")
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "light".to_string())
}

#[tauri::command]
fn set_reader_theme(state: tauri::State<'_, db::Db>, theme: String) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_app_state(&conn, "reader_theme", theme.trim())
}

#[tauri::command]
fn is_migrated(state: tauri::State<'_, db::Db>) -> bool {
    let conn = state.0.lock().unwrap();
    db::get_app_state(&conn, "localstorage_migrated")
        .ok()
        .flatten()
        .is_some()
}

#[tauri::command]
fn mark_migrated(state: tauri::State<'_, db::Db>) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_app_state(&conn, "localstorage_migrated", "1")
}

#[tauri::command]
fn get_reader_font(state: tauri::State<'_, db::Db>) -> serde_json::Value {
    let conn = state.0.lock().unwrap();
    let size = db::get_app_state(&conn, "reader_font_size")
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(16);
    let family = db::get_app_state(&conn, "reader_font_family")
        .ok()
        .flatten()
        .unwrap_or_else(|| "system".to_string());
    serde_json::json!({ "size": size, "family": family })
}

#[tauri::command]
fn set_reader_font(state: tauri::State<'_, db::Db>, size: u32, family: String) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_app_state(&conn, "reader_font_size", &size.to_string())?;
    db::set_app_state(&conn, "reader_font_family", &family)
}

/// 全局文字书页边距：上/下/左/右/双页中间距（px），仅文字书生效
#[tauri::command]
fn get_reader_margins(state: tauri::State<'_, db::Db>) -> serde_json::Value {
    let conn = state.0.lock().unwrap();
    let get = |key: &str, def: i64| -> i64 {
        db::get_app_state(&conn, key)
            .ok()
            .flatten()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(def)
    };
    serde_json::json!({
        "left": get("reader_margin_left", 10),
        "right": get("reader_margin_right", 10),
        "top": get("reader_margin_top", 28),
        "bottom": get("reader_margin_bottom", 38),
        "gap": get("reader_margin_gap", 30),
    })
}

#[tauri::command]
fn set_reader_margins(
    state: tauri::State<'_, db::Db>,
    left: i64,
    right: i64,
    top: i64,
    bottom: i64,
    gap: i64,
) -> Result<(), String> {
    let conn = state.0.lock().unwrap();
    db::set_app_state(&conn, "reader_margin_left", &left.to_string())?;
    db::set_app_state(&conn, "reader_margin_right", &right.to_string())?;
    db::set_app_state(&conn, "reader_margin_top", &top.to_string())?;
    db::set_app_state(&conn, "reader_margin_bottom", &bottom.to_string())?;
    db::set_app_state(&conn, "reader_margin_gap", &gap.to_string())
}

#[tauri::command]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

// ---- 旧数据迁移（.cshow / favorites / 应用配置 → SQLite，一次性）----

fn read_json_opt(path: &Path) -> Option<serde_json::Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn tags_to_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string())
}

/// 散装文件（书库根目录下的 epub/pdf）：数据在书库根 .cshow 里，按「文件名 / 完整路径」混合存储
fn import_loose_files(
    conn: &rusqlite::Connection,
    lib: &Path,
    v: &serde_json::Value,
) -> Result<(), String> {
    let mut paths: Vec<String> = Vec::new();
    let push_unique = |paths: &mut Vec<String>, p: String| {
        if !p.is_empty() && !paths.contains(&p) {
            paths.push(p);
        }
    };
    for key in ["file_meta", "file_hidden", "file_read_time"] {
        if let Some(map) = v.get(key).and_then(|x| x.as_object()) {
            for filename in map.keys() {
                let full = norm_path(&lib.join(filename));
                push_unique(&mut paths, full);
            }
        }
    }
    for key in ["volumes", "settings"] {
        if let Some(map) = v.get(key).and_then(|x| x.as_object()) {
            for full in map.keys() {
                push_unique(&mut paths, full.clone());
            }
        }
    }
    if let Some(lr) = v.get("last_read").and_then(|x| x.as_str()) {
        push_unique(&mut paths, lr.to_string());
    }

    for full in &paths {
        let p = Path::new(full);
        if !p.is_file() {
            continue; // 文件已移走/删除：丢弃孤儿记录
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext != "epub" && ext != "pdf" {
            continue;
        }
        let kind = if ext == "pdf" { "pdf" } else { "epub" };
        let filename = p
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let meta = meta_from_json(v.get("file_meta").and_then(|m| m.get(&filename)));
        let hidden = v
            .get("file_hidden")
            .and_then(|m| m.get(&filename))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let read_time = v
            .get("file_read_time")
            .and_then(|m| m.get(&filename))
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let is_last = v.get("last_read").and_then(|x| x.as_str()) == Some(full.as_str());
        let last_at = if is_last {
            v.get("last_read_at").and_then(|x| x.as_u64()).unwrap_or(0)
        } else {
            0
        };
        let lrv = if is_last { Some(full.as_str()) } else { None };
        db::upsert_book(
            conn, full, kind, false, hidden, &meta.title, &meta.author, meta.rating,
            &tags_to_json(&meta.tags), &meta.note, read_time, lrv, last_at,
        )?;
        if let Some(o) = v.get("volumes").and_then(|m| m.get(full)) {
            let pkind = o.get("kind").and_then(|x| x.as_str()).unwrap_or("epub");
            let page = o.get("page").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let total = o.get("total").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let mode = o.get("mode").and_then(|x| x.as_str()).unwrap_or("scroll");
            let finished = o.get("finished").and_then(|x| x.as_bool()).unwrap_or(false);
            db::upsert_position(conn, full, pkind, page, total, mode, finished, None)?;
        }
        if let Some(o) = v.get("settings").and_then(|m| m.get(full)) {
            let read_mode = o.get("read_mode").and_then(|x| x.as_str()).unwrap_or("scroll");
            let rtl = o.get("rtl").and_then(|x| x.as_bool()).unwrap_or(false);
            let dp = o.get("double_page").and_then(|x| x.as_bool()).unwrap_or(false);
            db::upsert_setting(conn, full, read_mode, rtl, dp, None, None, None)?;
        }
    }
    Ok(())
}

fn import_directory_book(
    conn: &rusqlite::Connection,
    dir: &Path,
    v: &serde_json::Value,
) -> Result<(), String> {
    // 只导入仍标记为电子书的目录（取消标记后残留的 .cshow 视为垃圾，不导入）
    if !v.get("ebook").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Ok(());
    }
    let path = norm_path(dir);
    let hidden = v.get("hidden").and_then(|x| x.as_bool()).unwrap_or(false);
    let meta = meta_from_json(Some(v));
    let read_time = v.get("read_time").and_then(|x| x.as_u64()).unwrap_or(0);
    let last = v.get("last_read").and_then(|x| x.as_str()).unwrap_or("");
    let last_at = v.get("last_read_at").and_then(|x| x.as_u64()).unwrap_or(0);
    let lrv = if !last.is_empty() && Path::new(last).exists() {
        Some(last)
    } else {
        None
    };
    db::upsert_book(
        conn, &path, "dir", true, hidden, &meta.title, &meta.author, meta.rating,
        &tags_to_json(&meta.tags), &meta.note, read_time, lrv, last_at,
    )?;
    if let Some(vols) = v.get("volumes").and_then(|x| x.as_object()) {
        for (vp, o) in vols {
            if !Path::new(vp).exists() {
                continue; // 卷已移走：丢弃
            }
            let pkind = o.get("kind").and_then(|x| x.as_str()).unwrap_or("imgdir");
            let page = o.get("page").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let total = o.get("total").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
            let mode = o.get("mode").and_then(|x| x.as_str()).unwrap_or("scroll");
            let finished = o.get("finished").and_then(|x| x.as_bool()).unwrap_or(false);
            db::upsert_position(conn, vp, pkind, page, total, mode, finished, None)?;
        }
    }
    let read_mode = v.get("read_mode").and_then(|x| x.as_str()).unwrap_or("scroll");
    let rtl = v.get("rtl").and_then(|x| x.as_bool()).unwrap_or(false);
    let dp = v.get("double_page").and_then(|x| x.as_bool()).unwrap_or(false);
    db::upsert_setting(conn, &path, read_mode, rtl, dp, None, None, None)?;
    Ok(())
}

fn scan_dir_books(
    conn: &rusqlite::Connection,
    dir: &Path,
    depth: usize,
    backup: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    if depth > 8 {
        return Ok(());
    }
    let Ok(rd) = fs::read_dir(dir) else { return Ok(()) };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by(|a, b| {
        let an = a.file_name().to_string_lossy().into_owned();
        let bn = b.file_name().to_string_lossy().into_owned();
        natural_cmp(&an, &bn)
    });
    for e in entries {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let cshow = p.join(".cshow");
        if cshow.is_file() {
            if let Some(v) = read_json_opt(&cshow) {
                backup.insert(norm_path(&cshow), v.clone());
                import_directory_book(conn, &p, &v)?;
            }
        }
        scan_dir_books(conn, &p, depth + 1, backup)?;
    }
    Ok(())
}

fn delete_cshow_recursive(dir: &Path, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            let _ = fs::remove_file(p.join(".cshow"));
            delete_cshow_recursive(&p, depth + 1);
        }
    }
}

/// 一次性迁移旧数据到 SQLite，并在备份后删除旧文件。
fn migrate_legacy_to_db(conn: &rusqlite::Connection, work_dir: &Path) -> Result<(), String> {
    if db::schema_version(conn)? >= 1 {
        return Ok(());
    }
    let favs = read_favorites_file();
    let mut backup: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    // 1. 书库（含书库根的 hidden/eye_password）
    for (i, fav) in favs.iter().enumerate() {
        let mut hidden = false;
        let mut eye_password: Option<String> = None;
        if let Some(v) = read_json_opt(&Path::new(&fav.path).join(".cshow")) {
            hidden = v.get("hidden").and_then(|x| x.as_bool()).unwrap_or(false);
            eye_password = v
                .get("eye_password")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            backup.insert(norm_path(&Path::new(&fav.path).join(".cshow")), v);
        }
        db::upsert_library(
            conn, &fav.path, &fav.alias, &fav.icon, i as i64, hidden, eye_password.as_deref(),
        )?;
    }

    // 2. 书籍与分卷
    for fav in &favs {
        let lib = Path::new(&fav.path);
        if !lib.is_dir() {
            continue;
        }
        if let Some(v) = read_json_opt(&lib.join(".cshow")) {
            import_loose_files(conn, lib, &v)?;
        }
        scan_dir_books(conn, lib, 0, &mut backup)?;
    }

    // 3. 应用配置 → app_state
    if let Ok(s) = fs::read_to_string(cwd_file()) {
        if !s.trim().is_empty() {
            db::set_app_state(conn, "cwd", s.trim())?;
        }
    }
    if let Ok(s) = fs::read_to_string(window_state_file()) {
        db::set_app_state(conn, "window", &s)?;
    }
    if let Ok(s) = fs::read_to_string(reader_theme_file()) {
        if !s.trim().is_empty() {
            db::set_app_state(conn, "reader_theme", s.trim())?;
        }
    }

    // 4. 备份 + 删除旧文件
    let favs_json = serde_json::to_value(&favs).unwrap_or_else(|_| serde_json::json!([]));
    let backup_doc = serde_json::json!({ "favorites": favs_json, "cshow": backup });
    if let Ok(s) = serde_json::to_string_pretty(&backup_doc) {
        let _ = fs::write(work_dir.join("migration_backup.json"), s);
    }
    for fav in &favs {
        let lib = Path::new(&fav.path);
        let _ = fs::remove_file(lib.join(".cshow"));
        delete_cshow_recursive(lib, 0);
    }
    let _ = fs::remove_file(favorites_file());
    let _ = fs::remove_file(cwd_file());
    let _ = fs::remove_file(window_state_file());
    let _ = fs::remove_file(reader_theme_file());

    db::set_schema_version(conn, 1)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn norm_title_key_keeps_book_title_in_guillemets() {
        // 《》是书名号：里面的书名必须保留，否则同作者的书写全撞成同一个 key
        assert_eq!(norm_title_key("《庆余年》（精校全本）作者：猫腻"), "庆余年作者：猫腻");
        assert_eq!(norm_title_key("《将夜》（精校全本）作者：猫腻"), "将夜作者：猫腻");
        assert_eq!(norm_title_key("庆余年"), "庆余年");
        assert_eq!(norm_title_key("大奉打更人（测试）"), "大奉打更人");
        assert_eq!(norm_title_key("死亡筆記（測試）"), "死亡笔记"); // 繁体转简体
    }

    #[test]
    fn detect_cover_finds_nested_cover() {
        let base = std::env::temp_dir().join(format!("cshow-detect-{}", std::process::id()));
        let img = base.join("image/cover.jpg");
        fs::create_dir_all(img.parent().unwrap()).unwrap();
        fs::write(&img, b"fake").unwrap();
        let c = detect_cover(&base, None).expect("should find nested cover");
        assert_eq!(c, "image/cover.jpg");
        assert!(base.join(&c).is_file());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn img_src_parsing() {
        let html = r#"<img src="../image/cover.jpg" alt="c"/><img src='a b.png'/><img src=noquote.png>"#;
        let srcs = all_img_srcs(html);
        assert_eq!(srcs, vec!["../image/cover.jpg", "a b.png", "noquote.png"]);

        // SVG 封面：<image xlink:href>，且值保留原始大小写
        let svg = r#"<svg xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="../Images/cover.jpg"/></svg>"#;
        assert_eq!(all_img_srcs(svg), vec!["../Images/cover.jpg"]);
        assert_eq!(first_img_src(svg).as_deref(), Some("../Images/cover.jpg"));
    }

    #[test]
    fn header_footer_detection() {
        let html = r#"<html><body>
<p class="calibre1">正常正文段落，不会被误伤。</p>
<p class="calibre1"><b class="calibre3">P a g e  | 322 </b></p>
<p class="calibre1"><b class="calibre3">Harry Potter and the Deathly Hallows – J.K. Rowling </b></p>
<pre>pre 不应被当作 p 段落</pre>
</body></html>"#;
        let paras = paragraphs_with_text(html);
        // 只有 p 段落（pre 排除）
        assert_eq!(paras.len(), 3);
        assert!(paras.iter().any(|(_, _, t)| t == "正常正文段落，不会被误伤。"));
        let page = paras.iter().find(|(_, _, t)| t.contains("Page|322")).unwrap();
        assert!(is_page_number(&page.2));
        let title = paras.iter().find(|(_, _, t)| t.contains("HarryPotter")).unwrap();
        assert!(!is_page_number(&title.2));

        // 注入标记不破坏原标签
        let injected = inject_hdr_mark(html, page.0);
        assert!(injected.contains("data-cshow-hdr=\"\""));
        assert!(injected.contains("P a g e"));
    }

    #[test]
    fn header_footer_marking_in_unpacked_epub() {
        let base = std::env::temp_dir().join(format!("cshow-hdr-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("META-INF")).unwrap();
        fs::write(
            base.join("META-INF/container.xml"),
            r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        )
        .unwrap();
        fs::write(
            base.join("content.opf"),
            r#"<package version="2.0" xmlns="http://www.idpf.org/2007/opf">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>测试书</dc:title></metadata>
<manifest>
  <item id="c1" href="chap1.xhtml" media-type="application/xhtml+xml"/>
  <item id="c2" href="chap2.xhtml" media-type="application/xhtml+xml"/>
</manifest>
<spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#,
        )
        .unwrap();
        // 每章：正文 + 页码页脚 + 书名页眉（重复 6 次 >= 5）
        for i in 1..=2 {
            let html = format!(
                r#"<html><body>
<p>这是第 {} 章的正文，很长的一段文字用于文字书判定。{}</p>
<p>P a g e | {}</p>
<p>Harry Potter and the Test – Author Name</p>
<p>Harry Potter and the Test – Author Name</p>
<p>Harry Potter and the Test – Author Name</p>
</body></html>"#,
                i,
                "字".repeat(500),
                i * 100
            );
            fs::write(base.join(format!("chap{i}.xhtml")), html).unwrap();
        }
        let meta = parse_unpacked_epub(&base).expect("解析失败");
        assert!(meta.text_book);
        let c1 = fs::read_to_string(base.join("chap1.xhtml")).unwrap();
        // 页码段落与重复书名段落都被标记
        let hdr_count = c1.matches("data-cshow-hdr=\"\"").count();
        assert!(hdr_count >= 2, "页眉页脚应被标记，实际 {hdr_count}");
        // 正常正文段落未被标记
        assert!(c1.contains("<p>这是第 1 章的正文"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn mime_charset_for_text() {
        // 无 charset 声明的 EPUB 章节依赖响应头明确 UTF-8，否则英文书的弯引号等会乱码
        assert_eq!(mime_for(Path::new("a.xhtml")), "text/html; charset=utf-8");
        assert_eq!(mime_for(Path::new("a.htm")), "text/html; charset=utf-8");
        assert_eq!(mime_for(Path::new("a.css")), "text/css; charset=utf-8");
        assert_eq!(mime_for(Path::new("a.mjs")), "text/javascript; charset=utf-8");
        assert_eq!(mime_for(Path::new("a.xml")), "text/xml; charset=utf-8");
        assert_eq!(mime_for(Path::new("a.png")), "image/png");
        assert_eq!(mime_for(Path::new("a.woff2")), "font/woff2");
    }

    #[test]
    fn db_roundtrip() {
        let base = std::env::temp_dir().join(format!("cshow-db-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let conn = db::open(&base).unwrap();

        // 目录书：最近阅读时间
        let book_dir = base.join("book");
        fs::create_dir_all(&book_dir).unwrap();
        let bd = norm_path(&book_dir);
        db::upsert_book(&conn, &bd, "dir", true, false, "", "", 0.0, "[]", "", 0, None, 67890).unwrap();
        assert_eq!(recent_read_time(&conn, &book_dir), 67890);

        // 散装文件：隐藏状态
        let loose = base.join("loose.epub");
        fs::write(&loose, b"fake").unwrap();
        let lp = norm_path(&loose);
        db::upsert_book(&conn, &lp, "epub", false, true, "", "", 0.0, "[]", "", 0, None, 0).unwrap();
        assert!(path_is_hidden(&conn, &loose));
        // 自定义封面
        db::set_book_cover(&conn, &lp, Some("/tmp/custom.png")).unwrap();
        let b = db::get_book(&conn, &lp).unwrap().unwrap();
        assert_eq!(b.cover.as_deref(), Some("/tmp/custom.png"));
        db::set_book_cover(&conn, &lp, None).unwrap();
        assert!(db::get_book(&conn, &lp).unwrap().unwrap().cover.is_none());

        // 分卷位置
        db::upsert_position(&conn, &lp, "epub", 3, 10, "flip", false, None).unwrap();
        let pos = db::get_position(&conn, &lp).unwrap().unwrap();
        assert_eq!((pos.page, pos.total, pos.mode.as_str()), (3, 10, "flip"));
        // 重置：删除分卷记录
        db::delete_position(&conn, &lp).unwrap();
        assert!(db::get_position(&conn, &lp).unwrap().is_none());

        // 阅读设置（含单书字体）
        db::upsert_setting(&conn, &bd, "flip", true, true, Some(20), Some("serif".to_string()), None).unwrap();
        let s = db::get_setting(&conn, &bd).unwrap().unwrap();
        assert!(s.rtl && s.double_page && s.read_mode == "flip");
        assert_eq!(s.font_size, Some(20));
        assert_eq!(s.font_family.as_deref(), Some("serif"));
        // None 字体不覆盖已有值（COALESCE 保留）
        db::upsert_setting(&conn, &bd, "scroll", false, false, None, None, None).unwrap();
        let s2 = db::get_setting(&conn, &bd).unwrap().unwrap();
        assert_eq!(s2.font_size, Some(20));
        assert_eq!(s2.font_family.as_deref(), Some("serif"));

        // 书库隐藏 + 密码
        db::upsert_library(&conn, &bd, "", "", 0, true, Some("hash")).unwrap();
        assert!(path_is_hidden(&conn, &book_dir));
        let lib = db::get_library(&conn, &bd).unwrap().unwrap();
        assert!(lib.hidden && lib.has_password);

        drop(conn);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn schema_migrate_v2_to_v4() {
        let base = std::env::temp_dir().join(format!("cshow-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let conn = db::open(&base).unwrap();
        // 模拟 v2 旧库：settings 无字体/主题列
        conn.execute_batch(
            "PRAGMA user_version = 2;
             DROP TABLE settings;
             CREATE TABLE settings (
               scope_path TEXT PRIMARY KEY,
               read_mode TEXT NOT NULL DEFAULT 'scroll',
               rtl INTEGER NOT NULL DEFAULT 0,
               double_page INTEGER NOT NULL DEFAULT 0
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO settings (scope_path, read_mode, rtl, double_page) VALUES ('/book', 'flip', 0, 1)",
            [],
        )
        .unwrap();
        db::migrate_schema(&conn).unwrap();
        assert_eq!(db::schema_version(&conn).unwrap(), 5);
        // v5：books 有 cover 列（自定义封面）
        let has_cover: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('books') WHERE name = 'cover'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_cover, 1);
        let s = db::get_setting(&conn, "/book").unwrap().unwrap();
        assert!(s.font_size.is_none() && s.font_family.is_none() && s.theme.is_none()); // 旧数据回退全局默认
        db::upsert_setting(
            &conn,
            "/book",
            "scroll",
            false,
            false,
            Some(18),
            Some("sans".to_string()),
            Some("sepia".to_string()),
        )
        .unwrap();
        let s2 = db::get_setting(&conn, "/book").unwrap().unwrap();
        assert_eq!(s2.font_size, Some(18));
        assert_eq!(s2.font_family.as_deref(), Some("sans"));
        assert_eq!(s2.theme.as_deref(), Some("sepia"));
        drop(conn);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn cleanup_orphan_caches_removes_old() {
        let base = std::env::temp_dir().join(format!("cshow-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let conn = db::open(&base).unwrap();
        // 当前书（文件需存在以计算 key）
        let book = base.join("book.epub");
        fs::write(&book, b"fake-epub").unwrap();
        let cur_key = epub_cache_key(&book);
        let epub_dir = base.join("epub");
        fs::create_dir_all(epub_dir.join(&cur_key)).unwrap();
        fs::create_dir_all(epub_dir.join("oldkey1")).unwrap();
        fs::create_dir_all(epub_dir.join("oldkey2")).unwrap();
        db::ensure_book(&conn, &norm_path(&book), "epub").unwrap();

        cleanup_orphan_caches(&conn, &base);
        assert!(epub_dir.join(&cur_key).is_dir(), "当前书缓存应保留");
        assert!(!epub_dir.join("oldkey1").exists(), "孤儿应删除");
        assert!(!epub_dir.join("oldkey2").exists(), "孤儿应删除");
        drop(conn);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn epub_cover_thumb_cache() {
        let base = std::env::temp_dir().join(format!("cshow-thumb-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        // 生成一张 800x1200 测试封面
        let cover = base.join("cover.jpg");
        let img = image::RgbImage::from_fn(800, 1200, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, 128])
        });
        img.save(&cover).unwrap();
        let src = base.join("book.epub");
        fs::write(&src, b"fake").unwrap();

        // 用隔离缓存目录，避免测试写真实缓存
        let cache_dir = base.join("thumbs");
        let cached = make_thumb_cache_in(&src, &cover, &cache_dir).expect("应生成封面缩略图缓存");
        let p = Path::new(&cached);
        assert!(p.is_file(), "缓存文件应存在");
        let got = image::open(p).unwrap();
        assert_eq!(got.width(), 360, "封面缩略图宽应为 360");
        assert_eq!(got.height(), 540, "封面缩略图高应按比例");
        // cached_thumb 应命中
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn text_image_detection() {
        let novel = "<html><body><p>这是一个有很多文字的长篇小说章节，包含成百上千个字符，用于判定为文字书。</p></body></html>";
        let (chars, imgs) = text_and_image_stats(novel);
        assert_eq!(imgs, 0);
        assert!(chars > 10);

        let comic = r#"<html><body><img src="1.jpg"/><img src="2.jpg"/></body></html>"#;
        let (chars2, imgs2) = text_and_image_stats(comic);
        assert!(imgs2 >= 2);
        assert!(chars2 < 500);
    }

    #[test]
    fn toc_nav_and_ncx_parsing() {
        let nav = r#"<nav><ol><li><a href="chap1.xhtml">第一章</a></li><li><a href="chap2.xhtml#s2">第二章</a></li></ol></nav>"#;
        let links = extract_toc_links(nav);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], ("第一章".to_string(), "chap1.xhtml".to_string()));
        assert_eq!(links[1], ("第二章".to_string(), "chap2.xhtml#s2".to_string()));

        let ncx = r#"<navPoint><navLabel><text>Intro</text></navLabel><content src="intro.xhtml"/></navPoint>"#;
        let links2 = extract_toc_links(ncx);
        assert_eq!(links2.len(), 1);
        assert_eq!(links2[0], ("Intro".to_string(), "intro.xhtml".to_string()));
    }

    #[test]
    fn toc_anchor_preserved() {
        let base = std::env::temp_dir().join(format!("cshow-toc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("chap.xhtml"), "<html><body><span id=\"p1\"></span><p>正文</p></body></html>").unwrap();
        fs::write(
            base.join("toc.ncx"),
            r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap><navPoint id="n1"><navLabel><text>第一章</text></navLabel>
<content src="chap.xhtml#p1"/></navPoint></navMap></ncx>"#,
        )
        .unwrap();
        let spine = vec!["chap.xhtml".to_string()];
        let items = vec![("ncx".to_string(), "toc.ncx".to_string(), String::new())];
        let (toc, _) = parse_toc(&base, "", &items, &spine, "ncx");
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].label, "第一章");
        assert_eq!(toc[0].chapter, 0);
        assert_eq!(toc[0].anchor.as_deref(), Some("p1"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn toc_anchor_remapped_across_files() {
        // 坏 NCX：navPoint 的 src 指向 chap1.xhtml#filepos...，但锚点实际在 chap2.xhtml
        // （Z-Library 转换常见）。目录应重定向到锚点真实所在章节。
        let base = std::env::temp_dir().join(format!("cshow-tocfix-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(
            base.join("chap1.xhtml"),
            "<html><body><p>第一章正文</p></body></html>",
        )
        .unwrap();
        fs::write(
            base.join("chap2.xhtml"),
            "<html><body><span id=\"filepos0000004886\"></span><p>第二章正文</p></body></html>",
        )
        .unwrap();
        fs::write(
            base.join("toc.ncx"),
            r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap><navPoint id="n1"><navLabel><text>第一章 白发</text></navLabel>
<content src="chap1.xhtml#filepos0000004886"/></navPoint></navMap></ncx>"#,
        )
        .unwrap();
        let spine = vec!["chap1.xhtml".to_string(), "chap2.xhtml".to_string()];
        let items = vec![("ncx".to_string(), "toc.ncx".to_string(), String::new())];
        let (toc, _) = parse_toc(&base, "", &items, &spine, "ncx");
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].label, "第一章 白发");
        assert_eq!(toc[0].chapter, 1, "锚点实际在 chap2，应重定向到章节 1");
        assert_eq!(toc[0].anchor.as_deref(), Some("filepos0000004886"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn toc_anchor_kept_when_in_referenced_file() {
        // 页面锚点 p20 等会在多个文件里重复出现：引用文件里有该锚点时不能重定向，
        // 否则第二册以后的章节会被错误拽回第一个文件（Z-Library 的 index_split 结构）。
        let base = std::env::temp_dir().join(format!("cshow-tocdup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(
            base.join("chap1.xhtml"),
            "<html><body><span id=\"p20\"></span><p>Book1</p></body></html>",
        )
        .unwrap();
        fs::write(
            base.join("chap2.xhtml"),
            "<html><body><span id=\"p20\"></span><p>Book2</p></body></html>",
        )
        .unwrap();
        fs::write(
            base.join("toc.ncx"),
            r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
<navMap>
<navPoint id="n1"><navLabel><text>BOOK1 CH</text></navLabel><content src="chap1.xhtml#p20"/></navPoint>
<navPoint id="n2"><navLabel><text>BOOK2 CH</text></navLabel><content src="chap2.xhtml#p20"/></navPoint>
</navMap></ncx>"#,
        )
        .unwrap();
        let spine = vec!["chap1.xhtml".to_string(), "chap2.xhtml".to_string()];
        let items = vec![("ncx".to_string(), "toc.ncx".to_string(), String::new())];
        let (toc, _) = parse_toc(&base, "", &items, &spine, "ncx");
        assert_eq!(toc.len(), 2);
        // 第二条引用 chap2#p20，锚点在 chap2 里存在 → 保持章节 1，不能被拽回 0
        assert_eq!(toc[1].chapter, 1, "重复锚点不应被重定向到首文件");
        assert_eq!(toc[1].anchor.as_deref(), Some("p20"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn toc_remap_live() {
        // 手动验收：CSHOW_TOC_BOOK=/path/to/unpacked-epub cargo test toc_remap_live -- --nocapture
        let p = std::env::var("CSHOW_TOC_BOOK").unwrap_or_default();
        if p.is_empty() {
            eprintln!("跳过：设置 CSHOW_TOC_BOOK 后可用真实书校验目录锚点重定向");
            return;
        }
        let meta = parse_unpacked_epub(Path::new(&p)).expect("解析失败");
        println!("toc len={} spine len={}", meta.toc.len(), meta.spine.len());
        let mut bad = 0usize;
        for (i, t) in meta.toc.iter().enumerate() {
            if let Some(a) = &t.anchor {
                let file = Path::new(&p).join(&meta.spine[t.chapter]);
                let html = fs::read_to_string(&file).unwrap_or_default();
                if !html.contains(&format!("id=\"{a}\"")) {
                    bad += 1;
                    println!("MISS {i}: label={} ch={} anchor={a} file={}", t.label, t.chapter, meta.spine[t.chapter]);
                }
            }
        }
        println!("锚点缺失条目数: {bad}");
        assert_eq!(bad, 0, "所有目录锚点都应能命中所在章节");
    }

    #[test]
    fn epub_toc_fix_rebuilds_broken_book() {
        // 构造坏书：NCX 三条都指向 c1，但锚点实际在 c2（Z-Library 转换常见）
        let base = std::env::temp_dir().join(format!("cshow-fix-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let epub = base.join("broken.epub");
        {
            let file = fs::File::create(&epub).unwrap();
            let mut z = zip::ZipWriter::new(file);
            let stored = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let def = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            z.start_file("mimetype", stored).unwrap();
            z.write_all(b"application/epub+zip").unwrap();
            z.start_file("META-INF/container.xml", def).unwrap();
            z.write_all(
                br#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            )
            .unwrap();
            z.start_file("content.opf", def).unwrap();
            z.write_all(
                r#"<package version="2.0" unique-identifier="BookId" xmlns="http://www.idpf.org/2007/opf">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>测试书</dc:title></metadata>
<manifest>
  <item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>
  <item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>
  <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
</manifest>
<spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#
                .as_bytes(),
            )
            .unwrap();
            z.start_file("c1.xhtml", def).unwrap();
            z.write_all(r#"<html><body><p>开篇</p></body></html>"#.as_bytes()).unwrap();
            z.start_file("c2.xhtml", def).unwrap();
            z.write_all(
                r#"<html><body><span id="a1"></span><p>第二章</p><span id="a2"></span><p>第三章</p><span id="a3"></span><p>第四章</p></body></html>"#
                    .as_bytes(),
            )
            .unwrap();
            z.start_file("toc.ncx", def).unwrap();
            z.write_all(
                r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><navMap>
<navPoint id="n1"><navLabel><text>第二章</text></navLabel><content src="c1.xhtml#a1"/></navPoint>
<navPoint id="n2"><navLabel><text>第三章</text></navLabel><content src="c1.xhtml#a2"/></navPoint>
<navPoint id="n3"><navLabel><text>第四章</text></navLabel><content src="c1.xhtml#a3"/></navPoint>
</navMap></ncx>"#
                .as_bytes(),
            )
            .unwrap();
            z.finish().unwrap();
        }

        // 分析：3 条全错位 → 需要修复
        let r = analyze_epub_toc(&epub).unwrap();
        assert!(r.needs_fix, "{}", r.message);
        assert_eq!((r.total, r.mispointed), (3, 3));

        // 修复（备份并替换原文件；备份目录用临时目录，避免碰真实工作目录）
        let r2 = rebuild_broken_epub_in(&epub, &base.join("backups")).unwrap();
        assert!(r2.chapters >= 4, "{}", r2.message);

        // 重新分析：无需修复
        let r3 = analyze_epub_toc(&epub).unwrap();
        assert!(!r3.needs_fix, "{}", r3.message);

        // 完整解析验证目录 1:1
        let ex = base.join("ex");
        fs::create_dir_all(&ex).unwrap();
        extract_zip_to(&epub, &ex).unwrap();
        let meta = parse_unpacked_epub(&ex).unwrap();
        assert_eq!(meta.toc.len(), 3);
        for (i, t) in meta.toc.iter().enumerate() {
            let file = ex.join(&meta.spine[t.chapter]);
            let html = fs::read_to_string(&file).unwrap_or_default();
            if let Some(a) = &t.anchor {
                assert!(html.contains(&format!("id=\"{a}\"")), "锚点 {a} 应命中");
            }
            assert_eq!(t.chapter, 1 + i, "目录应 1:1 对应新章节");
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn book_rel_path_traversal_rejected() {
        // 正常相对路径放行（含前导 / 与子目录）
        assert_eq!(safe_book_rel("chapter1.xhtml").as_deref(), Some("chapter1.xhtml"));
        assert_eq!(safe_book_rel("/OEBPS/Text/c1.xhtml").as_deref(), Some("OEBPS/Text/c1.xhtml"));
        // 穿越与当前目录段一律拒绝
        assert_eq!(safe_book_rel("../secret"), None);
        assert_eq!(safe_book_rel("a/../../etc/passwd"), None);
        assert_eq!(safe_book_rel("./a.html"), None);
        assert_eq!(safe_book_rel("."), None);
        // URL 解码发生在调用方（先 decode 再检查，%2e%2e → .. 即被拦截）；
        // 本函数只对已解码路径负责
        assert_eq!(safe_book_rel("a/%2e%2e/b").as_deref(), Some("a/%2e%2e/b"));
    }

    #[test]
    fn opf_in_subdir_epub_parsing() {
        // 回归：OPF 位于 OEBPS/ 子目录时，spine/NCX/封面相对 OPF 目录，
        // 必须规范化为相对解包根，否则封面/目录/章节统计全部解析失败
        let base = std::env::temp_dir().join(format!("cshow-opf-subdir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("META-INF")).unwrap();
        fs::create_dir_all(base.join("OEBPS/Text")).unwrap();
        fs::create_dir_all(base.join("OEBPS/Images")).unwrap();

        fs::write(
            base.join("META-INF/container.xml"),
            r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#,
        )
        .unwrap();
        fs::write(
            base.join("OEBPS/content.opf"),
            r#"<?xml version="1.0" encoding="utf-8"?>
<package version="2.0" unique-identifier="BookId" xmlns="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>测试书</dc:title>
    <meta name="cover" content="cover.jpg"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="cover.jpg" href="Images/cover.jpg" media-type="image/jpeg"/>
    <item id="cover.xhtml" href="Text/cover.xhtml" media-type="application/xhtml+xml"/>
    <item id="chap1" href="Text/chap1.xhtml" media-type="application/xhtml+xml"/>
    <item id="chap2" href="Text/chap2.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="cover.xhtml"/>
    <itemref idref="chap1"/>
    <itemref idref="chap2"/>
  </spine>
</package>"#,
        )
        .unwrap();
        fs::write(
            base.join("OEBPS/toc.ncx"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head><meta name="dtb:uid" content="u"/></head>
  <docTitle><text>测试书</text></docTitle>
  <navMap>
    <navPoint id="p1" playOrder="1"><navLabel><text>第一章 起点</text></navLabel><content src="Text/chap1.xhtml"/></navPoint>
    <navPoint id="p2" playOrder="2"><navLabel><text>第二章 高潮</text></navLabel><content src="Text/chap2.xhtml"/></navPoint>
  </navMap>
</ncx>"#,
        )
        .unwrap();
        fs::write(
            base.join("OEBPS/Images/cover.jpg"),
            b"fake-jpeg",
        )
        .unwrap();
        fs::write(
            base.join("OEBPS/Text/cover.xhtml"),
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="../Images/cover.jpg"/></svg></body></html>"#,
        )
        .unwrap();
        let chap = |i: usize| {
            format!(
                "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>chapter-{i} {}</p></body></html>",
                "字".repeat(600)
            )
        };
        fs::write(base.join("OEBPS/Text/chap1.xhtml"), chap(1)).unwrap();
        fs::write(base.join("OEBPS/Text/chap2.xhtml"), chap(2)).unwrap();

        let meta = parse_unpacked_epub(&base).expect("OPF 在子目录的 EPUB 应能解析");
        assert_eq!(
            meta.spine,
            vec![
                "OEBPS/Text/cover.xhtml".to_string(),
                "OEBPS/Text/chap1.xhtml".to_string(),
                "OEBPS/Text/chap2.xhtml".to_string(),
            ]
        );
        assert_eq!(meta.cover.as_deref(), Some("OEBPS/Images/cover.jpg"));
        assert_eq!(meta.toc.len(), 2);
        assert_eq!(meta.toc[0].label, "第一章 起点");
        assert_eq!(meta.toc[0].chapter, 1);
        assert_eq!(meta.toc[1].label, "第二章 高潮");
        assert_eq!(meta.toc[1].chapter, 2);
        assert!(meta.chapter_lengths[1] > 0 && meta.chapter_lengths[2] > 0);
        assert!(meta.text_book);
        // 文字书章节应被注入 reader 脚本（路径按规范化后的 spine 命中）
        let chap1 = fs::read_to_string(base.join("OEBPS/Text/chap1.xhtml")).unwrap();
        assert!(chap1.contains("__cshow_reader.js"));
        // book:// 根下按规范化路径可访问
        assert!(base.join("OEBPS/Images/cover.jpg").is_file());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ai_meta_helpers() {
        // 文件名 → 书名/作者
        let (t, a) = guess_title_author_from_filename("大王饶命 (会说话的肘子) (Z-Library).epub");
        assert_eq!(t, "大王饶命");
        assert_eq!(a, "会说话的肘子");
        let (t, a) = guess_title_author_from_filename("Elon Musk (Walter Isaacson).epub");
        assert_eq!(t, "Elon Musk");
        assert_eq!(a, "Walter Isaacson");
        let (t, _) = guess_title_author_from_filename("史上第一混乱.epub");
        assert_eq!(t, "史上第一混乱");

        // 五星换算：10 分制 → 5 星制，保留 1 位小数
        assert_eq!(to_five_star(9.8, 10.0), 4.9);
        assert_eq!(to_five_star(7.1, 10.0), 3.6);
        assert_eq!(to_five_star(4.5, 5.0), 4.5);
        assert_eq!(to_five_star(0.0, 10.0), 0.0);
    }

    #[test]
    fn ai_meta_opf_extraction() {
        // 解包目录形式的 EPUB：OPF 提供书名/作者（查询输入与兜底）
        let base = std::env::temp_dir().join(format!("cshow-aiopf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("META-INF")).unwrap();
        fs::create_dir_all(base.join("OEBPS/Text")).unwrap();
        fs::write(
            base.join("META-INF/container.xml"),
            r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        )
        .unwrap();
        fs::write(
            base.join("OEBPS/content.opf"),
            r#"<package version="2.0" xmlns="http://www.idpf.org/2007/opf">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:title>测试书</dc:title>
  <dc:creator>测试作者</dc:creator>
  <dc:language>zh</dc:language>
</metadata>
<manifest><item id="c1" href="Text/c1.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="c1"/></spine></package>"#,
        )
        .unwrap();
        fs::write(
            base.join("OEBPS/Text/c1.xhtml"),
            "<html><body><p>第一章</p></body></html>",
        )
        .unwrap();

        let (_, _, opf) = read_container_and_opf(&base).expect("应能读取 OPF");
        let basic = parse_opf_basic(&opf);
        assert_eq!(basic.title, "测试书");
        assert_eq!(basic.authors, vec!["测试作者"]);
        assert_eq!(basic.spine, vec!["Text/c1.xhtml"]);

        // .epub 压缩包形式也能读取 OPF
        let base2 = std::env::temp_dir().join(format!("cshow-aiopfzip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base2);
        fs::create_dir_all(&base2).unwrap();
        let epub = base2.join("book.epub");
        {
            let file = fs::File::create(&epub).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("META-INF/container.xml", opts).unwrap();
            zip.write_all(
                br#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
            )
            .unwrap();
            zip.start_file("content.opf", opts).unwrap();
            zip.write_all(
                r#"<package version="2.0" xmlns="http://www.idpf.org/2007/opf">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>ZIP书</dc:title><dc:creator>ZIP作者</dc:creator></metadata>
<manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest>
<spine><itemref idref="c1"/></spine></package>"#
                .as_bytes(),
            )
            .unwrap();
            zip.start_file("c1.xhtml", opts).unwrap();
            zip.write_all(b"<html><body><p>text</p></body></html>").unwrap();
            zip.finish().unwrap();
        }
        let (_, _, opf2) = read_container_and_opf(&epub).expect("zip OPF 应能读取");
        let basic2 = parse_opf_basic(&opf2);
        assert_eq!(basic2.title, "ZIP书");
        assert_eq!(basic2.authors, vec!["ZIP作者"]);
        let _ = fs::remove_dir_all(&base);
        let _ = fs::remove_dir_all(&base2);
    }

    #[test]
    fn parse_llm_meta_extracts_fields() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{
  "title": "大王饶命",
  "author": "会说话的肘子",
  "platform": "起点中文网",
  "platform_rating": 9.8,
  "platform_rating_count": "7.3万人评分",
  "fallback_rating": 7.2,
  "fallback_rating_source": "豆瓣",
  "tags": ["都市", "异能", "搞笑", "灵气复苏"],
  "core_setting": "靠怼人收集负面情绪变强",
  "synopsis": "灵气复苏时代的故事梗概。",
  "publish_start": "2017-08-18",
  "publish_end": "2018-11-26",
  "status": "已完结"
}"#,
        )
        .unwrap();
        let m = parse_llm_meta(&v, "兜底书名", "兜底作者");
        assert_eq!(m.title, "大王饶命");
        assert_eq!(m.author, "会说话的肘子");
        assert_eq!(m.rating, 4.9); // 平台优先：起点 9.8 → 4.9★
        assert!(m.rating_note.contains("起点中文网 9.8/10 → 4.9★"));
        assert!(m.rating_note.contains("豆瓣 7.2/10 → 3.6★"));
        assert_eq!(m.tags, vec!["都市", "异能", "搞笑", "灵气复苏"]);
        assert!(m.note.contains("【发表平台】起点中文网（2017-08-18，至 2018-11-26，已完结）"));
        assert!(m.note.contains("【核心设定】靠怼人收集负面情绪变强"));
        assert!(m.note.contains("【故事梗概】灵气复苏时代的故事梗概。"));
        assert!(m.message.contains("deepseek-v4-flash"));
    }

    #[test]
    fn parse_llm_meta_rating_fallback() {
        // 平台与平台评分缺失时用兜底评分
        let v: serde_json::Value = serde_json::from_str(
            r#"{"title":"Elon Musk","author":"Walter Isaacson","platform":null,
                "platform_rating":null,"fallback_rating":8.5,"fallback_rating_source":"Goodreads",
                "tags":["传记","科技"],"core_setting":null,"synopsis":"传记内容。"}"#,
        )
        .unwrap();
        let m = parse_llm_meta(&v, "", "");
        assert_eq!(m.rating, 4.3); // 8.5/10 → 4.25 → 4.3
        assert!(m.rating_note.contains("Goodreads 8.5/10 → 4.3★"));
        assert!(!m.note.contains("【发表平台】"));
        assert!(m.note.contains("【故事梗概】传记内容。"));
        assert!(m.message.contains("请人工确认后保存"));
    }

    #[test]
    fn llm_fetch_live() {
        // 手动验收：CSHOW_LIVE_BOOK=/path/to/book.epub DEEPSEEK_API_KEY=... cargo test llm_fetch_live -- --nocapture
        let p = std::env::var("CSHOW_LIVE_BOOK").unwrap_or_default();
        let key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
        if p.is_empty() || key.is_empty() {
            eprintln!("跳过：设置 CSHOW_LIVE_BOOK 与 DEEPSEEK_API_KEY 后可对本机真实书跑 AI 填入");
            return;
        }
        let r = llm_fetch_impl(Path::new(&p), "", "", &key).expect("AI 获取应成功");
        println!(
            "title={}\nauthor={}\nrating={}\ntags={:?}\nnote={}\nmessage={}",
            r.title, r.author, r.rating, r.tags, r.note, r.message
        );
        assert!(!r.title.is_empty());
    }

    #[test]
    fn height_script_csp_hash_matches() {
        // 图像书章节注入的是内联高度测量脚本，book:// CSP 依赖 sha256 精确放行。
        // 若 HEIGHT_SCRIPT 内容变化，此测试会失败，提示同步更新 BOOK_CSP 中的哈希。
        // 注意：CSP 哈希针对 <script> 内部内容计算，不含 <script>/</script> 标签本身。
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let inner = HEIGHT_SCRIPT
            .strip_prefix("<script>")
            .and_then(|s| s.strip_suffix("</script>"))
            .expect("HEIGHT_SCRIPT 应由 <script>...</script> 包裹");
        let mut hasher = Sha256::new();
        hasher.update(inner.as_bytes());
        let digest = hasher.finalize();
        let b64 = base64::engine::general_purpose::STANDARD.encode(digest);
        let token = format!("'sha256-{b64}'");
        assert!(
            BOOK_CSP.contains(&token),
            "BOOK_CSP 缺少 {}（HEIGHT_SCRIPT 已变化？请同步更新哈希）",
            token
        );
    }

    #[test]
    fn txt_book_parsing() {
        let base = std::env::temp_dir().join(format!("cshow-txt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let txt = base.join("测试书.txt");
        let content = "\
《测试书》 作者：某人

内容简介：
　　这是简介。

第一卷

第001章 初次见面
　　第一段正文。
　　第二段正文。

第002章 重逢
　　另一段正文。

第四章九天
　　第四段正文。

第二卷 东汉风云篇

第1章 归来
　　新卷第一段。
";
        fs::write(&txt, content).unwrap();

        let cache = base.join("cache");
        let meta = parse_txt_book(&txt, &cache).unwrap();
        assert!(meta.text_book);
        assert_eq!(meta.title, "测试书");
        assert_eq!(meta.spine.len(), 4, "应解析出 4 章");
        assert_eq!(meta.chapter_titles[0], "第001章 初次见面");
        // 无空格章节：解析后自动补空格（第四章九天 → 第四章 九天）
        assert!(meta.chapter_titles.iter().any(|t| t == "第四章 九天"));
        assert!(meta.toc.iter().any(|t| t.label == "第四章 九天"));
        assert!((8..=20).contains(&meta.chapter_lengths[0])); // 两段正文非空白字符数
        // 目录：两章 + 两个卷标记（第二卷在第二章前）
        assert!(meta.toc.iter().any(|t| t.label == "第一卷"));
        assert!(meta.toc.iter().any(|t| t.label == "第二卷 东汉风云篇"));
        assert!(meta.toc.iter().any(|t| t.label == "第1章 归来"));
        // 章节 XHTML：注入 reader、段落包裹、HTML 转义
        let chap = fs::read_to_string(cache.join("txt_chap_0001.xhtml")).unwrap();
        assert!(chap.contains("__cshow_reader.js"));
        assert!(chap.contains("body class=\"cshow-txt\""));
        assert_eq!(chap.matches("<p>").count(), 2, "每行一段：第一章应有两个段落");
        assert!(chap.contains("初次见面"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn txt_book_volume_chapter_inline() {
        let base = std::env::temp_dir().join(format!("cshow-txtvol-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let txt = base.join("b.txt");
        let content = "\
《测试》 作者：某人

第一卷 东林皆石

第一卷 东林皆石 第一章 钟楼街的游行
　　正文一。

第一卷 东林皆石 第二章 一百个黑衣少年的背后
　　正文二。

第二卷 上林的钟声 第三章 夜航
　　正文三。
";
        fs::write(&txt, content).unwrap();
        let meta = parse_txt_book(&txt, &base.join("cache")).unwrap();
        assert_eq!(meta.spine.len(), 3);
        assert_eq!(meta.chapter_titles[0], "第一章 钟楼街的游行");
        assert_eq!(meta.chapter_titles[2], "第三章 夜航");
        // 卷章同行：每章重复的卷前缀不重复建卷，目录卷条目唯一
        let vols: Vec<&str> = meta.toc.iter().filter(|t| t.label.contains('卷')).map(|t| t.label.as_str()).collect();
        assert_eq!(vols, vec!["第一卷 东林皆石", "第二卷 上林的钟声"]);
        let _ = fs::remove_dir_all(&base);
    }

}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if std::env::args().any(|a| a == "--version" || a == "-v") {
        println!("cshow-gui {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    env_logger::init();
    // 打开工作目录下的 SQLite 数据库（书籍信息/书库/位置/设置/应用状态统一存这里）
    let work = work_dir();
    // Android：先把暂存的元数据迁移导入（数据库 + 封面），再打开库
    #[cfg(target_os = "android")]
    import_staged_migration(&work);
    let conn = db::open(&work).expect("打开书库数据库失败");
    // 一次性迁移旧数据（.cshow / favorites / 应用配置 → DB），备份后删除旧文件
    if let Err(e) = migrate_legacy_to_db(&conn, &work) {
        log::error!("迁移旧数据到数据库失败（旧文件已尽量保留/备份）: {e}");
    }
    // 结构迁移（v1 → v2：positions 增加 progress 并回填近似百分比）
    if let Err(e) = db::migrate_schema(&conn) {
        log::error!("数据库结构迁移失败: {e}");
    }
    // 只保留当前书对应的缓存，清理旧版本/已移除书残留（缓存可重建）
    cleanup_orphan_caches(&conn, &work);
    tauri::Builder::default()
        .manage(BookState(Mutex::new(None)))
        .manage(db::Db(Mutex::new(conn)))
        .setup(|app| {
            // 启动即创建工作目录并迁移旧缓存
            let _ = work_dir();
            // 恢复上次记住的窗口尺寸；超过默认尺寸则按默认打开
            let state = app.state::<db::Db>();
            let conn = state.0.lock().unwrap();
            if let Some((w, h)) = load_window_size(&conn) {
                let w = if w > DEFAULT_WIN_W { DEFAULT_WIN_W } else { w };
                let h = if h > DEFAULT_WIN_H { DEFAULT_WIN_H } else { h };
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(w, h)));
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 记录窗口尺寸变化（转成逻辑尺寸保存），最大化/最小化不记录
            if let tauri::WindowEvent::Resized(size) = event {
                if window.is_maximized().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
                    return;
                }
                let scale = window.scale_factor().unwrap_or(1.0);
                if scale > 0.0 {
                    let state = window.app_handle().state::<db::Db>();
                    let conn = state.0.lock().unwrap();
                    save_window_size(&conn, size.width as f64 / scale, size.height as f64 / scale);
                }
            }
        })
        .register_uri_scheme_protocol("book", |ctx, request| {
            let base = ctx
                .app_handle()
                .state::<BookState>()
                .0
                .lock()
                .unwrap()
                .clone();
            let decoded = percent_encoding::percent_decode(request.uri().path().as_bytes())
                .decode_utf8_lossy()
                .to_string();
            // 路径安全：拒绝 .. / . 段，防止 book:// 逃出解包根读任意文件
            let Some(rel) = safe_book_rel(&decoded) else {
                return tauri::http::Response::builder()
                    .status(404)
                    .body(Vec::new())
                    .unwrap();
            };
            // 文字 EPUB 的 reader 脚本/样式始终从二进制内嵌版本提供（而非解包缓存里的旧文件），
            // 这样修复排版/分页问题后无需重新解包即可对所有已缓存的书生效。
            if rel == "__cshow_reader.js" {
                return tauri::http::Response::builder()
                    .header("Content-Type", "text/javascript; charset=utf-8")
                    .header("Content-Security-Policy", BOOK_CSP)
                    .header("Cache-Control", "no-store")
                    .body(READER_JS.as_bytes().to_vec())
                    .unwrap();
            }
            if rel == "__cshow_reader.css" {
                return tauri::http::Response::builder()
                    .header("Content-Type", "text/css; charset=utf-8")
                    .header("Content-Security-Policy", BOOK_CSP)
                    .header("Cache-Control", "no-store")
                    .body(READER_CSS.as_bytes().to_vec())
                    .unwrap();
            }
            if let Some(base) = base {
                let full = base.join(rel);
                // 纵深防御：确认解析后的路径仍在解包根内
                if full.strip_prefix(&base).is_ok() && full.is_file() {
                    if let Ok(bytes) = fs::read(&full) {
                        return tauri::http::Response::builder()
                            .header("Content-Type", mime_for(&full))
                            .header("Content-Security-Policy", BOOK_CSP)
                            .header("Cache-Control", "no-store")
                            .body(bytes)
                            .unwrap();
                    }
                }
            }
            tauri::http::Response::builder()
                .status(404)
                .body(Vec::new())
                .unwrap()
        })
        .invoke_handler(tauri::generate_handler![
            list_dir,
            image_dims,
            open_epub,
            epub_cover,
            ebook_volumes,
            epub_pages,
            refresh_book_cache,
            reset_book_progress,
            save_volume_position,
            set_book_cover,
            remove_book_cover,
            get_book_cover,
            save_thumb,
            save_cwd,
            save_position,
            read_position,
            toggle_ebook,
            toggle_eye,
            list_favorites,
            toggle_favorite,
            set_library_meta,
            reorder_libraries,
            get_book_meta,
            set_book_meta,
            smart_fetch_meta,
            get_deepseek_key,
            set_deepseek_key,
            check_epub_toc,
            fix_epub_toc,
            list_book_meta,
            reading_stats,
            add_reading_time,
            ebook_root,
            read_book_settings,
            write_book_settings,
            get_work_dir,
            set_work_dir,
            get_reader_theme,
            set_reader_theme,
            get_reader_font,
            set_reader_font,
            get_reader_margins,
            set_reader_margins,
            is_migrated,
            mark_migrated,
            app_version,
            initial_dir,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
