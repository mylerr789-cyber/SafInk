use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Manga { pub id: i64, pub name: String, pub chapters: i64 }
#[derive(Serialize, Clone, Debug)]
pub struct Chapter { pub id: i64, pub manga_id: i64, pub name: String, pub path: String, pub quick: bool, pub pages: i64, pub cleaned: i64 }
#[derive(Serialize, Clone, Debug)]
pub struct Page {
    pub id: i64,
    pub chapter_id: i64,
    pub idx: i64,
    pub path: String,
    pub name: String,
    pub width: i64,
    pub height: i64,
    pub sha256: String,
    /// new | mask_fetched | mask_edited | cleaned | cleaned_stale (temizlendi, sonra maske değişti) | error
    pub status: String,
    pub error: String,
    pub mask_conf: f64,
    pub detections: i64,
    pub rev: i64,
}

const PAGE_COLS: &str = "id,chapter_id,idx,path,name,width,height,sha256,status,error,mask_conf,detections,rev";
fn page_from(r: &rusqlite::Row) -> rusqlite::Result<Page> {
    Ok(Page {
        id: r.get(0)?, chapter_id: r.get(1)?, idx: r.get(2)?, path: r.get(3)?, name: r.get(4)?, width: r.get(5)?, height: r.get(6)?,
        sha256: r.get(7)?, status: r.get(8)?, error: r.get(9)?, mask_conf: r.get(10)?, detections: r.get(11)?, rev: r.get(12)?,
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS manga(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, created INTEGER DEFAULT (strftime('%s','now')));
            CREATE TABLE IF NOT EXISTS chapter(id INTEGER PRIMARY KEY, manga_id INTEGER NOT NULL REFERENCES manga(id) ON DELETE CASCADE,
                name TEXT NOT NULL, path TEXT NOT NULL DEFAULT '', quick INTEGER NOT NULL DEFAULT 0, created INTEGER DEFAULT (strftime('%s','now')));
            CREATE TABLE IF NOT EXISTS page(id INTEGER PRIMARY KEY, chapter_id INTEGER NOT NULL REFERENCES chapter(id) ON DELETE CASCADE,
                idx INTEGER NOT NULL, path TEXT NOT NULL, name TEXT NOT NULL, width INTEGER NOT NULL DEFAULT 0, height INTEGER NOT NULL DEFAULT 0,
                sha256 TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'new', error TEXT NOT NULL DEFAULT '',
                mask_conf REAL NOT NULL DEFAULT 0, detections INTEGER NOT NULL DEFAULT 0, rev INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS page_chapter ON page(chapter_id, idx);",
        )?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn ensure_manga(&self, name: &str) -> Result<i64> {
        let c = self.conn.lock();
        c.execute("INSERT OR IGNORE INTO manga(name) VALUES(?1)", params![name])?;
        Ok(c.query_row("SELECT id FROM manga WHERE name=?1", params![name], |r| r.get(0))?)
    }
    pub fn list_manga(&self) -> Result<Vec<Manga>> {
        let c = self.conn.lock();
        let mut s = c.prepare("SELECT m.id,m.name,(SELECT COUNT(*) FROM chapter WHERE manga_id=m.id) FROM manga m WHERE m.name<>'__quick__' ORDER BY m.name")?;
        let v = s.query_map([], |r| Ok(Manga { id: r.get(0)?, name: r.get(1)?, chapters: r.get(2)? }))?.collect::<Result<_, _>>()?;
        Ok(v)
    }
    pub fn delete_manga(&self, id: i64) -> Result<()> {
        self.conn.lock().execute("DELETE FROM manga WHERE id=?1", params![id])?;
        Ok(())
    }
    pub fn rename_manga(&self, id: i64, name: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE manga SET name=?2 WHERE id=?1", params![id, name])?;
        Ok(())
    }

    pub fn create_chapter(&self, manga_id: i64, name: &str, path: &str, quick: bool) -> Result<i64> {
        let c = self.conn.lock();
        c.execute("INSERT INTO chapter(manga_id,name,path,quick) VALUES(?1,?2,?3,?4)", params![manga_id, name, path, quick])?;
        Ok(c.last_insert_rowid())
    }
    pub fn list_chapters(&self, manga_id: i64) -> Result<Vec<Chapter>> {
        let c = self.conn.lock();
        let mut s = c.prepare("SELECT c.id,c.manga_id,c.name,c.path,c.quick,(SELECT COUNT(*) FROM page WHERE chapter_id=c.id),(SELECT COUNT(*) FROM page WHERE chapter_id=c.id AND status='cleaned') FROM chapter c WHERE manga_id=?1 ORDER BY c.name COLLATE NOCASE")?;
        let v = s
            .query_map(params![manga_id], |r| {
                Ok(Chapter { id: r.get(0)?, manga_id: r.get(1)?, name: r.get(2)?, path: r.get(3)?, quick: r.get::<_, i64>(4)? != 0, pages: r.get(5)?, cleaned: r.get(6)? })
            })?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
    pub fn get_chapter(&self, id: i64) -> Result<Option<Chapter>> {
        let c = self.conn.lock();
        Ok(c
            .query_row("SELECT c.id,c.manga_id,c.name,c.path,c.quick,0,0 FROM chapter c WHERE id=?1", params![id], |r| {
                Ok(Chapter { id: r.get(0)?, manga_id: r.get(1)?, name: r.get(2)?, path: r.get(3)?, quick: r.get::<_, i64>(4)? != 0, pages: 0, cleaned: 0 })
            })
            .optional()?)
    }
    pub fn delete_chapter(&self, id: i64) -> Result<()> {
        self.conn.lock().execute("DELETE FROM chapter WHERE id=?1", params![id])?;
        Ok(())
    }
    pub fn rename_chapter(&self, id: i64, name: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE chapter SET name=?2 WHERE id=?1", params![id, name])?;
        Ok(())
    }

    pub fn add_page(&self, chapter_id: i64, idx: i64, path: &str, name: &str, w: i64, h: i64, sha: &str) -> Result<i64> {
        let c = self.conn.lock();
        c.execute(
            "INSERT INTO page(chapter_id,idx,path,name,width,height,sha256) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![chapter_id, idx, path, name, w, h, sha],
        )?;
        Ok(c.last_insert_rowid())
    }
    pub fn next_idx(&self, chapter_id: i64) -> Result<i64> {
        Ok(self.conn.lock().query_row("SELECT COALESCE(MAX(idx)+1,0) FROM page WHERE chapter_id=?1", params![chapter_id], |r| r.get(0))?)
    }
    pub fn pages(&self, chapter_id: i64) -> Result<Vec<Page>> {
        let c = self.conn.lock();
        let mut s = c.prepare(&format!("SELECT {PAGE_COLS} FROM page WHERE chapter_id=?1 ORDER BY idx"))?;
        let v = s.query_map(params![chapter_id], page_from)?.collect::<Result<_, _>>()?;
        Ok(v)
    }
    pub fn page(&self, id: i64) -> Result<Option<Page>> {
        let c = self.conn.lock();
        Ok(c.query_row(&format!("SELECT {PAGE_COLS} FROM page WHERE id=?1"), params![id], page_from).optional()?)
    }
    pub fn set_status(&self, id: i64, status: &str, error: &str) -> Result<()> {
        self.conn.lock().execute("UPDATE page SET status=?2,error=?3,rev=rev+1 WHERE id=?1", params![id, status, error])?;
        Ok(())
    }
    pub fn set_mask_info(&self, id: i64, conf: f64, detections: i64) -> Result<()> {
        self.conn.lock().execute(
            "UPDATE page SET mask_conf=?2,detections=?3,status='mask_fetched',error='',rev=rev+1 WHERE id=?1",
            params![id, conf, detections],
        )?;
        Ok(())
    }
    /// Durumu bozmadan kullanılan maske güven eşiğini günceller.
    pub fn set_conf(&self, id: i64, conf: f64, detections: i64) -> Result<()> {
        self.conn.lock().execute("UPDATE page SET mask_conf=?2,detections=?3,rev=rev+1 WHERE id=?1", params![id, conf, detections])?;
        Ok(())
    }
    /// Bölümün sayfalarını verilen sırayla yeniden numaralar (`idx` 0..n). Listede olmayan sayfalar mevcut sırayla sona eklenir.
    pub fn reorder_pages(&self, chapter_id: i64, order: &[i64]) -> Result<()> {
        let c = self.conn.lock();
        let tx = c.unchecked_transaction()?;
        let mut ids: Vec<i64> = Vec::with_capacity(order.len());
        {
            let mut s = tx.prepare("SELECT id FROM page WHERE chapter_id=?1 ORDER BY idx")?;
            let cur: Vec<i64> = s.query_map(params![chapter_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
            for id in order {
                if cur.contains(id) && !ids.contains(id) {
                    ids.push(*id);
                }
            }
            for id in cur {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        for (i, id) in ids.iter().enumerate() {
            tx.execute("UPDATE page SET idx=?3 WHERE id=?1 AND chapter_id=?2", params![id, chapter_id, i as i64])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn delete_page(&self, id: i64) -> Result<()> {
        self.conn.lock().execute("DELETE FROM page WHERE id=?1", params![id])?;
        Ok(())
    }
    /// "Quick Clean" geçici bölümlerini siler, silinen sayfa id'lerini döner.
    pub fn delete_quick_chapters(&self) -> Result<Vec<i64>> {
        let c = self.conn.lock();
        let mut s = c.prepare("SELECT id FROM page WHERE chapter_id IN (SELECT id FROM chapter WHERE quick=1)")?;
        let ids: Vec<i64> = s.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        drop(s);
        c.execute("DELETE FROM chapter WHERE quick=1", [])?;
        Ok(ids)
    }
}
