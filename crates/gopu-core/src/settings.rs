//! The user's settings: `settings.json` in the app's config dir (`%APPDATA%\Gopu` on Windows); provider keys live in
//! the OS credential store, never in the file. Env vars (`GROQ_API_KEY`, `GOPU_*`) are the fallback, for dev.

use serde::{Deserialize, Serialize};

/// A provider preset: an OpenAI-compatible endpoint and a default model.
pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub model: &'static str,
    pub needs_key: bool,
}

// ponytail: default models age; the window lets the user type any model, so update these when they go stale
pub const PROVIDERS: &[Provider] = &[
    Provider { id: "groq", name: "Groq", base_url: "https://api.groq.com/openai/v1", model: "openai/gpt-oss-120b", needs_key: true },
    Provider { id: "openai", name: "OpenAI", base_url: "https://api.openai.com/v1", model: "gpt-5-mini", needs_key: true },
    Provider { id: "anthropic", name: "Anthropic", base_url: "https://api.anthropic.com/v1", model: "claude-haiku-5-5", needs_key: true },
    Provider { id: "openrouter", name: "OpenRouter", base_url: "https://openrouter.ai/api/v1", model: "openai/gpt-oss-120b", needs_key: true },
    Provider { id: "ollama", name: "Ollama", base_url: "http://localhost:11434/v1", model: "gpt-oss:20b", needs_key: false },
    Provider { id: "custom", name: "Custom", base_url: "", model: "", needs_key: false },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// What's saved. Every field is optional: unset falls back to the provider preset, then env vars, then Groq.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// low / medium / high reasoning effort, for models that take it.
    pub reasoning: Option<String>,
    /// How the user appears in chats, so their own messages aren't mistaken for others'.
    pub user_name: Option<String>,
    /// Free text about the user (role, work, context), in their own words.
    pub about: Option<String>,
    /// The user's style notes ("always / never …"), applied to everything Gopu writes.
    pub style: Option<String>,
    /// Turned off from the tray: taps do nothing.
    pub paused: bool,
    /// The Gopu key (a virtual-key code from `KEYS`); Right Ctrl when unset.
    pub key: Option<u32>,
    /// A backup shortcut, for keyboards without the key.
    pub chord: Option<Chord>,
    /// Write the last tap (screen text included) to the log folder, for bug reports.
    pub debug: bool,
    /// The first-run flow finished (a real tap landed).
    pub onboarded: bool,
    /// Memory turned off (the welcome's "Not now", or the Privacy page). Capture is on unless this is set.
    pub memory_off: bool,
    /// Memory paused until this time (Unix seconds); the Gopu key keeps working.
    pub memory_paused_until: Option<i64>,
    /// The user's ignore list: apps (exe name) and sites (a domain covers its subdomains) memory never captures.
    pub ignore: Vec<String>,
    /// How many days memory keeps captures; 0 = forever, unset = 90.
    pub keep_days: Option<u32>,
    /// "Use memory in replies" unticked: Gopu-key requests carry no recall.
    pub recall_off: bool,
    /// Other AI apps may read memory through the MCP server (off until the user turns it on).
    pub mcp_on: bool,
    /// MCP clients (their handshake name) the user cut off.
    pub mcp_blocked: Vec<String>,
    /// "Start Gopu with Windows" unticked.
    pub autostart_off: bool,
    /// "Check for updates once a day" unticked.
    pub updates_off: bool,
    /// "Offer reminders from my replies" unticked (Reminders page).
    pub offers_off: bool,
    /// The Ask Gopu box is pinned: it stays on screen when the user clicks elsewhere.
    pub ask_pinned: bool,
    /// Where the pinned box was left: its bottom-left corner (physical px), so it grows upward from there.
    pub ask_at: Option<(i32, i32)>,
    /// Where Ask Gopu waits on screen: "taskbar" (Gopu in the taskbar), else a tab on the screen edge.
    pub ask_anchor: Option<String>,
    /// Where the tab was left (its top-left, physical px).
    pub ask_tab_at: Option<(i32, i32)>,
    /// The screen edge the tab is stuck to: "right" (unset), "left", "top", "bottom", or "float" (dropped mid-screen).
    pub ask_tab_edge: Option<String>,
    /// Ask Gopu's saved asks: the ★ buttons in the box.
    pub ask_saved: Vec<SavedAsk>,
    /// The end-of-day reminder from before check-ins (0.1.0); `load` turns it into the first check-in.
    pub ask_reminder: Option<Reminder>,
    /// Check-ins: saved asks offered on a schedule (reminders map, slice 03).
    pub check_ins: Vec<CheckIn>,
    /// One-off reminders ("Call with Anna at 5 PM"), until the user marks them done.
    pub reminders: Vec<OneOff>,
    /// Fields this build doesn't know (written by a newer Gopu), kept as they are so saving never drops them.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// A one-off reminder (reminders map, slice 02). Kept in settings, not memory: forgetting memory never drops one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OneOff {
    pub id: i64,
    pub title: String,
    /// When it is (Unix seconds).
    pub at: i64,
    /// Show it this many minutes early.
    pub early: u32,
    /// Snoozed: show it again at this time instead.
    pub snoozed: Option<i64>,
}

