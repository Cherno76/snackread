//! SQLite 持久化层：所有书籍信息 / 书库 / 阅读位置 / 设置 / 应用状态统一存到工作目录的 library.sqlite3。

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

/// 数据库连接（单连接 + 互斥锁；所有命令都是短事务，够用）。
pub struct Db(pub Mutex<Connection>);

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS libraries (
  path         TEXT PRIMARY KEY,
  alias        TEXT NOT NULL DEFAULT '',
  icon         TEXT NOT NULL DEFAULT '',
  sort_order   INTEGER NOT NULL DEFAULT 0,
  hidden       INTEGER NOT NULL DEFAULT 0,
  eye_password TEXT
);

CREATE TABLE IF NOT EXISTS books (
  path         TEXT PRIMARY KEY,
  kind         TEXT NOT NULL DEFAULT 'dir',
  is_ebook     INTEGER NOT NULL DEFAULT 0,
  hidden       INTEGER NOT NULL DEFAULT 0,
  title        TEXT,
  author       TEXT,
  rating       REAL NOT NULL DEFAULT 0,
  tags         TEXT NOT NULL DEFAULT '[]',
  note         TEXT,
  cover        TEXT,
  read_time    INTEGER NOT NULL DEFAULT 0,
  last_read_volume TEXT,
  last_read_at INTEGER NOT NULL DEFAULT 0,
  updated_at   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS positions (
  volume_path  TEXT PRIMARY KEY,
  kind         TEXT NOT NULL,
  page         INTEGER NOT NULL DEFAULT 0,
  total        INTEGER NOT NULL DEFAULT 0,
  mode         TEXT NOT NULL DEFAULT 'scroll',
  finished     INTEGER NOT NULL DEFAULT 0,
  progress     REAL,
  updated_at   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS settings (
  scope_path   TEXT PRIMARY KEY,
  read_mode    TEXT NOT NULL DEFAULT 'scroll',
  rtl          INTEGER NOT NULL DEFAULT 0,
  double_page  INTEGER NOT NULL DEFAULT 0,
  font_size    INTEGER,
  font_family  TEXT,
  theme        TEXT
);

CREATE TABLE IF NOT EXISTS app_state (
  key   TEXT PRIMARY KEY,
  value TEXT
);

CREATE TABLE IF NOT EXISTS dir_state (
  path    TEXT PRIMARY KEY,
  current TEXT NOT NULL DEFAULT '',
  page    INTEGER NOT NULL DEFAULT 0
);
"#;

/// 打开（不存在则创建）工作目录下的数据库，并建表。
pub fn open(work_dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(work_dir).map_err(|e| e.to_string())?;
    let path = work_dir.join("library.sqlite3");
    let conn = Connection::open(&path).map_err(|e| e.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL").map_err(|e| e.to_string())?;
    conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    Ok(conn)
}

/// 当前数据库结构版本（user_version）。0 = 尚未迁移（或首次运行）。
pub fn schema_version(conn: &Connection) -> Result<u32, String> {
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(v as u32)
}

/// 标记数据库结构版本。
pub fn set_schema_version(conn: &Connection, v: u32) -> Result<(), String> {
    conn.execute_batch(&format!("PRAGMA user_version = {};", v))
        .map_err(|e| e.to_string())
}

/// 结构迁移到 v2：positions 增加 progress 列（文字书阅读百分比），并为旧数据回填近似百分比。
pub fn migrate_schema(conn: &Connection) -> Result<(), String> {
    if schema_version(conn)? >= 5 {
        return Ok(());
    }
    // v1 → v2：positions 增加 progress 列
    if schema_version(conn)? < 2 {
        let has_progress: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('positions') WHERE name = 'progress'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if has_progress == 0 {
            conn.execute_batch("ALTER TABLE positions ADD COLUMN progress REAL")
                .map_err(|e| e.to_string())?;
        }
        // 回填：用旧 page/total 近似成百分比（图片书也会回填，但图片书不使用 progress，无影响）
        conn.execute_batch(
            "UPDATE positions SET progress = CAST(page AS REAL) / NULLIF(total, 0) WHERE progress IS NULL AND total > 0",
        )
        .map_err(|e| e.to_string())?;
        set_schema_version(conn, 2)?;
    }
    // v2 → v3：settings 增加字体列（NULL = 未设置，回退全局默认）
    let has_font_size: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('settings') WHERE name = 'font_size'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if has_font_size == 0 {
        conn.execute_batch("ALTER TABLE settings ADD COLUMN font_size INTEGER")
            .map_err(|e| e.to_string())?;
        conn.execute_batch("ALTER TABLE settings ADD COLUMN font_family TEXT")
            .map_err(|e| e.to_string())?;
    }
    // v3 → v4：settings 增加主题列（NULL = 未设置，回退全局默认主题）
    let has_theme: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('settings') WHERE name = 'theme'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if has_theme == 0 {
        conn.execute_batch("ALTER TABLE settings ADD COLUMN theme TEXT")
            .map_err(|e| e.to_string())?;
    }
    // v4 → v5：books 增加 cover 列（自定义封面图片路径，NULL = 未设置）
    let has_cover: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('books') WHERE name = 'cover'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if has_cover == 0 {
        conn.execute_batch("ALTER TABLE books ADD COLUMN cover TEXT")
            .map_err(|e| e.to_string())?;
    }
    set_schema_version(conn, 5)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---- libraries ----

#[derive(Debug, Clone, serde::Serialize)]
pub struct LibraryRow {
    pub path: String,
    pub alias: String,
    pub icon: String,
    pub sort_order: i64,
    pub hidden: bool,
    pub has_password: bool,
}

pub fn upsert_library(
    conn: &Connection,
    path: &str,
    alias: &str,
    icon: &str,
    sort_order: i64,
    hidden: bool,
    eye_password: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO libraries (path, alias, icon, sort_order, hidden, eye_password)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path) DO UPDATE SET
           alias=excluded.alias, icon=excluded.icon, sort_order=excluded.sort_order,
           hidden=excluded.hidden, eye_password=excluded.eye_password",
        params![path, alias, icon, sort_order, hidden as i64, eye_password],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_libraries(conn: &Connection) -> Result<Vec<LibraryRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT path, alias, icon, sort_order, hidden, eye_password FROM libraries ORDER BY sort_order ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(LibraryRow {
                path: r.get(0)?,
                alias: r.get(1)?,
                icon: r.get(2)?,
                sort_order: r.get(3)?,
                hidden: r.get::<_, i64>(4)? != 0,
                has_password: {
                    let p: Option<String> = r.get(5)?;
                    p.map(|s| !s.is_empty()).unwrap_or(false)
                },
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn reorder_libraries(conn: &Connection, paths: &[String]) -> Result<(), String> {
    for (i, p) in paths.iter().enumerate() {
        conn.execute(
            "UPDATE libraries SET sort_order = ?1 WHERE path = ?2",
            params![i as i64, p],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- books ----

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BookRow {
    pub path: String,
    pub kind: String,
    pub is_ebook: bool,
    pub hidden: bool,
    pub title: String,
    pub author: String,
    pub rating: f64,
    pub tags: String, // JSON 数组字符串
    pub note: String,
    pub cover: Option<String>,
    pub read_time: u64,
    pub last_read_volume: String,
    pub last_read_at: u64,
}

fn book_from_row(r: &rusqlite::Row) -> rusqlite::Result<BookRow> {
    Ok(BookRow {
        path: r.get(0)?,
        kind: r.get(1)?,
        is_ebook: r.get::<_, i64>(2)? != 0,
        hidden: r.get::<_, i64>(3)? != 0,
        title: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        author: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        rating: r.get(6)?,
        tags: r.get(7)?,
        note: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
        cover: r.get(9)?,
        read_time: r.get(10)?,
        last_read_volume: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
        last_read_at: r.get(12)?,
    })
}

const BOOK_COLS: &str = "path, kind, is_ebook, hidden, title, author, rating, tags, note, cover, read_time, last_read_volume, last_read_at";

/// 插入或整行覆盖一本书（迁移用）。
#[allow(clippy::too_many_arguments)]
pub fn upsert_book(
    conn: &Connection,
    path: &str,
    kind: &str,
    is_ebook: bool,
    hidden: bool,
    title: &str,
    author: &str,
    rating: f64,
    tags: &str,
    note: &str,
    read_time: u64,
    last_read_volume: Option<&str>,
    last_read_at: u64,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO books (path, kind, is_ebook, hidden, title, author, rating, tags, note, read_time, last_read_volume, last_read_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(path) DO UPDATE SET
           kind=excluded.kind, is_ebook=excluded.is_ebook, hidden=excluded.hidden,
           title=excluded.title, author=excluded.author, rating=excluded.rating,
           tags=excluded.tags, note=excluded.note, read_time=excluded.read_time,
           last_read_volume=excluded.last_read_volume, last_read_at=excluded.last_read_at,
           updated_at=excluded.updated_at",
        params![path, kind, is_ebook as i64, hidden as i64, title, author, rating, tags, note, read_time as i64, last_read_volume, last_read_at as i64, now_secs() as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_book(conn: &Connection, path: &str) -> Result<Option<BookRow>, String> {
    let q = format!("SELECT {} FROM books WHERE path = ?1", BOOK_COLS);
    conn.query_row(&q, params![path], book_from_row)
        .optional()
        .map_err(|e| e.to_string())
}

/// 书库中所有 EPUB 文件路径（散装与分卷），用于计算当前缓存 key
pub fn list_epub_paths(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT path FROM books WHERE kind = 'epub'")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

// ---- positions ----

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PositionRow {
    pub kind: String,
    pub page: u32,
    pub total: u32,
    pub mode: String,
    pub finished: bool,
    pub progress: Option<f64>,
}

pub fn upsert_position(
    conn: &Connection,
    volume_path: &str,
    kind: &str,
    page: u32,
    total: u32,
    mode: &str,
    finished: bool,
    progress: Option<f64>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO positions (volume_path, kind, page, total, mode, finished, progress, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(volume_path) DO UPDATE SET
           kind=excluded.kind, page=excluded.page, total=excluded.total,
           mode=excluded.mode, finished=excluded.finished, progress=excluded.progress, updated_at=excluded.updated_at",
        params![volume_path, kind, page as i64, total as i64, mode, finished as i64, progress, now_secs() as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_position(conn: &Connection, volume_path: &str) -> Result<Option<PositionRow>, String> {
    conn.query_row(
        "SELECT kind, page, total, mode, finished, progress FROM positions WHERE volume_path = ?1",
        params![volume_path],
        |r| {
            Ok(PositionRow {
                kind: r.get(0)?,
                page: r.get::<_, i64>(1)? as u32,
                total: r.get::<_, i64>(2)? as u32,
                mode: r.get(3)?,
                finished: r.get::<_, i64>(4)? != 0,
                progress: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// 删除单个分卷的阅读记录（重置进度用）
pub fn delete_position(conn: &Connection, volume_path: &str) -> Result<(), String> {
    conn.execute("DELETE FROM positions WHERE volume_path = ?1", params![volume_path])
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---- settings ----

#[derive(Debug, Clone, Default)]
pub struct SettingRow {
    pub read_mode: String,
    pub rtl: bool,
    pub double_page: bool,
    pub font_size: Option<u32>,
    pub font_family: Option<String>,
    pub theme: Option<String>,
}

pub fn upsert_setting(
    conn: &Connection,
    scope_path: &str,
    read_mode: &str,
    rtl: bool,
    double_page: bool,
    font_size: Option<u32>,
    font_family: Option<String>,
    theme: Option<String>,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (scope_path, read_mode, rtl, double_page, font_size, font_family, theme)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(scope_path) DO UPDATE SET
           read_mode=excluded.read_mode, rtl=excluded.rtl, double_page=excluded.double_page,
           font_size=COALESCE(excluded.font_size, settings.font_size),
           font_family=COALESCE(excluded.font_family, settings.font_family),
           theme=COALESCE(excluded.theme, settings.theme)",
        params![
            scope_path,
            read_mode,
            rtl as i64,
            double_page as i64,
            font_size.map(|v| v as i64),
            font_family,
            theme,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_setting(conn: &Connection, scope_path: &str) -> Result<Option<SettingRow>, String> {
    conn.query_row(
        "SELECT read_mode, rtl, double_page, font_size, font_family, theme FROM settings WHERE scope_path = ?1",
        params![scope_path],
        |r| {
            Ok(SettingRow {
                read_mode: r.get(0)?,
                rtl: r.get::<_, i64>(1)? != 0,
                double_page: r.get::<_, i64>(2)? != 0,
                font_size: r.get::<_, Option<i64>>(3)?.map(|v| v as u32),
                font_family: r.get(4)?,
                theme: r.get(5)?,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

// ---- app_state ----

pub fn set_app_state(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO app_state (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_app_state(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row("SELECT value FROM app_state WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

// ---- 增补 CRUD ----

fn library_from_row(r: &rusqlite::Row) -> rusqlite::Result<LibraryRow> {
    Ok(LibraryRow {
        path: r.get(0)?,
        alias: r.get(1)?,
        icon: r.get(2)?,
        sort_order: r.get(3)?,
        hidden: r.get::<_, i64>(4)? != 0,
        has_password: {
            let p: Option<String> = r.get(5)?;
            p.map(|s| !s.is_empty()).unwrap_or(false)
        },
    })
}

pub fn get_library(conn: &Connection, path: &str) -> Result<Option<LibraryRow>, String> {
    conn.query_row(
        "SELECT path, alias, icon, sort_order, hidden, eye_password FROM libraries WHERE path = ?1",
        params![path],
        library_from_row,
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn set_library_meta(conn: &Connection, path: &str, alias: &str, icon: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE libraries SET alias = ?1, icon = ?2 WHERE path = ?3",
        params![alias, icon, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_library_hidden(conn: &Connection, path: &str, hidden: bool) -> Result<(), String> {
    conn.execute(
        "UPDATE libraries SET hidden = ?1 WHERE path = ?2",
        params![hidden as i64, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_library_eye_password(conn: &Connection, path: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT eye_password FROM libraries WHERE path = ?1",
        params![path],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

pub fn set_library_eye_password(
    conn: &Connection,
    path: &str,
    eye_password: Option<&str>,
) -> Result<(), String> {
    conn.execute(
        "UPDATE libraries SET eye_password = ?1 WHERE path = ?2",
        params![eye_password, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// 级联删除一个书库：书籍 / 分卷位置 / 阅读设置 / 目录状态一并删除。
/// 返回所有受影响路径（去重），供调用方清理缩略图与 EPUB 解包缓存。
pub fn delete_library_cascade(conn: &Connection, lib_path: &str) -> Result<Vec<String>, String> {
    let like = like_escape(&format!("{}/", lib_path)) + "%";
    let mut paths: Vec<String> = Vec::new();
    for sql in [
        "SELECT path FROM books WHERE path LIKE ?1 ESCAPE '\\'",
        "SELECT volume_path FROM positions WHERE volume_path LIKE ?1 ESCAPE '\\'",
        "SELECT scope_path FROM settings WHERE scope_path LIKE ?1 ESCAPE '\\'",
        "SELECT path FROM dir_state WHERE path LIKE ?1 ESCAPE '\\'",
    ] {
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([&like], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for row in rows {
            paths.push(row.map_err(|e| e.to_string())?);
        }
    }
    paths.sort();
    paths.dedup();

    conn.execute("DELETE FROM books WHERE path LIKE ?1 ESCAPE '\\'", [&like]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM positions WHERE volume_path LIKE ?1 ESCAPE '\\'", [&like]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM settings WHERE scope_path LIKE ?1 ESCAPE '\\'", [&like]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM dir_state WHERE path LIKE ?1 ESCAPE '\\'", [&like]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM dir_state WHERE path = ?1", [lib_path]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM libraries WHERE path = ?1", [lib_path]).map_err(|e| e.to_string())?;
    Ok(paths)
}

pub fn set_book_hidden(conn: &Connection, path: &str, hidden: bool) -> Result<(), String> {
    conn.execute(
        "UPDATE books SET hidden = ?1 WHERE path = ?2",
        params![hidden as i64, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_book_is_ebook(conn: &Connection, path: &str, is_ebook: bool) -> Result<(), String> {
    conn.execute(
        "UPDATE books SET is_ebook = ?1 WHERE path = ?2",
        params![is_ebook as i64, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn ensure_book(conn: &Connection, path: &str, kind: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO books (path, kind) VALUES (?1, ?2) ON CONFLICT(path) DO NOTHING",
        params![path, kind],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_book_meta(
    conn: &Connection,
    path: &str,
    title: &str,
    author: &str,
    rating: f64,
    tags_json: &str,
    note: &str,
) -> Result<(), String> {
    conn.execute(
        "UPDATE books SET title = ?1, author = ?2, rating = ?3, tags = ?4, note = ?5, updated_at = ?6 WHERE path = ?7",
        params![title, author, rating, tags_json, note, now_secs() as i64, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 设置/清除一本书的自定义封面（cover = 封面文件路径，None 表示清除）
pub fn set_book_cover(conn: &Connection, path: &str, cover: Option<&str>) -> Result<(), String> {
    conn.execute(
        "UPDATE books SET cover = ?1, updated_at = ?2 WHERE path = ?3",
        params![cover, now_secs() as i64, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn add_read_time(conn: &Connection, path: &str, seconds: u64) -> Result<u64, String> {
    conn.execute(
        "UPDATE books SET read_time = read_time + ?1, updated_at = ?2 WHERE path = ?3",
        params![seconds as i64, now_secs() as i64, path],
    )
    .map_err(|e| e.to_string())?;
    let total: i64 = conn
        .query_row("SELECT read_time FROM books WHERE path = ?1", params![path], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    Ok(total as u64)
}

pub fn set_last_read(
    conn: &Connection,
    path: &str,
    volume: Option<&str>,
    at: u64,
) -> Result<(), String> {
    conn.execute(
        "UPDATE books SET last_read_volume = ?1, last_read_at = ?2, updated_at = ?3 WHERE path = ?4",
        params![volume, at as i64, now_secs() as i64, path],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn set_dir_state(conn: &Connection, path: &str, current: &str, page: u32) -> Result<(), String> {
    conn.execute(
        "INSERT INTO dir_state (path, current, page) VALUES (?1, ?2, ?3)
         ON CONFLICT(path) DO UPDATE SET current=excluded.current, page=excluded.page",
        params![path, current, page as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_dir_state(conn: &Connection, path: &str) -> Result<Option<(String, u32)>, String> {
    conn.query_row(
        "SELECT current, page FROM dir_state WHERE path = ?1",
        params![path],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32)),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// 按规范化路径批量读元数据（书库网格一次取齐），返回与前端约定的 JSON。
pub fn list_book_meta(conn: &Connection, paths: &[String]) -> Result<Vec<serde_json::Value>, String> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let row = get_book(conn, p)?.unwrap_or_default();
        let tags: Vec<String> = serde_json::from_str(&row.tags).unwrap_or_default();
        out.push(serde_json::json!({
            "path": p,
            "title": row.title,
            "author": row.author,
            "rating": row.rating,
            "tags": tags,
            "note": row.note,
            "read_time": row.read_time,
        }));
    }
    Ok(out)
}
