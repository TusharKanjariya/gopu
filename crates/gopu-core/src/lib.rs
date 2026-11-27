//! Gopu's platform-neutral core. Words follow GLOSSARY.md.

pub mod ask;
pub mod assist;
pub mod llm;
pub mod mcp;
pub mod memory;
pub mod people;
pub mod platform;
pub mod privacy;
pub mod settings;
pub mod style;

/// A setting from the environment, falling back to the saved user variable (a shell or app started before the
/// variable was set doesn't see it).
pub fn setting(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty()).or_else(|| platform::user_env(name))
}

/// Did a write-back land? The field now holds the start of `wrote` and, when it was meant to replace `replaced`, no
/// longer holds that old text. Compared on letters and digits only: apps reflow text, add trailing newlines and
/// zero-width placeholders, and swap emoji for images that read back as `:thumbsup:` (Discord).
pub fn landed(field_text: &str, wrote: &str, replaced: Option<&str>) -> bool {
    let squash = |s: &str| -> String { s.chars().filter(|c| c.is_alphanumeric()).collect() };
    let (now, new) = (squash(field_text), squash(wrote));
    let start: String = new.chars().take(40).collect();
    let end: String = new.chars().skip(new.chars().count().saturating_sub(40)).collect();
    let old_left = replaced.map(squash).is_some_and(|old| !old.is_empty() && !new.contains(&old) && now.contains(&old));
    !start.is_empty() && now.contains(&start) && now.contains(&end) && !old_left
}

/// Splits `text` at word ends into pieces of about `words` words, at most `max` pieces (longer texts get bigger
/// pieces). Concatenated, the pieces are exactly `text`. Write-back pastes them one by one so Gopu walks with the text.
/// Each piece ends on a word and the whitespace leads the next one: Gmail drops a pasted piece's trailing space.
pub fn chunks(text: &str, words: usize, max: usize) -> Vec<&str> {
    let ends: Vec<usize> = text
        .char_indices()
        .zip(std::iter::once(' ').chain(text.chars()))
        .filter(|((i, c), prev)| c.is_whitespace() && !prev.is_whitespace() && !text[*i..].trim().is_empty())
        .map(|((i, _), _)| i)
        .collect();
    let per = words.max((ends.len() + 1).div_ceil(max.max(1))).max(1);
    let mut out = vec![];
    let mut last = 0;
    for cut in ends.iter().skip(per - 1).step_by(per) {
        out.push(&text[last..*cut]);
        last = *cut;
    }
    out.push(&text[last..]);
    out
}

/// Splits a field's text into (draft, gap, kept). Kept text is what an email compose box holds below the user's own
/// writing: a signature ("-- ") or a quoted thread ("On … wrote:", Outlook's "From:/Sent:" header or separator).
/// Write-back only replaces draft + gap and re-adds the gap, so kept text survives. No kept text: (text, "", "").
// ponytail: a plain document with a lone "--" line also counts as kept below it; narrow to email apps if that bites
pub fn split_kept(text: &str) -> (&str, &str, &str) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut at = 0;
    for (i, line) in lines.iter().enumerate() {
        let l = line.trim();
        let next = &lines[i + 1..];
        let marker = l == "--"
            || l == "-----Original Message-----"
            || (l.len() >= 10 && l.chars().all(|c| c == '_'))
            || (l.starts_with("On ") && l.ends_with("wrote:"))
            || (l.starts_with("From:") && next.iter().take(4).any(|n| n.trim_start().starts_with("Sent:") || n.trim_start().starts_with("Date:")));
        if marker {
            let draft = text[..at].trim_end();
            return (draft, &text[draft.len()..at], &text[at..]);
        }
        at += line.len();
    }
    (text, "", "")
}

