#!/usr/bin/env python3
"""从 Mac 书库数据库重新生成外部元数据预置库（默认 ~/Documents/cshow-work/presets.json）。

行为：
- 保留现有条目（人工整理的丰富信息），按当前数据库补齐空字段；
- 数据库里没有对应条目的书追加新条目（name = 文件名，用于按文件名匹配）；
- 已有封面压缩为最大 240px 的 JPEG 缩略图，以 base64 内嵌（cover_b64）。

用法：python3 scripts/update_presets.py [library.sqlite3 路径]
"""

import base64
import io
import json
import os
import sqlite3
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PRESETS = (
    sys.argv[1]
    if len(sys.argv) > 1
    else os.path.expanduser("~/Documents/cshow-work/presets.json")
)
COVERS_DIR = os.path.expanduser("~/Documents/cshow-work/covers")
DB = (
    sys.argv[2]
    if len(sys.argv) > 2
    else os.path.expanduser("~/Documents/cshow-work/library.sqlite3")
)

COVER_MAX_W = 240
COVER_QUALITY = 82


def read_cover_thumb(path: str):
    """把封面压成 JPEG 缩略图，返回 (字节, base64)；失败返回 None。"""
    if not path or not os.path.isfile(path):
        return None
    try:
        from PIL import Image

        im = Image.open(path)
        im = im.convert("RGB")
        if im.width > COVER_MAX_W:
            h = round(im.height * COVER_MAX_W / im.width)
            im = im.resize((COVER_MAX_W, h), Image.LANCZOS)
        buf = io.BytesIO()
        im.save(buf, "JPEG", quality=COVER_QUALITY)
        data = buf.getvalue()
        return data, base64.b64encode(data).decode("ascii")
    except Exception:
        return None


def safe_cover_name(title: str):
    import re

    s = re.sub(r'[\/\\:*?"<>|]', "_", title.strip())
    if len(s) > 80:
        s = s[:80]
    return s or "未命名"


def main():
    if not os.path.isfile(DB):
        print(f"数据库不存在: {DB}")
        sys.exit(1)

    if os.path.isfile(PRESETS):
        with open(PRESETS, encoding="utf-8") as f:
            entries = json.load(f)
    else:
        entries = []
    by_name = {e.get("name", ""): e for e in entries}

    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    rows = conn.execute(
        "SELECT path, title, author, rating, tags, note, cover FROM books "
        "WHERE title IS NOT NULL AND trim(title) != '' ORDER BY path"
    ).fetchall()
    conn.close()

    added = 0
    covers = 0
    os.makedirs(COVERS_DIR, exist_ok=True)
    used_cover_names = set()
    for path, title, author, rating, tags, note, cover in rows:
        name = os.path.basename(path.rstrip("/"))
        if not name:
            continue
        entry = by_name.get(name)
        if entry is None:
            entry = {
                "title": title,
                "author": author or "",
                "rating": rating or 0.0,
                "tags": json.loads(tags) if tags else [],
                "note": note or "",
                "name": name,
            }
            entries.append(entry)
            by_name[name] = entry
            added += 1
        # 补齐空字段（不覆盖已有的人工整理内容）
        if not entry.get("title"):
            entry["title"] = title
        if not entry.get("author"):
            entry["author"] = author or ""
        if not entry.get("rating"):
            entry["rating"] = rating or 0.0
        if not entry.get("tags"):
            entry["tags"] = json.loads(tags) if tags else []
        if not entry.get("note"):
            entry["note"] = note or ""
        if not entry.get("cover_b64") and cover:
            thumb = read_cover_thumb(cover)
            if thumb:
                data, b64 = thumb
                entry["cover_b64"] = b64
                covers += 1
                # 封面文件名用书名，便于人工识别（重名自动加序号）
                base = safe_cover_name(entry.get("title") or title)
                fname = f"{base}.jpg"
                n = 2
                while fname in used_cover_names:
                    fname = f"{base} ({n}).jpg"
                    n += 1
                used_cover_names.add(fname)
                with open(os.path.join(COVERS_DIR, fname), "wb") as f:
                    f.write(data)

    with open(PRESETS, "w", encoding="utf-8") as f:
        json.dump(entries, f, ensure_ascii=False, indent=1)
        f.write("\n")

    size = os.path.getsize(PRESETS)
    print(f"预置条目: {len(entries)}（新增 {added}），内嵌封面: {covers}，文件大小: {size / 1024:.0f} KB")


if __name__ == "__main__":
    main()
