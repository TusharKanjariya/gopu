//! Search your own memory from the console: `cargo run -p gopu-core --example memory_search -- invoice`
//! Opens %LOCALAPPDATA%\Gopu\memory.db read-only, so it works while Gopu is running. No words: a summary.
use gopu_core::memory::{db_path, now, Filter, Store, MARK_END, MARK_START};

fn main() {
    let path = db_path().expect("no local app data folder");
    let store = Store::open_read_only(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let q = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if q.is_empty() {
        return println!("{}: {}", path.display(), store.summary().unwrap());
    }
    if q == "--results" {
        return println!("key results by verdict: {:?}", store.verdicts().unwrap());
    }
    if q == "--recall" {
        // the recall a tap would carry right now (the clipboard ring lives in the app, so it's empty here); sizes only
        let r = gopu_core::memory::recall(now(), "", "", "", None).unwrap_or_default();
        return println!("recall: {} chars, {} lines (budget {})", r.chars().count(), r.lines().count(), gopu_core::memory::RECALL_CHARS);
    }
    for h in store.search(&q, &Filter::default(), 20).unwrap() {
        let ago = (now() - h.last_seen) / 60;
        println!("#{} {} | {} | {} | {ago} min ago\n    {}\n", h.id, h.app, h.title, h.site.unwrap_or_default(), h.snippet.replace('\n', " ").replace(MARK_START, "[").replace(MARK_END, "]"));
    }
}
