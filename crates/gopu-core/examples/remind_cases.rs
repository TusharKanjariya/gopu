//! Live check of the "remind me" reader: `cargo run -p gopu-core --example remind_cases [-- ask]` (ask: other languages, through the normal ask).

fn main() {
    let me = gopu_core::settings::Settings::load().resolve(gopu_core::setting);
    let llm = gopu_core::llm::Llm::new(&me);
    let now = gopu_core::memory::now();
    if std::env::args().any(|a| a == "ask") {
        // any language goes through the normal ask, which notices a reminder request itself
        let store = gopu_core::memory::Store::open_read_only(&gopu_core::memory::db_path().unwrap()).unwrap();
        for q in ["mane aaje 6:30 e meeting mate yaad karavje", "kal subah 10 baje doctor ke paas jaana yaad dilana", "Recuérdame llamar a Ana mañana a las 5", "what did I do today?"] {
            std::thread::sleep(std::time::Duration::from_secs(25)); // the free tier's per-minute limit
            let Ok(a) = gopu_core::ask::ask(&llm, &store, q, &[], &[], &me, now).map_err(|e| println!("{q}
  -> {e}")) else { continue };
            println!("{q}
  -> {:?} {}", a.reminder, a.text.chars().take(60).collect::<String>());
        }
        return;
    }
    for q in [
        "remind me call with anna at 5, 10 min early",
        "remind me to send the invoice tomorrow",
        "remind me in 20 minutes to check the build",
        "Remind me about the dentist on Monday at 9:30 am, half an hour before",
        "remind me to stretch",
    ] {
        println!("{q}\n  -> {:?}", gopu_core::ask::reminder(&llm, q, now));
    }
}
