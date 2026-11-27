//! What capture reads from a window. Prints counts and the never-capture verdict, not the text.
//! `cargo run -p gopu-core --example capture_once`: the window in front, after 3 s to switch to it.
//! `cargo run -p gopu-core --example capture_once -- Notepad`: the first window of that window class.
use gopu_core::{platform, privacy};

fn main() {
    let w = match std::env::args().nth(1) {
        Some(class) => {
            let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::FindWindowW(&windows::core::HSTRING::from(class), None) }
                .expect("no window of that class");
            platform::read_window(hwnd.0)
        }
        None => {
            std::thread::sleep(std::time::Duration::from_secs(3));
            platform::foreground_text()
        }
    };
    let Some(w) = w else { return println!("nothing read (Gopu's own window, a hung app, or a timeout)") };
    let site = w.url.as_deref().and_then(privacy::site_from_url);
    println!("app: {} | title: {} chars | site: {:?}", w.app, w.title.chars().count(), site);
    println!("text: {} chars | never capture: {}", w.text.chars().count(), privacy::never_capture(&w.app, &w.title, site.as_deref()));
}
