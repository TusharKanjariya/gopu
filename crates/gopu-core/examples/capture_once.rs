//! What capture reads from a window. Prints counts and the never-capture verdict, not the text.
//! `cargo run -p gopu-core --example capture_once`: the window in front, after 8 s to switch to it.
//! `cargo run -p gopu-core --example capture_once -- Notepad`: the first window of that window class.
//! `cargo run -p gopu-core --example capture_once -- @firefox`: waits (up to 60 s) until that app is in front, then reads it.
use gopu_core::{platform, privacy};

fn main() {
    let w = match std::env::args().nth(1) {
        Some(app) if app.starts_with('@') => {
            let want = app[1..].to_lowercase();
            let t0 = std::time::Instant::now();
            while !platform::foreground_app().to_lowercase().starts_with(&want) && t0.elapsed().as_secs() < 60 {
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            platform::foreground_text()
        }
        Some(class) => {
            let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowW(&windows::core::HSTRING::from(class), None) }
                .expect("no window of that class");
            platform::read_window(hwnd.0)
        }
        None => {
            std::thread::sleep(std::time::Duration::from_secs(8));
            platform::foreground_text()
        }
    };
    let Some(w) = w else { return println!("nothing read (Gopu's own window, a hung app, or a timeout)") };
    let site = w.url.as_deref().and_then(privacy::site_from_url);
    println!("app: {} | title: {} chars | site: {:?}", w.app, w.title.chars().count(), site);
    println!("text: {} chars | never capture: {}", w.text.chars().count(), privacy::never_capture(&w.app, &w.title, site.as_deref()));
}