/// A screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The focused field at the moment of a Gopu-key tap. Never a password field.
#[derive(Debug, Clone, Default)]
pub struct Field {
    /// The field rectangle.
    pub rect: Rect,
    /// The caret rectangle. `None` when the field is empty or the app doesn't report one;
    /// either way Gopu treats the caret as being at the field's text start.
    pub caret: Option<Rect>,
    /// Everything in the field.
    pub text: String,
    /// The highlighted text, empty when nothing is selected.
    pub selection: String,
    /// The field text up to the caret, when the app reports where the caret is.
    pub before_caret: Option<String>,
    /// The focus can't be typed into (a page, a button, a list): the tap says so instead of thinking.
    pub not_editable: bool,
}

/// The on-screen context of one tap: the focused window's text in document order, split around the field.
/// Untrusted data, never instructions.
#[derive(Debug, Clone, Default)]
pub struct OnScreenContext {
    pub app: String,
    pub window_title: String,
    /// Text above/before the field, trimmed so the part nearest the field survives.
    pub before: String,
    /// Text below/after the field, trimmed so the part nearest the field survives.
    pub after: String,
    /// False when the field wasn't found in the window's tree: everything is in `before`, and it's noisier.
    pub anchored: bool,
    /// A browser's address bar text, so memory's site rules apply to the tap's window too.
    pub url: Option<String>,
    /// A one-to-one chat's name from the conversation header (`people::chat_header`), read from the whole window so a
    /// long chat can't push it out of `before`.
    pub chat_header: Option<String>,
    pub took_ms: u128,
}

pub const MAX_BEFORE: usize = 6000;
pub const MAX_AFTER: usize = 1500;

/// Join lines, keeping at most `max` chars from the end (the lines nearest the field when they come before it).
pub fn keep_tail(lines: &[String], max: usize) -> String {
    let s = lines.join("\n");
    let n = s.chars().count();
    s.chars().skip(n.saturating_sub(max)).collect()
}

/// Join lines, keeping at most `max` chars from the start (the lines nearest the field when they come after it).
pub fn keep_head(lines: &[String], max: usize) -> String {
    lines.join("\n").chars().take(max).collect()
}

impl Field {
    /// Screen x where Gopu stands: by the caret, else at the text start.
    pub fn stand_x(&self) -> i32 {
        const TEXT_START: i32 = 12;
        let x = self.caret.map_or(self.rect.x + TEXT_START, |c| c.x);
        x.clamp(self.rect.x, self.rect.x + self.rect.w)
    }

    /// Screen y Gopu's feet rest on: the field's top border, unless the caret sits more than about two lines below it
    /// (a document or a tall box), then the top of the caret's own line so Gopu stays by the caret.
    pub fn stand_y(&self) -> i32 {
        match self.caret {
            Some(c) if c.y - self.rect.y > 2 * c.h.max(12) => c.y,
            _ => self.rect.y,
        }
    }
}