impl OneOff {
    pub fn shows_at(&self) -> i64 {
        self.snoozed.unwrap_or(self.at - self.early as i64 * 60)
    }

    /// Due at `now`: `Some(missed)`. Missed when its time itself had passed by the time it could show (the PC was
    /// asleep or Gopu closed), so the note says so instead of "in 10 min".
    pub fn due(&self, now: i64) -> Option<bool> {
        (now >= self.shows_at()).then_some(self.snoozed.is_none() && now > self.at + 120)
    }
}

/// The end-of-day reminder as 0.1.0 saved it: weekdays at `at` (minutes after local midnight), offering `ask`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    pub at: u32,
    pub ask: String,
}

/// Monday to Friday, as check-in days (bit 0 is Sunday).
pub const WEEKDAYS: u8 = 0b011_1110;

/// A check-in: on the chosen days at `at` (minutes after local midnight), a note offers the saved ask named `ask`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckIn {
    pub id: i64,
    pub ask: String,
    /// Bit n set: due on weekday n (0 is Sunday).
    pub days: u8,
    pub at: u32,
    pub on: bool,
}

impl CheckIn {
    /// The local day (days since 1970-01-01) if it's due at `now`: one of its days, within the hour after its time
    /// (a PC woken later than that skips the day). The caller shows it once per day.
    pub fn due(&self, now: i64, utc_offset: i64) -> Option<i64> {
        let local = now + utc_offset;
        let (day, mins) = (local.div_euclid(86_400), local.rem_euclid(86_400) / 60);
        let weekday = (day + 4).rem_euclid(7); // 1970-01-01 was a Thursday
        (self.on && self.days & (1 << weekday) != 0 && (self.at as i64..self.at as i64 + 60).contains(&mins)).then_some(day)
    }
}

/// A saved ask: the button's name and the request it sends (a format, a scope, whatever the user wrote).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedAsk {
    pub name: String,
    pub text: String,
}

impl Settings {
    /// Ask Gopu waits in the taskbar (else in the edge tab, the default).
    pub fn ask_in_taskbar(&self) -> bool {
        self.ask_anchor.as_deref() == Some("taskbar")
    }
}

/// Is memory capturing right now?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryState {
    On,
    /// Paused until this time (Unix seconds).
    Paused(i64),
    Off,
}

/// A shortcut: modifier flags (RegisterHotKey's MOD_ALT 1, MOD_CONTROL 2, MOD_SHIFT 4, MOD_WIN 8) plus a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chord {
    pub mods: u32,
    pub vk: u32,
}

/// Keys that can be the Gopu key: a lone tap of each does nothing in apps. Not Alt (opens menus), Win (opens
/// Start) or Caps Lock (toggles caps).
// ponytail: Windows virtual-key codes; macOS gets its own table with its platform module
pub const KEYS: &[(u32, &str)] = &[(0xA3, "Right Ctrl"), (0xA2, "Left Ctrl"), (0xA1, "Right Shift"), (0xA0, "Left Shift")];

impl Settings {
    pub fn memory_state(&self, now: i64) -> MemoryState {
        match self.memory_paused_until {
            _ if self.memory_off => MemoryState::Off,
            Some(until) if until > now => MemoryState::Paused(until),
            _ => MemoryState::On,
        }
    }

