//! Memory: captures of on-screen text in one local SQLite file with full-text search (memory-v1.1 tickets 02, 04, 11).
//! Words follow GLOSSARY.md: capture, mask, forget.

use crate::privacy::{self, MASK_VERSION};
use crate::settings::{MemoryState, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The same window seen again within this many seconds updates its row instead of adding one.
pub const MERGE_SECS: i64 = 5 * 60;
/// Above this size the oldest captures are forgotten first.
pub const MAX_BYTES: i64 = 1 << 30;

/// One reading of a window, before masking.
pub struct Capture {
    pub app: String,
    pub title: String,
    /// Host + path for browsers, never a query string.
    pub site: Option<String>,
    pub text: String,
}

/// Search snippets mark each match between these (control characters, never in captured text).
pub const MARK_START: char = '\u{1}';
pub const MARK_END: char = '\u{2}';

/// Narrows a search.
#[derive(Default)]
pub struct Filter {
    /// Only captures seen since (Unix seconds).
    pub since: Option<i64>,
    /// Only this app (exe name).
    pub app: Option<String>,
}

/// A search result.
#[derive(Debug)]
pub struct Hit {
    pub id: i64,
    pub app: String,
    pub title: String,
    pub site: Option<String>,
    pub last_seen: i64,
    /// Text around the match, each match between `MARK_START` and `MARK_END`.
    pub snippet: String,
}

pub struct Store(Connection);

/// `%LOCALAPPDATA%\Gopu\memory.db`: on this PC only, never roams.
pub fn db_path() -> Option<PathBuf> {
    crate::platform::log_dir().map(|d| d.join("memory.db"))
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

impl Store {
    pub fn open(path: &Path) -> Result<Store, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Store::init(Connection::open(path).map_err(|e| e.to_string())?)
    }

    /// Read-only, for searching while the app writes (WAL lets both run).
    pub fn open_read_only(path: &Path) -> Result<Store, String> {
        let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
        Ok(Store(c))
    }

    #[cfg(test)]
    fn in_memory() -> Store {
        Store::init(Connection::open_in_memory().unwrap()).unwrap()
    }

    #[cfg(test)]
    pub(crate) fn in_memory_for_tests() -> Store {
        Store::in_memory()
    }

    /// The connection, for the other memory modules (people pages).
    pub(crate) fn db(&self) -> &Connection {
        &self.0
    }

    fn init(c: Connection) -> Result<Store, String> {
        c.execute_batch(
            "pragma journal_mode = wal;
             pragma secure_delete = on; -- forgotten text is overwritten in the file, not just unlinked
             create table if not exists captures(
               id integer primary key, app text not null, title text not null, site text, text text not null,
               hash integer not null, first_seen integer not null, last_seen integer not null, mask_v integer not null);
             create index if not exists captures_seen on captures(last_seen);
             create index if not exists captures_window on captures(app, title, last_seen);
             create virtual table if not exists captures_fts using fts5(
               app, title, site, text, content = 'captures', content_rowid = 'id', tokenize = 'unicode61 remove_diacritics 2');
             insert into captures_fts(captures_fts, rank) values('secure-delete', 1);
             create trigger if not exists captures_ai after insert on captures begin
               insert into captures_fts(rowid, app, title, site, text) values (new.id, new.app, new.title, new.site, new.text); end;
             create trigger if not exists captures_ad after delete on captures begin
               insert into captures_fts(captures_fts, rowid, app, title, site, text) values ('delete', old.id, old.app, old.title, old.site, old.text); end;
             create table if not exists key_results(
               id integer primary key, at integer not null, app text not null, site text, title text not null,
               field_case text not null, user_text text not null, output text not null, recipient text,
               sent_text text, sent_at integer, similarity real, source text not null default 'watching',
               verdict text not null default 'unseen');
             create index if not exists key_results_at on key_results(at);
             create index if not exists key_results_recipient on key_results(recipient collate nocase);
             create table if not exists people(
               id integer primary key, name text not null unique collate nocase, aliases text not null default '[]',
               summary text not null default '', language text not null default '', facts text not null default '[]',
               edited integer not null default 0, created_at integer not null, updated_at integer not null);
             create table if not exists people_never(name text primary key collate nocase);
             create table if not exists style_rules(
               id integer primary key, rule text not null, scope text not null default '', source text not null,
               confirmed integer not null default 0, on_ integer not null default 1, edited integer not null default 0,
               created_at integer not null, updated_at integer not null);
             create table if not exists learn_runs(at integer not null);
             create table if not exists ask_turns(
               id integer primary key, chat integer not null, at integer not null, question text not null, label text,
               answer text not null, sources text not null default '[]');
             create index if not exists ask_turns_chat on ask_turns(chat, at);
             create trigger if not exists captures_au after update of app, title, site, text on captures begin
               insert into captures_fts(captures_fts, rowid, app, title, site, text) values ('delete', old.id, old.app, old.title, old.site, old.text);
               insert into captures_fts(rowid, app, title, site, text) values (new.id, new.app, new.title, new.site, new.text); end;",
        )
        .map_err(|e| e.to_string())?;
        // columns added after a table first shipped
        let has = |table: &str, col: &str| -> bool {
            c.prepare(&format!("select 1 from pragma_table_info('{table}') where name = '{col}'")).and_then(|mut st| st.exists([])).unwrap_or(false)
        };
        if !has("key_results", "learned") {
            c.execute_batch("alter table key_results add column learned integer not null default 0").map_err(|e| e.to_string())?;
        }
        let s = Store(c);
        s.remask()?;
        Ok(s)
    }

    /// Masks and stores one reading. The same window (app + title + site) seen within `MERGE_SECS` updates its row:
    /// unchanged text only moves `last_seen`, changed text replaces the row's text.
    pub fn record(&self, c: &Capture, now: i64) -> Result<(), String> {
        let (title, text) = (privacy::mask(&clean_title(&c.title)), privacy::mask(&c.text));
        if title.trim().is_empty() && text.trim().is_empty() {
            return Ok(());
        }
        let mut h = DefaultHasher::new();
        (&c.app, &title, &c.site, &text).hash(&mut h);
        let hash = h.finish() as i64;
        let db = &self.0;
        let recent: Option<(i64, i64)> = db
            .query_row(
                "select id, hash from captures where app = ?1 and title = ?2 and site is ?3 and last_seen >= ?4
                 order by last_seen desc limit 1",
                params![c.app, title, c.site, now - MERGE_SECS],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let r = match recent {
            Some((id, h)) if h == hash => db.execute("update captures set last_seen = ?2 where id = ?1", params![id, now]),
            Some((id, _)) => db.execute(
                "update captures set text = ?2, hash = ?3, last_seen = ?4, mask_v = ?5 where id = ?1",
                params![id, text, hash, now, MASK_VERSION],
            ),
            None => db.execute(
                "insert into captures(app, title, site, text, hash, first_seen, last_seen, mask_v) values (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7)",
                params![c.app, title, c.site, text, hash, now, MASK_VERSION],
            ),
        };
        r.map(|_| ()).map_err(|e| e.to_string())
    }

    /// Full-text search, best match first, narrowed by `f`. Captures holding every word come first; when none do (a
    /// whole sentence typed in), any word counts. Words are matched literally, so any input is safe. A blank query
    /// lists the most recent captures.
    pub fn search(&self, query: &str, f: &Filter, limit: usize) -> Result<Vec<Hit>, String> {
        let q: Vec<String> = query.split_whitespace().map(|w| format!("\"{}\"", w.replace('"', "\"\""))).collect();
        if q.is_empty() {
            return self.hits(
                "select id, app, title, site, last_seen, substr(text, 1, 200) from captures
                 where (?1 is null or last_seen >= ?1) and (?2 is null or app = ?2) order by last_seen desc limit ?3",
                params![f.since, f.app, limit as i64],
            );
        }
        let all = self.matching(&q.join(" "), f, limit)?;
        if !all.is_empty() || q.len() == 1 {
            return Ok(all);
        }
        self.matching(&q.join(" OR "), f, limit)
    }

    fn matching(&self, fts_query: &str, f: &Filter, limit: usize) -> Result<Vec<Hit>, String> {
        self.hits(
            &format!(
                "select c.id, c.app, c.title, c.site, c.last_seen, snippet(captures_fts, 3, '{MARK_START}', '{MARK_END}', '…', 24)
                 from captures_fts join captures c on c.id = captures_fts.rowid
                 where captures_fts match ?1 and (?2 is null or c.last_seen >= ?2) and (?3 is null or c.app = ?3)
                 order by bm25(captures_fts) limit ?4"
            ),
            params![fts_query, f.since, f.app, limit as i64],
        )
    }

    fn hits(&self, sql: &str, p: impl rusqlite::Params) -> Result<Vec<Hit>, String> {
        let mut st = self.0.prepare(sql).map_err(|e| e.to_string())?;
        let rows = st
            .query_map(p, |r| {
                Ok(Hit { id: r.get(0)?, app: r.get(1)?, title: r.get(2)?, site: r.get(3)?, last_seen: r.get(4)?, snippet: r.get(5)? })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// Windows seen since `since`, newest first, without the window the user is in now (`app` + `title`):
    /// (last_seen, app, title, site, text).
    #[allow(clippy::type_complexity)]
    pub fn activity(&self, since: i64, app: &str, title: &str, limit: usize) -> Result<Vec<(i64, String, String, Option<String>, String)>, String> {
        let mut st = self
            .0
            .prepare(
                "select last_seen, app, title, site, text from captures where last_seen >= ?1 and not (app = ?2 and title = ?3)
                 order by last_seen desc limit ?4",
            )
            .map_err(|e| e.to_string())?;
        let rows = st
            .query_map(params![since, app, privacy::mask(&clean_title(title)), limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// Every capture seen in [since, until), oldest first, for a period's timeline:
    /// (id, app, title, site, first_seen, last_seen, text length).
    #[allow(clippy::type_complexity)]
    pub fn windows(&self, since: i64, until: i64) -> Result<Vec<(i64, String, String, Option<String>, i64, i64, i64)>, String> {
        let mut st = self
            .0
            .prepare(
                "select id, app, title, site, first_seen, last_seen, length(text) from captures
                 where last_seen >= ?1 and first_seen < ?2 order by first_seen",
            )
            .map_err(|e| e.to_string())?;
        let rows = st
            .query_map(params![since, until], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// Stores one Gopu-key result (masked) and returns its id; send detection fills in the rest later.
    pub fn add_key_result(&self, k: &KeyResult, now: i64) -> Result<i64, String> {
        let m = privacy::mask;
        self.0
            .execute(
                "insert into key_results(at, app, site, title, field_case, user_text, output, recipient) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![now, k.app, k.site, m(&k.title), k.case, m(&k.user_text), m(&k.output), k.recipient],
            )
            .map_err(|e| e.to_string())?;
        Ok(self.0.last_insert_rowid())
    }

    /// How a key result's send watch ended: `source` "commit" / "probable" / "none", and the sent text if seen.
    pub fn set_sent(&self, id: i64, source: &str, sent: Option<&str>, now: i64) -> Result<(), String> {
        let output: String =
            self.0.query_row("select output from key_results where id = ?1", params![id], |r| r.get(0)).map_err(|e| e.to_string())?;
        let sent = sent.map(privacy::mask);
        let graded = sent.as_deref().map(|s| grade(&output, s));
        let (sim, verdict) = (graded.map(|g| g.0), graded.map_or("unseen", |g| g.1));
        self.0
            .execute(
                "update key_results set sent_text = ?2, sent_at = ?3, similarity = ?4, source = ?5, verdict = ?6 where id = ?1",
                params![id, sent, sent.as_ref().map(|_| now), sim, source, verdict],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Key results as (verdict, count), for checks and later the kept rate.
    pub fn verdicts(&self) -> Result<Vec<(String, i64)>, String> {
        let mut st = self.0.prepare("select verdict, count(*) from key_results group by verdict").map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// One capture in full: (app, title, site, text, first_seen, last_seen).
    #[allow(clippy::type_complexity)]
    pub fn get(&self, id: i64) -> Result<Option<(String, String, Option<String>, String, i64, i64)>, String> {
        self.0
            .query_row("select app, title, site, text, first_seen, last_seen from captures where id = ?1", params![id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })
            .optional()
            .map_err(|e| e.to_string())
    }

    /// The apps memory holds captures from, for the search filter.
    pub fn apps(&self) -> Result<Vec<String>, String> {
        let mut st = self.0.prepare("select distinct app from captures order by app collate nocase").map_err(|e| e.to_string())?;
        let rows = st.query_map([], |r| r.get(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    }

    /// (file size in bytes, when the oldest capture was first seen).
    pub fn stats(&self) -> Result<(i64, Option<i64>), String> {
        self.0
            .query_row(
                "select (select page_count * page_size from pragma_page_count(), pragma_page_size()), (select min(first_seen) from captures)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| e.to_string())
    }

    /// Forgets one capture.
    pub fn forget(&self, id: i64) -> Result<usize, String> {
        self.0.execute("delete from captures where id = ?1", params![id]).map_err(|e| e.to_string())
    }

    /// Forgets every capture seen since `since` (the last 15 minutes, hour, today).
    pub fn forget_since(&self, since: i64) -> Result<usize, String> {
        self.0.execute("delete from key_results where at >= ?1", params![since]).map_err(|e| e.to_string())?;
        self.0.execute("delete from ask_turns where at >= ?1", params![since]).map_err(|e| e.to_string())?;
        self.0.execute("delete from captures where last_seen >= ?1", params![since]).map_err(|e| e.to_string())
    }

    /// Forgets everything, and compacts the file so nothing is left in it.
    pub fn forget_all(&self) -> Result<usize, String> {
        self.0.execute("delete from key_results", []).map_err(|e| e.to_string())?;
        self.0.execute("delete from people", []).map_err(|e| e.to_string())?;
        self.0.execute("delete from style_rules", []).map_err(|e| e.to_string())?;
        self.0.execute("delete from learn_runs", []).map_err(|e| e.to_string())?;
        self.0.execute("delete from ask_turns", []).map_err(|e| e.to_string())?;
        let n = self.0.execute("delete from captures", []).map_err(|e| e.to_string())?;
        self.0.execute_batch("insert into captures_fts(captures_fts) values('rebuild'); vacuum").map_err(|e| e.to_string())?;
        Ok(n)
    }

    /// Captures an ignore-list entry covers: how many (`delete` false), or forgets them (`delete` true).
    pub fn ignored(&self, entry: &str, delete: bool) -> Result<usize, String> {
        let ids: Vec<i64> = {
            let mut st = self.0.prepare("select id, app, site from captures").map_err(|e| e.to_string())?;
            let rows = st
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))
                .map_err(|e| e.to_string())?;
            rows.filter_map(Result::ok).filter(|(_, app, site)| privacy::ignored(entry, app, site.as_deref())).map(|(id, ..)| id).collect()
        };
        if delete {
            for id in &ids {
                self.forget(*id)?;
            }
            // key results from there go too
            let keys: Vec<i64> = {
                let mut st = self.0.prepare("select id, app, site from key_results").map_err(|e| e.to_string())?;
                let rows = st
                    .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?)))
                    .map_err(|e| e.to_string())?;
                rows.filter_map(Result::ok).filter(|(_, app, site)| privacy::ignored(entry, app, site.as_deref())).map(|(id, ..)| id).collect()
            };
            for id in keys {
                self.0.execute("delete from key_results where id = ?1", params![id]).map_err(|e| e.to_string())?;
            }
        }
        Ok(ids.len())
    }

    /// "N captures from M apps, since <first_seen>", for a quick look.
    pub fn summary(&self) -> Result<String, String> {
        self.0
            .query_row("select count(*), count(distinct app), min(first_seen) from captures", [], |r| {
                let oldest = r.get::<_, Option<i64>>(2)?.map_or(String::new(), |t| format!(", oldest {} min ago", (now() - t) / 60));
                Ok(format!("{} captures from {} apps{oldest}", r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| e.to_string())
    }

    /// Forgets captures older than `keep_days` (0 = never), then the oldest ones while the file is over `MAX_BYTES`, and
    /// compacts the file after a big delete. Returns how many were forgotten.
    pub fn purge(&self, now: i64, keep_days: u32) -> Result<usize, String> {
        let db = &self.0;
        let e = |e: rusqlite::Error| e.to_string();
        let keep = if keep_days == 0 { i64::MAX / 2 } else { keep_days as i64 * 24 * 3600 };
        db.execute("delete from key_results where at < ?1", params![now - keep]).map_err(e)?;
        // Ask Gopu's chats: 30 days, or less when memory keeps less
        let chats = keep.min(crate::ask::KEEP_CHATS_SECS);
        db.execute("delete from ask_turns where at < ?1", params![now - chats]).map_err(e)?;
        let mut gone = db.execute("delete from captures where last_seen < ?1", params![now - keep]).map_err(e)?;
        let size = || -> Result<i64, String> {
            db.query_row("select page_count * page_size from pragma_page_count(), pragma_page_size()", [], |r| r.get(0)).map_err(e)
        };
        while size()? > MAX_BYTES {
            let n = db
                .execute("delete from captures where id in (select id from captures order by last_seen limit 1000)", [])
                .map_err(e)?;
            if n == 0 {
                break;
            }
            gone += n;
            db.execute_batch("vacuum").map_err(e)?; // the file only shrinks once compacted
        }
        if gone > 500 {
            db.execute_batch("insert into captures_fts(captures_fts) values('optimize'); vacuum").map_err(e)?;
        }
        Ok(gone)
    }

    /// Masks rows stored under older mask rules again.
    fn remask(&self) -> Result<(), String> {
        let e = |e: rusqlite::Error| e.to_string();
        let old: Vec<(i64, String, String)> = {
            let mut st = self.0.prepare("select id, title, text from captures where mask_v < ?1").map_err(e)?;
            let rows = st.query_map(params![MASK_VERSION], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(e)?;
            rows.collect::<Result<_, _>>().map_err(e)?
        };
        for (id, title, text) in old {
            self.0
                .execute(
                    "update captures set title = ?2, text = ?3, mask_v = ?4 where id = ?1",
                    params![id, privacy::mask(&title), privacy::mask(&text), MASK_VERSION],
                )
                .map_err(e)?;
        }
        Ok(())
    }
}

/// A window title without what animates or counts in front of it: a spinner or status dot (Claude Code's ◐ ◑ ✳,
/// VS Code's ●) and an unread count ("(3) WhatsApp"). Otherwise each frame of the spinner was a new capture.
pub fn clean_title(title: &str) -> String {
    let keep = |c: char| c.is_alphanumeric() || "@#([{<\"'¿¡".contains(c);
    let mut t = title.trim_start_matches(|c: char| !keep(c));
    while let Some(rest) = t.strip_prefix('(').and_then(|r| r.split_once(')')).filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit() || c == '+')) {
        t = rest.1.trim_start_matches(|c: char| !keep(c));
    }
    t.trim_end().to_string()
}

/// The start of today, local time (Unix seconds).
pub fn today_start(now: i64) -> i64 {
    let off = crate::platform::utc_offset_secs();
    let local = now + off;
    local - local.rem_euclid(86_400) - off
}

/// A time as the user's clock shows it: "3:40 pm".
pub fn clock_text(t: i64) -> String {
    let secs = (t + crate::platform::utc_offset_secs()).rem_euclid(86_400);
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "am" } else { "pm" })
}

/// Where a tap happened, as style rules and the kept rate group it: the site's host in a browser, else the app.
pub fn surface(app: &str, site: Option<&str>) -> String {
    site.and_then(|s| s.split('/').next()).filter(|h| !h.is_empty()).unwrap_or(app).to_string()
}

/// May memory touch this window: memory on and not paused, not on the never-capture list or the user's ignore list?
/// The same answer for capture, the clipboard and recall in the Gopu key.
pub fn allowed_here(s: &Settings, now: i64, app: &str, title: &str, site: Option<&str>) -> bool {
    s.memory_state(now) == MemoryState::On
        && !privacy::never_capture(app, title, site)
        && !s.ignore.iter().any(|i| privacy::ignored(i, app, site))
}

/// Starts background capture on its own thread: the window in front is read when it changes (1 s later) and every
/// 10 s while the user is active, never while idle. Settings are read before each read, so turning memory off,
/// pausing it or ignoring an app applies at once.
pub fn start_capture() {
    std::thread::spawn(move || {
        let store = match db_path().ok_or("no local app data folder".to_string()).and_then(|p| Store::open(&p)) {
            Ok(s) => s,
            Err(e) => return println!("memory is off: {e}"),
        };
        watch_clipboard();
        crate::style::start_learner();
        let mut purged = 0;
        crate::platform::capture_loop(|| {
            let (now, settings) = (now(), Settings::load());
            if now - purged > 3600 {
                purged = now;
                if let Err(e) = store.purge(now, settings.keep_days()) {
                    println!("memory: purge failed: {e}");
                }
            }
            if settings.memory_state(now) != MemoryState::On {
                return;
            }
            let Some(w) = crate::platform::foreground_text() else { return };
            let site = w.url.as_deref().and_then(privacy::site_from_url);
            if !allowed_here(&settings, now, &w.app, &w.title, site.as_deref()) {
                return; // no trace at all
            }
            if let Err(e) = store.record(&Capture { app: w.app, title: w.title, site, text: w.text }, now) {
                println!("memory: couldn't store a capture: {e}");
            }
        });
    });
}

// ---------------------------------------------------------------- key results and send detection (ticket 07)

/// One Gopu-key run, as memory keeps it (glossary: key result). Masked when stored.
pub struct KeyResult {
    pub app: String,
    pub site: Option<String>,
    pub title: String,
    /// "empty", "draft", "instruction", "selection", "atcaret".
    pub case: String,
    /// What the user had typed or selected.
    pub user_text: String,
    /// What Gopu wrote.
    pub output: String,
    /// Who the message was for, when known (people pages fill this in).
    pub recipient: Option<String>,
}

/// Text made comparable: lowercase, straight quotes, no zero-width characters, single spaces, no closing full stop.
fn norm(s: &str) -> String {
    let s: String = s
        .chars()
        .filter(|c| !matches!(c, '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}'))
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            c => c,
        })
        .collect();
    let s = s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    s.trim_end_matches(['.', '!', ' ']).to_string() // a dropped full stop isn't an edit
}

/// How alike Gopu's text and the sent text are, 0..1 (normalized Levenshtein).
pub fn similarity(a: &str, b: &str) -> f64 {
    strsim::normalized_levenshtein(&norm(a), &norm(b))
}

/// The verdict for a seen send: kept (≥ 0.97), edited (≥ 0.50), replaced.
pub fn verdict(similarity: f64) -> &'static str {
    match similarity {
        s if s >= 0.97 => "kept",
        s if s >= 0.50 => "edited",
        _ => "replaced",
    }
}

/// (similarity, verdict) for what Gopu wrote against what was sent. Keeping Gopu's text whole and adding to it (or
/// sending only part of it) is an edit however much was added, not a replacement.
pub fn grade(output: &str, sent: &str) -> (f64, &'static str) {
    let sim = similarity(output, sent);
    let (a, b) = (norm(output), norm(sent));
    let kept_part = !a.is_empty() && !b.is_empty() && (b.contains(&a) || a.contains(&b));
    (sim, if kept_part && sim < 0.97 { "edited" } else { verdict(sim) })
}

/// Does the window now show `sent` (a chat thread after sending)? Chat apps render markdown away, so the markers go
/// on both sides; then the start and end of the message must both be there, as `landed` checks a write-back.
pub fn shows_in(window: &str, sent: &str) -> bool {
    let plain = |s: &str| -> String {
        let lines = s.lines().map(|l| l.trim_start().trim_start_matches(['#', '-', '>']).replace(['*', '_', '~', '`', '|'], ""));
        norm(&lines.collect::<Vec<_>>().join(" "))
    };
    crate::landed(&plain(window), &plain(sent), None)
}

/// Sites whose compose window closes when the message is sent: a vanished composer counts as a probable send there.
const CLOSES_ON_SEND: &[&str] = &["mail.google.com"];

/// Watches what happens to Gopu's text after a write-back, on its own thread, and records it on key result `id`.
/// The field must still have focus when this is called (right after the write-back).
pub fn watch_send(id: i64, site: Option<String>) {
    std::thread::spawn(move || {
        let end = crate::platform::watch_send(std::time::Duration::from_secs(15 * 60));
        let gmail = site.as_deref().is_some_and(|s| CLOSES_ON_SEND.iter().any(|h| s.split('/').next() == Some(h)));
        let (source, sent) = match end {
            crate::platform::SendEnd::Sent(last) => ("commit", Some(last)),
            crate::platform::SendEnd::Gone(last) if gmail => ("probable", Some(last)),
            _ => ("none", None),
        };
        // an email's signature or quoted thread below the message isn't what the user wrote
        let sent = sent.map(|s| crate::split_kept(&s).0.to_string()).filter(|s| !s.trim().is_empty());
        let saved = db_path().ok_or("no folder".to_string()).and_then(|p| Store::open(&p)).and_then(|st| st.set_sent(id, source, sent.as_deref(), now()));
        match saved {
            Ok(()) => println!("send check: {}", if source == "none" { "not seen (unseen)" } else { source }),
            Err(e) => println!("memory: couldn't record the send: {e}"),
        }
    });
}

// ---------------------------------------------------------------- recall (memory-v1.1 ticket 06)

/// How far back recall looks.
pub const RECALL_SECS: i64 = 30 * 60;
const MAX_ACTIVITY: usize = 12;
const EXCERPT_CHARS: usize = 200;
const MAX_CLIPS: usize = 3;
const CLIP_CHARS: usize = 500;
/// Recall's share of a Gopu-key request: ~1.5K tokens, counted as chars / 3 (safe for Gujarati and Hindi).
pub const RECALL_CHARS: usize = 4500;

/// A copy the user made: when, and its masked text.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub at: i64,
    pub text: String,
}

/// The last copies, newest first. In RAM only: never written to disk, gone when Gopu quits.
static CLIPS: Mutex<VecDeque<Clip>> = Mutex::new(VecDeque::new());
const RING: usize = 10;

/// Keeps a copy (masked). A text already in the ring isn't added again (Gopu restoring the clipboard after a
/// write-back looks like a copy).
pub fn remember_clip(text: &str, now: i64) {
    let text = privacy::mask(text.trim());
    if text.is_empty() {
        return;
    }
    let mut ring = CLIPS.lock().unwrap();
    if ring.iter().any(|c| c.text == text) {
        return;
    }
    ring.push_front(Clip { at: now, text });
    ring.truncate(RING);
}

/// Forgets copies made since `since` (the Forget buttons cover the clipboard too).
pub fn forget_clips_since(since: i64) {
    CLIPS.lock().unwrap().retain(|c| c.at < since);
}

/// Watches the clipboard on its own thread. A copy is kept only where memory is allowed, and never when the app that
/// copied it marked it private (password managers do).
fn watch_clipboard() {
    std::thread::spawn(|| {
        let mut seq = crate::platform::clipboard_seq();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let now_seq = crate::platform::clipboard_seq();
            if now_seq == seq {
                continue;
            }
            seq = now_seq;
            let (now, settings) = (now(), Settings::load());
            if settings.memory_state(now) != MemoryState::On {
                continue;
            }
            let Some(w) = crate::platform::foreground_text() else { continue };
            let site = w.url.as_deref().and_then(privacy::site_from_url);
            if !allowed_here(&settings, now, &w.app, &w.title, site.as_deref()) {
                continue;
            }
            if let Some(text) = crate::platform::clipboard_text() {
                remember_clip(&text, now);
            }
        }
    });
}

/// The recall blocks for one Gopu-key tap in this window, within `RECALL_CHARS`, or None when there's nothing.
/// `surface`: the app or site (`surface`), whose style rules go first. `recipient`: who the message is for, when
/// known; their people page comes next.
pub fn recall(now: i64, app: &str, title: &str, surface: &str, recipient: Option<&str>) -> Option<String> {
    let store = Store::open_read_only(&db_path()?).ok()?;
    let rules = crate::style::block(&store, surface);
    let person = recipient.and_then(|r| crate::people::find(&store, r)).map(|p| crate::people::block(&p)).unwrap_or_default();
    let person = format!("{rules}{person}"); // rules rarely change, so they lead (the provider's prompt cache)
    let activity = store.activity(now - RECALL_SECS, app, title, MAX_ACTIVITY).unwrap_or_default();
    let clips: Vec<Clip> = CLIPS.lock().unwrap().iter().filter(|c| c.at >= now - RECALL_SECS).take(MAX_CLIPS).cloned().collect();
    let text = format!("{person}{}", recall_text(&activity, &clips, now, person.chars().count()));
    (!text.is_empty()).then_some(text)
}

/// `<recent_activity>` (oldest first, ending with the window used just before this one) and `<clipboard>` (newest
/// first). The clipboard is small and kept whole; the oldest activity lines go first when over budget.
/// `used`: chars already taken by the people page, so activity shrinks to fit.
pub fn recall_text(activity: &[(i64, String, String, Option<String>, String)], clips: &[Clip], now: i64, used: usize) -> String {
    let tail = |s: &str, n: usize| -> String {
        let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        let k = s.chars().count();
        if k > n { format!("…{}", s.chars().skip(k - n).collect::<String>()) } else { s }
    };
    let ago = |t: i64| match (now - t) / 60 {
        0 => "just now".to_string(),
        m => format!("{m} min ago"),
    };
    let clip_block = if clips.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = clips.iter().map(|c| format!("[{}] {}", ago(c.at), c.text.chars().take(CLIP_CHARS).collect::<String>())).collect();
        format!("<clipboard>\nWhat the user copied recently, newest first:\n{}\n</clipboard>\n", lines.join("\n"))
    };
    // newest first from the store; keep the newest that fit, then show them oldest first
    let mut lines: Vec<String> = activity
        .iter()
        .map(|(t, app, title, site, text)| {
            let site = site.as_deref().map(|s| format!(" ({s})")).unwrap_or_default();
            format!("[{}] {} · {title}{site}: {}", ago(*t), app.trim_end_matches(".exe"), tail(text, EXCERPT_CHARS))
        })
        .collect();
    let activity_block = |lines: &[String]| -> String {
        if lines.is_empty() {
            return String::new();
        }
        let oldest_first: Vec<&str> = lines.iter().rev().map(String::as_str).collect();
        format!(
            "<recent_activity>\nOther windows the user had open in the last 30 minutes, oldest first; the last line is where they were just before this one:\n{}\n</recent_activity>\n",
            oldest_first.join("\n")
        )
    };
    // measured whole, headers included, so the budget holds exactly
    let room = RECALL_CHARS.saturating_sub(used + clip_block.chars().count());
    while !lines.is_empty() && activity_block(&lines).chars().count() > room {
        lines.pop(); // the oldest
    }
    format!("{}{clip_block}", activity_block(&lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(title: &str, text: &str) -> Capture {
        Capture { app: "chrome.exe".into(), title: title.into(), site: Some("mail.google.com/mail/u/0".into()), text: text.into() }
    }
    #[test]
    fn forgetting() {
        let s = Store::in_memory();
        s.record(&cap("Inbox", "a"), 1000).unwrap();
        let chat = Capture { app: "chrome.exe".into(), title: "Chat".into(), site: Some("web.whatsapp.com/".into()), text: "b".into() };
        s.record(&chat, 2000).unwrap();
        s.record(&cap("Later", "c"), 3000).unwrap();
        assert_eq!(s.ignored("whatsapp.com", false).unwrap(), 1); // counted, not deleted
        assert_eq!(s.ignored("whatsapp.com", true).unwrap(), 1);
        assert_eq!(s.forget_since(2500).unwrap(), 1); // "Later"
        assert_eq!(count(&s), 1);
        assert_eq!(s.forget_all().unwrap(), 1);
        assert!(s.search("", &Filter::default(), 10).unwrap().is_empty());
    }

    #[test]
    fn grading_sends() {
        assert_eq!(verdict(similarity("Sure, I’ll send it today.", "sure, i'll send it today")), "kept");
        assert_eq!(verdict(similarity("Sure, I'll send it by Friday.", "Sure, I'll send it by Monday, sorry!")), "edited");
        assert_eq!(verdict(similarity("Sure, I'll send it by Friday.", "can't today")), "replaced");
        // the user's real case: Gopu's line kept, a second sentence added (similarity 0.49)
        let g = grade("Looking forward to seeing you there!", "Looking forward to seeing you there! I'll bring the slides along too.");
        assert_eq!(g.1, "edited");
        assert_eq!(grade("Sure, I'll send it by Friday.", "can't today").1, "replaced");
        // the thread shows the message with markdown rendered away, plus names and times around it
        assert!(shows_in("Rahul\n10:42\nsending it **today**, ok?\n✓✓", "sending it **today**, ok?"));
        assert!(!shows_in("Rahul\n10:42\nsomething else", "sending it today"));
    }

    #[test]
    fn key_results() {
        let s = Store::in_memory();
        let k = KeyResult {
            app: "Discord.exe".into(), site: None, title: "@Priya".into(), case: "empty".into(),
            user_text: String::new(), output: "Yes, sending it today.".into(), recipient: None,
        };
        let id = s.add_key_result(&k, 1000).unwrap();
        s.set_sent(id, "commit", Some("yes, sending it today"), 1100).unwrap();
        let unseen = s.add_key_result(&k, 1200).unwrap();
        s.set_sent(unseen, "none", None, 1300).unwrap();
        let mut v = s.verdicts().unwrap();
        v.sort();
        assert_eq!(v, [("kept".to_string(), 1), ("unseen".to_string(), 1)]);
        s.forget_since(1150).unwrap(); // forgetting a span covers key results
        assert_eq!(s.verdicts().unwrap(), [("kept".to_string(), 1)]);
    }

    #[test]
    fn recall_blocks() {
        let now = 10_000;
        let act = |t: i64, title: &str| (t, "chrome.exe".to_string(), title.to_string(), None, format!("{title} text"));
        // newest first, as the store gives them; shown oldest first
        let a = vec![act(now - 60, "Newer"), act(now - 600, "Older")];
        let clips = vec![Clip { at: now - 30, text: "https://example.com/doc".into() }];
        let r = recall_text(&a, &clips, now, 0);
        assert!(r.find("Older").unwrap() < r.find("Newer").unwrap());
        assert!(r.contains("<clipboard>") && r.contains("[just now] https://example.com/doc"));
        assert_eq!(recall_text(&[], &[], now, 0), "");
        // over budget: the oldest lines go, the clipboard stays
        let many: Vec<_> = (0..40).map(|i| (now - i, "a.exe".to_string(), format!("w{i}"), None, "x".repeat(300))).collect();
        let r = recall_text(&many, &clips, now, 0);
        assert!(r.chars().count() <= RECALL_CHARS && r.contains("w0:") && !r.contains("w39:") && r.contains("<clipboard>"));
        assert!(r.chars().count() > RECALL_CHARS - 400); // and it fills the budget, not far under it
        // a people page takes its share first: less activity fits
        let with_page = recall_text(&many, &clips, now, 1500);
        assert!(with_page.chars().count() + 1500 <= RECALL_CHARS && with_page.lines().count() < r.lines().count());
    }

    #[test]
    fn clipboard_ring() {
        remember_clip("card 4111 1111 1111 1111", 100);
        remember_clip("card 4111 1111 1111 1111", 200); // the same again (a clipboard restore): not added
        let ring = CLIPS.lock().unwrap().clone();
        assert_eq!(ring.iter().filter(|c| c.text == "card [card]").count(), 1); // masked, once
        drop(ring);
        forget_clips_since(50);
        assert!(CLIPS.lock().unwrap().iter().all(|c| c.at < 50));
    }

    fn count(s: &Store) -> i64 {
        s.0.query_row("select count(*) from captures", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn animated_titles_are_one_window() {
        assert_eq!(clean_title("◐ Gopu MCP server registration - gopu - Visual Studio Code"), "Gopu MCP server registration - gopu - Visual Studio Code");
        assert_eq!(clean_title("⠋ (12+) WhatsApp"), "WhatsApp");
        assert_eq!(clean_title("@Priya - Discord"), "@Priya - Discord");
        assert_eq!(clean_title("(draft) notes"), "(draft) notes");
        let s = Store::in_memory();
        for (t, spin) in [(1000, "◐"), (1010, "◑"), (1020, "✳")] {
            s.record(&cap(&format!("{spin} Claude Code"), "same text"), t).unwrap();
        }
        assert_eq!(count(&s), 1);
    }

    #[test]
    fn same_window_within_five_minutes_is_one_row() {
        let s = Store::in_memory();
        s.record(&cap("Inbox", "hello invoice"), 1000).unwrap();
        s.record(&cap("Inbox", "hello invoice"), 1010).unwrap(); // unchanged: last_seen only
        s.record(&cap("Inbox", "hello invoice and more"), 1200).unwrap(); // changed: text replaced in place
        assert_eq!(count(&s), 1);
        let last: (String, i64) = s.0.query_row("select text, last_seen from captures", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(last, ("hello invoice and more".into(), 1200));
        s.record(&cap("Inbox", "hello invoice and more"), 1200 + MERGE_SECS + 1).unwrap(); // came back later: new row
        s.record(&cap("Other chat", "x"), 1600).unwrap(); // another window
        assert_eq!(count(&s), 3);
    }

    #[test]
    fn search_finds_masked_text_only() {
        let s = Store::in_memory();
        s.record(&cap("Inbox", "send the November invoice, card 4111 1111 1111 1111"), 1000).unwrap();
        let any = Filter::default();
        let hits = s.search("invoice", &any, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("\u{1}invoice\u{2}") && hits[0].snippet.contains("[card]"));
        assert!(s.search("4111", &any, 10).unwrap().is_empty()); // the card number never reached the store
        assert!(s.search("\"unbalanced ) OR", &any, 10).is_ok()); // any input is safe
        // a whole sentence: not every word is there, so any word counts
        assert_eq!(s.search("i am sending the invoice for sure", &any, 10).unwrap().len(), 1);
        // filters, and a blank query lists recent captures
        assert!(s.search("invoice", &Filter { since: Some(2000), app: None }, 10).unwrap().is_empty());
        assert!(s.search("invoice", &Filter { since: None, app: Some("slack.exe".into()) }, 10).unwrap().is_empty());
        assert_eq!(s.search("", &any, 10).unwrap().len(), 1);
    }

    #[test]
    fn old_captures_are_forgotten() {
        let s = Store::in_memory();
        s.record(&cap("Old", "old invoice"), 0).unwrap();
        let later = 90 * 86_400 + 100;
        s.record(&cap("New", "new invoice"), later).unwrap();
        assert_eq!(s.purge(later, 0).unwrap(), 0); // kept forever
        assert_eq!(s.purge(later, 90).unwrap(), 1);
        let hits = s.search("invoice", &Filter::default(), 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "New"); // the search index forgot it too
    }
}