/// Keep a caret rect only if it looks like a caret: a thin bar inside the field.
/// An empty Chromium field reports the whole field as its "caret" (spike 03); that gets `None`.
pub fn plausible_caret(field: Rect, c: Rect) -> Option<Rect> {
    const SLACK: i32 = 4;
    let inside = c.x >= field.x - SLACK
        && c.x <= field.x + field.w + SLACK
        && c.y >= field.y - SLACK
        && c.y <= field.y + field.h + SLACK;
    (c.w <= SLACK && c.h > 0 && inside).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    const F: Rect = Rect { x: 100, y: 200, w: 400, h: 40 };

    #[test]
    fn caret_rules() {
        let bar = Rect { x: 180, y: 210, w: 1, h: 16 };
        assert_eq!(plausible_caret(F, bar), Some(bar));
        assert_eq!(plausible_caret(F, F), None); // empty Chromium field
        assert_eq!(plausible_caret(F, Rect { x: 900, y: 210, w: 1, h: 16 }), None); // elsewhere on screen
        assert_eq!(plausible_caret(F, Rect { h: 0, ..bar }), None);

        assert_eq!(Field { rect: F, caret: Some(bar), ..Default::default() }.stand_x(), 180);
        assert_eq!(Field { rect: F, ..Default::default() }.stand_x(), 112); // text start

        // a one-line box: the field's top border
        assert_eq!(Field { rect: F, caret: Some(bar), ..Default::default() }.stand_y(), 200);
        // a document with the caret on line 4 (the user's Notepad): the caret's own line
        let doc = Rect { x: 0, y: 60, w: 800, h: 500 };
        let deep = Rect { x: 250, y: 140, w: 1, h: 20 };
        assert_eq!(Field { rect: doc, caret: Some(deep), ..Default::default() }.stand_y(), 140);
        assert_eq!(Field { rect: doc, ..Default::default() }.stand_y(), 60); // no caret: top border
    }

    #[test]
    fn landed_check() {
        assert!(landed("\u{FEFF}Sure, I'll take a look.\n", "Sure, I'll take a look.", None));
        assert!(landed("Sure, I'll\ntake a look.", "Sure, I'll take a look.", None)); // reflowed
        assert!(!landed("ya ill check it later", "Yes, I'll check it later today.", None));
        assert!(!landed("anything", "   ", None)); // nothing to look for never counts as landed
        // the bug the user hit: select-all failed, so the result was appended after the old draft
        let old = "ya ill check it later";
        assert!(!landed("ya ill check it laterYa, I'll check it later.", "Ya, I'll check it later.", Some(old)));
        assert!(landed("Ya, I'll check it later.", "Ya, I'll check it later.", Some(old)));
        assert!(landed("ok sure", "ok sure", Some("ok"))); // new text containing the old text is fine
        // the user's Discord run: an emoji Gopu wrote reads back as its :shortcode: once the app renders it
        assert!(landed("Sure, I'll send it over once it's done. :thumbsup:", "Sure, I’ll send it over once it’s done. 👍", None));
        assert!(landed("Koi extra changes nathi :blush:", "Koi extra changes nathi 😊", None));
    }

    #[test]
    fn kept_text() {
        assert_eq!(split_kept("ya ill check"), ("ya ill check", "", ""));
        assert_eq!(split_kept("thx will do\n\n-- \nAlex\n"), ("thx will do", "\n\n", "-- \nAlex\n"));
        let gmail = "sounds good\n\nOn Thu, Nov 19, 2026 at 3:04 PM Maya <m@x.com> wrote:\n> can you check?";
        assert_eq!(split_kept(gmail).0, "sounds good");
        assert!(split_kept(gmail).2.starts_with("On Thu"));
        let outlook = "ok\r\n\r\nFrom: Maya\r\nSent: Thursday\r\nTo: me\r\n";
        assert_eq!(split_kept(outlook), ("ok", "\r\n\r\n", "From: Maya\r\nSent: Thursday\r\nTo: me\r\n"));
        assert_eq!(split_kept("--\nsig").0, ""); // only kept text: an empty draft
        assert_eq!(split_kept("From: the start, it was fine").2, ""); // "From:" without a header block isn't kept
        assert_eq!(split_kept("a -- b").2, "");
    }

    #[test]
    fn chunking() {
        assert_eq!(chunks("a b c d e", 2, 16), ["a b", " c d", " e"]);
        assert_eq!(chunks("Hi Maya,\n\nyes. ", 1, 16), ["Hi", " Maya,", "\n\nyes. "]); // trailing whitespace stays put
        assert_eq!(chunks("ok\n\n", 1, 16), ["ok\n\n"]); // the gap before kept text rides with the last word
        assert_eq!(chunks("", 3, 16), [""]);
        let long = "word ".repeat(1000);
        let parts = chunks(&long, 3, 16);
        assert!(parts.len() <= 16 && parts.concat() == long); // long text: bigger pieces, nothing lost
        assert_eq!(chunks("  lead", 3, 16).concat(), "  lead");
    }

    #[test]
    fn trimming_keeps_text_nearest_the_field() {
        let lines: Vec<String> = ["old", "older msg", "latest msg"].map(String::from).to_vec();
        assert_eq!(keep_tail(&lines, 10), "latest msg");
        assert_eq!(keep_head(&lines, 3), "old");
        assert_eq!(keep_tail(&["héllo".into()], 4), "éllo"); // counts chars, never splits one
    }
}
