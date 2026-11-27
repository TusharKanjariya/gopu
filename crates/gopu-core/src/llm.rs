//! The one LLM client: OpenAI-compatible chat completions, so every provider preset (and Ollama / llama.cpp) fits.

use serde_json::{json, Value};
use std::time::Duration;

pub struct Llm {
    pub base_url: String,
    pub model: String,
    api_key: String,
    /// low / medium / high reasoning effort.
    pub reasoning: String,
    agent: ureq::Agent,
}

impl Llm {
    /// The client for resolved settings (`Settings::load().resolve(gopu_core::setting)`).
    pub fn new(r: &crate::settings::Resolved) -> Llm {
        let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).build().into();
        Llm {
            api_key: crate::settings::api_key(&r.provider).unwrap_or_default(),
            base_url: r.base_url.clone(),
            model: r.model.clone(),
            reasoning: r.reasoning.clone(),
            agent,
        }
    }

    /// Use this key instead of the saved one (the settings window tests a key before saving it).
    pub fn with_key(mut self, key: String) -> Llm {
        self.api_key = key;
        self
    }

    pub fn has_key(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// One chat completion; returns the reply text.
    pub fn chat(&self, system: &str, user: &str) -> Result<String, String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "reasoning_effort": self.reasoning, // gpt-oss; other servers ignore it
            "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }],
        });
        let mut req = self.agent.post(&url).header("User-Agent", "Gopu");
        if self.has_key() {
            req = req.header("Authorization", &format!("Bearer {}", self.api_key));
        }
        let mut resp = req.send_json(&body).map_err(|e| format!("LLM request failed: {e}"))?;
        let v: Value = resp.body_mut().read_json().map_err(|e| format!("LLM reply unreadable: {e}"))?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "LLM reply had no text".into())
    }
}

/// What went wrong with a model call, in words for Gopu's bubble, when it's something the user can act on.
/// `true`: the fix is in Settings (open it). None: not the model's fault (or unknown), so the general message.
pub fn problem(err: &str) -> Option<(&'static str, bool)> {
    if !err.starts_with("LLM request failed") {
        return None;
    }
    let status = |code: &str| err.contains(&format!("status: {code}")) || err.contains(&format!("status {code}"));
    Some(if status("401") || status("403") {
        ("My model key didn't work. I've opened Settings so you can check it.", true)
    } else if status("429") {
        ("The model is busy right now (its free limit). Try again in a minute.", false)
    } else if status("404") {
        ("The model wasn't found. I've opened Settings: check the model name.", true)
    } else if err.contains("status") {
        ("The model had a problem. Try again in a moment.", false)
    } else {
        ("I couldn't reach the model. Check your internet connection.", false)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn problems_in_plain_words() {
        let p = |e: &str| super::problem(e).map(|(_, settings)| settings);
        assert_eq!(p("LLM request failed: http status: 401"), Some(true));
        assert_eq!(p("LLM request failed: http status: 429"), Some(false));
        assert!(super::problem("LLM request failed: io: failed to lookup address").unwrap().0.contains("internet"));
        assert_eq!(p("the field changed since the tap, so Gopu left it alone"), None);
    }
}
