//! Live check of the "remind me …" reader against the configured model: `cargo run -p gopu-core --example remind_cases`.

fn main() {
    let me = gopu_core::settings::Settings::load().resolve(gopu_core::setting);
    let llm = gopu_core::llm::Llm::new(&me);
    let now = gopu_core::memory::now();
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
