//! What Ask Gopu would send the model for a question, from your own memory, without calling it:
//! `cargo run -p gopu-core --example ask_sources -- what am I working on right now`
use gopu_core::memory::{db_path, now, Store};

fn main() {
    let store = Store::open_read_only(&db_path().expect("no folder")).expect("memory");
    let q = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let (block, list) = gopu_core::ask::sources(&store, &q, &[], now());
    println!("{} sources, {} chars\n", list.len(), block.chars().count());
    for line in block.lines().filter(|l| l.starts_with('[')) {
        println!("{line}");
    }
}
