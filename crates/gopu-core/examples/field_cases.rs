//! Live check of every field case against the configured LLM, with made-up screen content:
//! `cargo run -p gopu-core --example field_cases`
use gopu_core::{assist, llm::Llm, Field, OnScreenContext};

fn main() {
    let me = gopu_core::settings::Settings::load().resolve(gopu_core::setting);
    let llm = Llm::new(&me);
    let anon = gopu_core::settings::Resolved { user_name: None, ..me.clone() };
    let ctx = OnScreenContext {
        app: "Slack.exe".into(),
        window_title: "#launch | Acme".into(),
        before: "Maya Patel\n10:02 AM\nHey! Could you review the launch deck today? Need it before Friday 🙏".into(),
        after: "Message #launch".into(),
        anchored: true,
        url: None,
        chat_header: None,
        took_ms: 0,
    };
    let field = |text: &str, selection: &str| Field { text: text.into(), selection: selection.into(), ..Default::default() };
    let cases = [
        ("empty", field("", "")),
        ("draft", field("ya ill check it later today", "")),
        ("instruction", field("reply saying ill send notes by thursday evening", "")),
        ("instruction, stray quote", field("\"reply saying ill send notes by thursday evening", "")),
        ("selection", field("Sure, I will definately look at it and send you my thoughts.", "definately look at it")),
        ("selected instruction", field("Thanks Maya!\nwrite one line saying the deck looks great", "write one line saying the deck looks great")),
    ];

    // the user's run (2026-11-26): their own words plus a request about how to write them, typed or selected.
    // `-- mixed`: just these
    let intro = OnScreenContext {
        app: "msedge.exe".into(),
        window_title: "Introduce yourself".into(),
        before: "Priya · Team lead\n#welcome · just now\nWelcome aboard! Before Monday's kickoff, could you introduce \
                 yourself to the team? Who you are, what you work on, and one thing you're great at.\nWho am I?"
            .into(),
        after: "Post".into(),
        anchored: true,
        url: None,
        chat_header: None,
        took_ms: 0,
    };
    let mixed = "I am Tushar, Full-Stack Developer. Write like a grinding way";
    let tone = "Write about me in comedy and grinding tone.";
    for (name, f, who) in [
        ("draft + request", field(mixed, ""), &anon),
        ("draft + request, selected", field(mixed, mixed), &anon),
        // with the user's own name and About: the name as set, and a short answer despite the tone request
        ("tone request, as the user", field(tone, ""), &me),
    ] {
        match assist::run(&llm, &f, &intro, who, None) {
            Ok(o) => println!("[{name}] -> {:?} asked={}\n  {}\n", o.case, o.asked, o.text.replace('\n', "\n  ")),
            Err(e) => println!("[{name}] ERROR: {e}\n"),
        }
    }
    if std::env::args().nth(1).as_deref() == Some("mixed") {
        return;
    }

    let only_dm = std::env::args().nth(1).as_deref() == Some("dm"); // `-- dm`: just the chat scenario below
    for (name, f) in cases.into_iter().filter(|_| !only_dm) {
        let t0 = std::time::Instant::now();
        match assist::run(&llm, &f, &ctx, &anon, None) {
            Ok(o) => println!("[{name}] -> {:?} ({} ms)\n  {}\n", o.case, t0.elapsed().as_millis(), o.text.replace('\n', "\n  ")),
            Err(e) => println!("[{name}] ERROR: {e}\n"),
        }
    }

    // A one-to-one chat laid out the way Discord's tree reads: the author's name only on the first message of a group,
    // timestamps and hover actions in between, romanized Gujarati, and the user (Rahul) sent the latest messages.
    let dm = OnScreenContext {
        app: "Discord.exe".into(),
        window_title: "@Priya Shah - Discord".into(),
        before: "Rahul Mehta\nOnline\nUnmute\nDirect Message\nPriya Shah chat\naa report kone banavyo chhe?\nPriya Shah\n\
                 aa to automatically aavyo chhe, mare edit nathi thato\n:thumbsup:\nAdd Reaction\nEdit\nForward\nMore\n3:12 PM\n\
                 meeting 11 vage chhe ke 12?\nReply\nRahul Mehta\nmeeting 11 vage chhe\nHa\n3:19 PM\nzoom link savare mokli dais\n\
                 agenda pan share kari dais\n(\nedited\n)\n👍\n1\nAdd Reaction\nEdit\nForward\nMore"
            .into(),
        after: "Message @Priya Shah\nPriya Shah's profile".into(),
        anchored: true,
        url: None,
        chat_header: None,
        took_ms: 0,
    };
    for name in [Some("Rahul Mehta"), None] {
        let who = gopu_core::settings::Resolved { user_name: name.map(String::from), ..anon.clone() };
        match assist::run(&llm, &field("\u{FEFF}\n", ""), &dm, &who, None) {
            Ok(o) => println!("[empty, user spoke last, name {name:?}] -> {:?}\n  {}\n", o.case, o.text),
            Err(e) => println!("[empty, user spoke last] ERROR: {e}\n"),
        }
    }
}