    pub fn keep_days(&self) -> u32 {
        self.keep_days.unwrap_or(90)
    }

    /// The Gopu key's code: the saved one if it's allowed, else Right Ctrl.
    pub fn key_vk(&self) -> u32 {
        self.key.filter(|k| KEYS.iter().any(|(v, _)| v == k)).unwrap_or(KEYS[0].0)
    }
}

pub fn gopu_key_name(vk: u32) -> &'static str {
    KEYS.iter().find(|(v, _)| *v == vk).map_or("Right Ctrl", |(_, n)| n)
}

/// The settings a tap runs with, everything resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub reasoning: String,
    pub user_name: Option<String>,
    pub about: Option<String>,
    pub style: Option<String>,
}

impl Settings {
    fn path() -> Option<std::path::PathBuf> {
        crate::platform::config_dir().map(|d| d.join("settings.json"))
    }

    /// The saved settings; defaults when there's no file yet or it can't be read.
    pub fn load() -> Settings {
        let s: Settings =
            Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        s.migrated()
    }

    /// 0.1.0's end-of-day reminder becomes the first check-in (saved with the next save).
    fn migrated(mut self) -> Settings {
        if let Some(r) = self.ask_reminder.take() {
            if self.check_ins.is_empty() {
                self.check_ins.push(CheckIn { id: 1, ask: r.ask, days: WEEKDAYS, at: r.at, on: true });
            }
        }
        self
    }

    /// Written to a temporary file, then swapped in: a read never sees half a file (which would load as defaults, and
    /// the next save would wipe everything).
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config folder")?;
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    /// Saved value → provider preset → env var → Groq default. `env` is `crate::setting` outside tests.
    pub fn resolve(&self, env: impl Fn(&str) -> Option<String>) -> Resolved {
        let set = |v: &Option<String>| v.clone().filter(|v| !v.trim().is_empty());
        let preset = set(&self.provider).as_deref().and_then(provider);
        let from_preset = |f: fn(&Provider) -> &str| preset.map(f).filter(|v| !v.is_empty()).map(String::from);
        let groq = &PROVIDERS[0];
        Resolved {
            provider: preset.map_or("groq", |p| p.id).into(),
            base_url: set(&self.base_url).or(from_preset(|p| p.base_url)).or(env("GOPU_BASE_URL")).unwrap_or(groq.base_url.into()),
            model: set(&self.model).or(from_preset(|p| p.model)).or(env("GOPU_MODEL")).unwrap_or(groq.model.into()),
            reasoning: set(&self.reasoning).or(env("GOPU_REASONING")).unwrap_or("medium".into()),
            user_name: set(&self.user_name).or(env("GOPU_USER_NAME")),
            about: set(&self.about),
            style: set(&self.style),
        }
    }
}

/// The key for a provider: the credential store, else `GROQ_API_KEY` for Groq.
pub fn api_key(provider: &str) -> Option<String> {
    crate::platform::secret_get(&key_name(provider)).or_else(|| (provider == "groq").then(|| crate::setting("GROQ_API_KEY")).flatten())
}

/// Saves (or with `None`, removes) a provider's key in the credential store.
pub fn set_api_key(provider: &str, key: Option<&str>) -> Result<(), String> {
    crate::platform::secret_set(&key_name(provider), key)
}

