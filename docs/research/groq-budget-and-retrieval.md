# Groq free-tier budget and retrieval for memory v1.1

Researched 2026-11-23 for `.scratch/memory-v1.1/issues/08-groq-budget.md`. All Groq figures come from Groq's own docs as fetched that day, checked against one live request (headers below). Rate limits change, so re-check before release. Anything that is inferred and not stated by a source is marked **inferred**.

---

## Recommendation

1. **The key gets `openai/gpt-oss-120b` to itself, and background jobs run on `openai/gpt-oss-20b`.** Groq lists limits per model ID, so each model has its own 200K tokens/day. Putting people pages and style rules on 20b costs the key nothing. Groq itself names 20b as the replacement for `llama-3.1-8b-instant`, which is now enterprise-only ([deprecations](https://console.groq.com/docs/deprecations), [models](https://console.groq.com/docs/models)). **Inferred:** that the buckets are separate per model. The docs table is per model, but no sentence says so outright. Confirm by reading 20b's headers after a 120b call.
2. **The 8K tokens-per-minute cap limits the size of every key request, before any daily budget comes into play.** One tap is input plus completion (with reasoning). Today's 3–8K of input plus ~1K of output already reaches 8K, and the recall sections (+2–4K) would go past it. Cap the key at **about 6K input and `max_completion_tokens` about 1.5K (7.5K total)**. Recall has to fit inside that cap: trim on-screen context to make room, don't add on top. Put the stable parts first (system prompt, style rules, people page) so Groq's prompt cache can hit: "Cached tokens do not count towards your rate limits" ([prompt caching](https://console.groq.com/docs/prompt-caching)).
3. **Daily split, if everything stays on one model: reserve 75% of 120b's 200K TPD for the key (150K ≈ 20–30 full taps) and give background at most 50K (≈ 10 jobs).** That is tight. That is why point 1 is the default.
4. **Background budget on 20b: use 160K of its 200K TPD (80%, keeping a margin), about 30 jobs a day.** For example, 20 people-page refreshes (~5.5K each) plus 10 style-rule batches of 5 draft→sent pairs each (~4.5K each). Run at most one job a minute (8K TPM). Use `reasoning_effort: "low"`.
5. **Graceful fallback:** if 120b returns 429 on a tap, retry the tap once on 20b, not after `retry-after`. A slightly weaker reply now beats a spinner.
6. **No embeddings in v1.1. FTS5 + `bm25()` is enough.** People pages are looked up by name or handle, and recall in the key is filtered by time, app and names. Both are lexical. Groq has no usable embeddings model (see §4). Defer `sqlite-vec` + fastembed-rs until a search fails that FTS5 can't fix. Then use bge-small-en-v1.5. ADR 0001's line "`sqlite-vec` and local embeddings arrive with memory in v1.1" should be amended to "when needed".

---

## 1. Free-tier limits

Source: [console.groq.com/docs/rate-limits](https://console.groq.com/docs/rate-limits), "Free Plan Limits" tab, read 2026-11-23. The page notes that "the limits shown below are the base limits for the Developer plan" (that sentence sits next to the tabs). The free-plan tab is the one below. An organization's exact limits are at console.groq.com/settings/limits.

| Model | RPM | RPD | TPM | TPD | Notes |
|---|---|---|---|---|---|
| `openai/gpt-oss-120b` | 30 | 1,000 | 8,000 | 200,000 | Current Gopu default. Prompt caching supported |
| `openai/gpt-oss-20b` | 30 | 1,000 | 8,000 | 200,000 | ~1000 tok/s vs 500 for 120b ([models](https://console.groq.com/docs/models)). Prompt caching supported |
| `qwen/qwen3.8-27b` | 30 | 1,000 | 8,000 | 200,000 | Preview model, max completion 16,384 |
| `openai/gpt-oss-safeguard-20b` | 3 | 1,000 | 2,000 | 200,000 | Moderation model, not for writing |
| `meta-llama/llama-prompt-guard-2-22m` / `-86m` | 30 | 14,400 | 15,000 | 500,000 | Classifiers, 512-token context |
| `llama-3.1-8b-instant`, `llama-3.3-70b-versatile` | n/a | n/a | n/a | n/a | Deprecated August 16, 2026. Now "Enterprise / Contact Sales" only ([deprecations](https://console.groq.com/docs/deprecations)) |

Developer plan (paid), for comparison: 120b and 20b get 1K RPM, 500K RPD, 250K TPM ([rate-limits](https://console.groq.com/docs/rate-limits), Developer tab).

Other rules from the same page:
- "Cached tokens do not count towards your rate limits." "Rate limits apply at the organization level", so all of one user's apps share them. Whichever limit is reached first applies.
- Some organizations also have separate input and output per-minute caps (ITPM/OTPM), shown on the Limits page.

**Live check (one request, 2026-11-23 05:33 UTC, `gpt-oss-120b`, 80 tokens):**

```
x-ratelimit-limit-requests: 1000      x-ratelimit-remaining-requests: 999    x-ratelimit-reset-requests: 1m26.4s
x-ratelimit-limit-tokens: 8000        x-ratelimit-remaining-tokens: 7920     x-ratelimit-reset-tokens: 600ms
```

This matches the free table. It also shows that **the limits refill continuously, not at midnight**: 86.4 s is exactly 1/1000 of a day (one request's worth of RPD), and 600 ms is 80 tokens at 8000/60 s. **Inferred:** TPD refills the same way, at 200K/86,400 s ≈ 2.3 tokens/s ≈ 8.3K tokens an hour. Once the daily bucket is drained, a user gets back roughly one full tap per hour.

## 2. Budget math

Assumed sizes (completion includes gpt-oss reasoning tokens; the live call spent 6 of 8 completion tokens on reasoning):

| Call | Input | Output | Total |
|---|---|---|---|
| Key tap today | 3–8K | 0.5–1.5K | 3.5–9.5K |
| Key tap + recall, uncapped | 5–12K | 0.5–1.5K | 5.5–13.5K: **over 8K TPM** |
| Key tap, capped (rec. 2) | ≤ 6K | ≤ 1.5K | ≤ 7.5K |
| People-page refresh | old page 1K + new snippets 3K + prompt 0.5K | 1K | ~5.5K |
| Style-rule batch (5 pairs) | pairs 2K + current rules 1K + prompt 0.5K | 1K | ~4.5K |

**Per minute (8K TPM):** one full tap uses almost the whole minute. A second tap within ~60 s can get a 429 with a short `retry-after`. If background shared the key's model, any background call running when the user taps would cause exactly that. This is the strongest reason to put background on its own model.

**Per day (200K TPD per model):**
- Key alone on 120b: 200K / 7.5K ≈ 26 full taps, or ~40 at 5K. RPD (1,000) never binds, TPD does. Prompt-cache hits stretch this.
- Shared 120b, 75/25: key 150K (~20–30 taps), background 50K (~10 jobs).
- Split models (recommended): key gets all 200K of 120b. Background gets 160K of 20b (80%) ≈ 30 jobs/day, about 20 people pages + 10 style batches (≈ 50 draft→sent pairs).

**Request size check:** Groq lists `413 Request Entity Too Large` ([errors](https://console.groq.com/docs/errors)). **Unverified:** that a single request over TPM is refused outright and never queued. So enforce the 7.5K cap on the client side.

### Scheduler driven by headers

- **Headers are always present** (`retry-after` only on 429). Note that `*-requests` headers mean **RPD** and `*-tokens` headers mean **TPM** ([rate-limits](https://console.groq.com/docs/rate-limits)). **There is no TPD header.**
- So keep a small local ledger per model: a bucket that starts at 200K, refills at ~2.3 tokens/s, and is debited by `usage.total_tokens` after each call (minus cached tokens, if `usage` reports them). Then cross-check it against the headers' remaining values.
- Background job gate: run only if `remaining-tokens` (TPM) ≥ job size and ledger TPD ≥ job size + 20% margin. Otherwise sleep for `reset-tokens` or until the ledger refills. Queue jobs lowest-priority first to drop. People pages for people seen today come before stale ones.
- On 429: sleep `retry-after` seconds, then retry once. For background jobs, double the delay on each retry. The key never queues behind background work. Key fallback: rec. 5.

## 3. Background model choice

`gpt-oss-20b`: same 30/1K/8K/200K limits as 120b, in its own bucket (inferred, rec. 1). It is twice as fast, supports prompt caching, and is Groq's named successor to the 8B model ([deprecations](https://console.groq.com/docs/deprecations)). The jobs are summarising and merging (people page) and extracting rules from diffs (style rules), short structured outputs where 20b should be adequate. Check this with a few real pairs before committing. `qwen/qwen3.8-27b` is a preview model ("may be discontinued at short notice", [models](https://console.groq.com/docs/models)), so don't depend on it. The same split works on Ollama/llama.cpp by setting a separate `background_model`.

## 4. Embeddings vs FTS5

**Groq:** `/openai/v1/embeddings` appears in Groq's API spec with an example model `nomic-embed-text-v1_5` ([api-reference](https://console.groq.com/docs/api-reference)). But no embedding model is listed on the [models page](https://console.groq.com/docs/models) or in the rate-limit tables, and `/docs/embeddings` returns 404. In practice there is no Groq embeddings model. Sending every capture to the cloud to embed would also break local-first.

**What v1.1 retrieval actually asks:**
- People pages: "find captures that mention Priya / @priya / priya@…". That's an exact entity match, which FTS5 handles directly. The page itself is a row looked up by person ID.
- Recall in the key: recent activity in this app/window, plus captures that mention names or phrases in the field. That's a time filter plus lexical match.
- Search window: the user types words they remember seeing. That's lexical, which is how `prototype/recall.py` already works.

**FTS5 options** ([sqlite.org/fts5](https://www.sqlite.org/fts5.html)): `bm25()` ranking with per-column weights, `snippet()`/`highlight()`, external-content tables with triggers. Use `unicode61 remove_diacritics 2` as the main tokenizer. The `trigram` tokenizer "extends FTS5 to support substring matching": it is good for partial names and handles, but queries need at least 3 characters and the index gets larger. Add it as a second, small index on names/titles only if partial matching is needed.

**If embeddings are needed later:**

| Piece | Version / licence | Size |
|---|---|---|
| [fastembed-rs](https://github.com/Anush008/fastembed-rs) (ONNX via `ort`) | v7.8.0 (2026-11-22), Apache-2.0 | plus the ONNX Runtime library |
| [bge-small-en-v1.5](https://huggingface.co/BAAI/bge-small-en-v1.5) (fastembed default, 384-dim) | MIT | 133 MB fp32 ONNX, 66 MB fp16 ([Qdrant Q build](https://huggingface.co/Qdrant/bge-small-en-v1.5-onnx-Q)) |
| [all-MiniLM-L6-v2](https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2) (384-dim) | Apache-2.0 | 90 MB fp32, 23 MB int8 ONNX |
| [sqlite-vec](https://github.com/asg017/sqlite-vec) | v0.1.9 (2026-03-31), pre-v1 | small extension |

Speed is unmeasured here. It needs a quick spike on the target laptop before adoption. The cost is roughly a 25–135 MB model download plus the ONNX runtime, a background embedding job per capture, and a pre-v1 extension to pin. None of that buys anything for name and keyword lookups. Add it when a real query fails on FTS5 ("that article about pricing" when the page says "cost").

## Sources (all read 2026-11-23)

- Groq rate limits (free and Developer tables, headers, 429): https://console.groq.com/docs/rate-limits
- Groq models (production/preview, speeds, context): https://console.groq.com/docs/models
- Groq deprecations (llama-3.1-8b-instant → gpt-oss-20b): https://console.groq.com/docs/deprecations
- Groq prompt caching (cached tokens excluded from limits; gpt-oss supported): https://console.groq.com/docs/prompt-caching
- Groq error codes (413, 429): https://console.groq.com/docs/errors
- Groq API reference (embeddings endpoint in spec): https://console.groq.com/docs/api-reference
- Live response headers: one `gpt-oss-120b` chat call, 80 tokens, 2026-11-23 05:33 UTC
- SQLite FTS5: https://www.sqlite.org/fts5.html
- fastembed-rs: https://github.com/Anush008/fastembed-rs
- sqlite-vec: https://github.com/asg017/sqlite-vec
- Model cards: https://huggingface.co/BAAI/bge-small-en-v1.5, https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2, https://huggingface.co/Qdrant/bge-small-en-v1.5-onnx-Q
