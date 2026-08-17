#!/usr/bin/env python3
"""把桌面版 cshow 数据库里的元数据（书名/作者/评分/标签/备注/封面）
按文件名匹配同步到手机版数据库。

用法（手机通过 adb 连接）：
    python3 scripts/sync_meta_to_phone.py

可配置项见下方常量。
"""
import os
import re
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time

ADB = "/Users/cherno/Library/Android/sdk/platform-tools/adb"
DESKTOP_DB = os.path.expanduser("~/Documents/cshow-work/library.sqlite3")
DESKTOP_COVERS = os.path.expanduser("~/Documents/cshow-work/covers")
LOCAL_LIB = "/Users/cherno/Documents/电子书"          # 与手机上目标书库同源的文件列表
PHONE_ROOT = "/storage/emulated/0/E-Books/电子书"     # 手机上的书库根
PHONE_PKG = "com.cherno.cshow_gui"
PHONE_DB_REL = "files/cshow-work/library.sqlite3"
PHONE_COVERS_REL = "files/cshow-work/covers"
PHONE_DATA_ROOT = "/data/user/0/com.cherno.cshow_gui"


def adb(*args):
    subprocess.run([ADB, *args], check=True)


def adb_out(*args):
    r = subprocess.run([ADB, *args], capture_output=True)
    return r.stdout


def main():
    if not os.path.exists(DESKTOP_DB):
        sys.exit(f"桌面库不存在: {DESKTOP_DB}")

    # 1) 桌面库：按文件名收集元数据
    src = sqlite3.connect(DESKTOP_DB)
    rows = src.execute(
        "SELECT path, title, author, rating, tags, note, cover FROM books"
    ).fetchall()
    by_name = {}
    for path, title, author, rating, tags, note, cover in rows:
        by_name.setdefault(os.path.basename(path),
                           (title, author, rating, tags, note, cover))
    src.close()

    # 2) 手机书库文件列表（与本地同源，直接列本地目录）
    files = sorted(f for f in os.listdir(LOCAL_LIB) if not f.startswith("."))
    matched = [f for f in files if f in by_name]
    print(f"匹配: {len(matched)}/{len(files)}")
    for f in files:
        if f not in by_name:
            print(f"  ⚠ 未找到元数据: {f}")

    # 3) 停应用，拉手机库（主文件 + WAL）
    adb("shell", f"am force-stop {PHONE_PKG}")
    time.sleep(1)
    tmp = tempfile.mkdtemp(prefix="cshow-mig-")
    main_p = os.path.join(tmp, "library.sqlite3")
    wal_p = main_p + "-wal"
    with open(main_p, "wb") as f:
        f.write(adb_out("exec-out", f"run-as {PHONE_PKG} cat {PHONE_DB_REL}"))
    with open(wal_p, "wb") as f:
        f.write(adb_out("exec-out", f"run-as {PHONE_PKG} cat {PHONE_DB_REL}-wal"))

    # 4) 写元数据（按路径 upsert，只动元数据字段）
    conn = sqlite3.connect(main_p)
    now = int(time.time())
    covers_to_copy = []
    for f in matched:
        title, author, rating, tags, note, cover = by_name[f]
        ext = os.path.splitext(f)[1].lower().lstrip(".")
        kind = "txt" if ext == "txt" else ("epub" if ext == "epub" else ("pdf" if ext == "pdf" else "dir"))
        phone_path = PHONE_ROOT + "/" + f
        cover_phone = None
        if cover and os.path.exists(cover):
            base = os.path.basename(cover)
            covers_to_copy.append(cover)
            cover_phone = f"{PHONE_DATA_ROOT}/{PHONE_COVERS_REL}/{base}"
        conn.execute(
            """INSERT INTO books
                 (path, kind, is_ebook, hidden, title, author, rating, tags, note, cover,
                  read_time, last_read_volume, last_read_at, updated_at)
               VALUES (?,?,1,0,?,?,?,?,?,?,0,NULL,0,?)
               ON CONFLICT(path) DO UPDATE SET
                 kind=excluded.kind, is_ebook=excluded.is_ebook,
                 title=excluded.title, author=excluded.author, rating=excluded.rating,
                 tags=excluded.tags, note=excluded.note, cover=excluded.cover,
                 updated_at=excluded.updated_at""",
            (phone_path, kind, title or "", author or "", float(rating or 0),
             tags or "[]", note or "", cover_phone, now),
        )
    conn.commit()
    final_p = os.path.join(tmp, "final.sqlite3")
    conn.execute("VACUUM INTO ?", (final_p,))
    conn.close()
    print(f"已写入 {len(matched)} 本元数据")

    # 5) 推送数据库 + 封面文件
    adb("shell", "rm -rf /sdcard/Download/mig-tmp && mkdir -p /sdcard/Download/mig-tmp/covers")
    adb("push", final_p, "/sdcard/Download/mig-tmp/library.sqlite3")
    for c in covers_to_copy:
        adb("push", c, "/sdcard/Download/mig-tmp/covers/")
    adb("shell", f"run-as {PHONE_PKG} sh -c 'rm -f {PHONE_DB_REL}-wal {PHONE_DB_REL}-shm; "
                 f"mkdir -p {PHONE_COVERS_REL}; "
                 f"cp /sdcard/Download/mig-tmp/library.sqlite3 {PHONE_DB_REL}; "
                 f"cp /sdcard/Download/mig-tmp/covers/* {PHONE_COVERS_REL}/'")
    adb("shell", "rm -rf /sdcard/Download/mig-tmp")
    print(f"封面已复制: {len(covers_to_copy)} 个")

    # 6) 重启应用并验证
    adb("shell", f"am start -n {PHONE_PKG}/.MainActivity")
    time.sleep(6)
    with open(os.path.join(tmp, "check.sqlite3"), "wb") as f:
        f.write(adb_out("exec-out", f"run-as {PHONE_PKG} cat {PHONE_DB_REL}"))
    ck = sqlite3.connect(os.path.join(tmp, "check.sqlite3"))
    n = ck.execute("SELECT COUNT(*) FROM books WHERE title != '' AND path LIKE ?",
                   (PHONE_ROOT + "%",)).fetchone()[0]
    ncover = ck.execute("SELECT COUNT(*) FROM books WHERE cover IS NOT NULL AND path LIKE ?",
                        (PHONE_ROOT + "%",)).fetchone()[0]
    ck.close()
    print(f"验证: 电子书库有书名 {n} 本，有封面 {ncover} 本")
    shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