/// One credential per provider, so switching back keeps the old key: `Gopu:groq`, `Gopu:openai`, …
fn key_name(provider: &str) -> String {
    format!("Gopu:{provider}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_order() {
        let none = |_: &str| None;
        let env = |k: &str| (k == "GOPU_MODEL" || k == "GOPU_BASE_URL").then(|| format!("env-{k}"));
        // nothing saved, nothing in env: Groq
        let r = Settings::default().resolve(none);
        assert_eq!((r.provider.as_str(), r.base_url.as_str(), r.model.as_str(), r.reasoning.as_str()), ("groq", PROVIDERS[0].base_url, PROVIDERS[0].model, "medium"));
        // nothing saved: env vars still work (dev setups)
        assert_eq!(Settings::default().resolve(env).model, "env-GOPU_MODEL");
        // a saved provider's preset beats env
        let s = Settings { provider: Some("ollama".into()), ..Default::default() };
        assert_eq!(s.resolve(env).base_url, "http://localhost:11434/v1");
        // a saved value beats the preset; blank counts as unset
        let s = Settings { provider: Some("openai".into()), model: Some("my-model".into()), base_url: Some(" ".into()), ..Default::default() };
        assert_eq!((s.resolve(none).model.as_str(), s.resolve(none).base_url.as_str()), ("my-model", "https://api.openai.com/v1"));
        // custom has no preset URL: falls through to env
        let s = Settings { provider: Some("custom".into()), ..Default::default() };
        assert_eq!(s.resolve(env).base_url, "env-GOPU_BASE_URL");
        // the Gopu key: only allowed keys, else Right Ctrl
        assert_eq!(Settings::default().key_vk(), 0xA3);
        assert_eq!(Settings { key: Some(0xA1), ..Default::default() }.key_vk(), 0xA1);
        assert_eq!(Settings { key: Some(0x5B), ..Default::default() }.key_vk(), 0xA3); // Win: not allowed
        // old or hand-edited files: missing fields default; fields from a newer Gopu survive a save (an older build
        // once wiped the user's reminders by dropping them)
        let s: Settings = serde_json::from_str(r#"{"model":"x","later":[1]}"#).unwrap();
        assert_eq!((s.model.as_deref(), s.paused), (Some("x"), false));
        let back: serde_json::Value = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back["later"], serde_json::json!([1]));
        // memory: on by default; a pause ends by itself; off wins
        assert_eq!(Settings::default().memory_state(100), MemoryState::On);
        let p = Settings { memory_paused_until: Some(200), ..Default::default() };
        assert_eq!((p.memory_state(100), p.memory_state(200)), (MemoryState::Paused(200), MemoryState::On));
        assert_eq!(Settings { memory_off: true, ..p }.memory_state(100), MemoryState::Off);
        // a check-in: its days only, within the hour after its time, local time; off is never due
        let monday_6pm = 4 * 86_400 + 18 * 3600; // 1970-01-05
        let c = CheckIn { id: 1, ask: "Daily report".into(), days: WEEKDAYS, at: 18 * 60, on: true };
        assert_eq!(c.due(monday_6pm, 0), Some(4));
        assert_eq!(c.due(monday_6pm + 59 * 60, 0), Some(4));
        assert_eq!(c.due(monday_6pm + 60 * 60, 0), None);
        assert_eq!(c.due(monday_6pm - 60, 0), None);
        assert_eq!(c.due(monday_6pm - 86_400, 0), None); // Sunday
        assert_eq!(c.due(monday_6pm - 5 * 3600, 5 * 3600), Some(4)); // 1 pm UTC is 6 pm at UTC+5
        assert_eq!(CheckIn { days: 1, ..c.clone() }.due(monday_6pm - 86_400, 0), Some(3)); // Sundays only
        assert_eq!(CheckIn { on: false, ..c.clone() }.due(monday_6pm, 0), None);
        // 0.1.0's end-of-day reminder becomes the first check-in, once
        let old: Settings = serde_json::from_str(r#"{"ask_reminder":{"at":1080,"ask":"Daily report"}}"#).unwrap();
        let m = old.migrated();
        assert_eq!((m.ask_reminder.is_none(), m.check_ins.clone()), (true, vec![c.clone()]));
        assert_eq!(m.migrated().check_ins.len(), 1);
        // one-offs: shown `early` minutes before; missed when the time itself went by unseen; a snooze moves it
        let r = OneOff { id: 1, title: "Call".into(), at: 1000 * 60, early: 10, snoozed: None };
        assert_eq!((r.due(989 * 60), r.due(990 * 60), r.due(1001 * 60)), (None, Some(false), Some(false)));
        assert_eq!(r.due(1040 * 60), Some(true)); // woke 40 minutes late
        let s = OneOff { snoozed: Some(1100 * 60), ..r };
        assert_eq!((s.due(1040 * 60), s.due(1100 * 60)), (None, Some(false))); // snoozed past its time: not "missed"
    }
}
