"""recall - personal on-screen memory for Windows (a minimal first prototype).

  python recall.py watch            capture foreground window text every 5s (leave running)
  python recall.py search QUERY     full-text search what you've seen
  python recall.py time [YYYY-MM-DD] minutes per app for a day (default today)
  python recall.py summarize        hourly summaries via Groq (needs GROQ_API_KEY)
  python recall.py mcp              MCP server so Claude Code can query it (needs `pip install mcp`)
  python recall.py assist           tap RIGHT CTRL in any text box: empty -> drafts a reply from what's
                                    on screen; draft -> polishes it; "draft + instruction" -> applies it
                                    (needs GROQ_API_KEY; Ctrl+Z undoes)
  python recall.py test             self-check

Optional: `pip install uiautomation` to capture window *content*, not just titles.
"""
import contextlib, ctypes, ctypes.wintypes as W, hashlib, json, os, queue, sqlite3, sys, threading, time, urllib.request, winsound
from datetime import datetime

LLM_MODEL = os.environ.get("RECALL_MODEL", "openai/gpt-oss-120b")
DB = os.environ.get("RECALL_DB", os.path.expanduser("~/.recall/recall.db"))
INTERVAL, IDLE_S, MAX_CHARS, MAX_NODES = 5, 120, 4000, 600
SKIP_APPS = {"keepass", "keepassxc", "1password", "bitwarden", "lockapp", "credentialuibroker"}


def db(path=DB):
    if path != ":memory:":
        os.makedirs(os.path.dirname(path), exist_ok=True)
    c = sqlite3.connect(path, check_same_thread=False)
    c.executescript("""
      pragma journal_mode=wal;
      create virtual table if not exists snaps using fts5(app, title, content, first_seen unindexed, last_seen unindexed);
      create table if not exists time_ledger(hour text, app text, title text, seconds real, primary key(hour, app, title));
      create table if not exists summaries(hour text primary key, text text);""")
    return c


# ---------- capture (Win32 via ctypes, content via UI Automation) ----------
u32, k32 = ctypes.windll.user32, ctypes.windll.kernel32
u32.GetForegroundWindow.restype = W.HWND
k32.OpenProcess.restype = W.HANDLE
k32.GetTickCount.restype = W.DWORD


def idle_seconds():
    class LII(ctypes.Structure):
        _fields_ = [("cbSize", W.UINT), ("dwTime", W.DWORD)]
    li = LII(ctypes.sizeof(LII))
    u32.GetLastInputInfo(ctypes.byref(li))
    return ((k32.GetTickCount() - li.dwTime) & 0xFFFFFFFF) / 1000


def foreground():
    h = u32.GetForegroundWindow()
    if not h:
        return None
    n = u32.GetWindowTextLengthW(h)
    title = ctypes.create_unicode_buffer(n + 1)
    u32.GetWindowTextW(h, title, n + 1)
    pid = W.DWORD()
    u32.GetWindowThreadProcessId(h, ctypes.byref(pid))
    exe, hp = "", k32.OpenProcess(0x1000, False, pid.value)  # PROCESS_QUERY_LIMITED_INFORMATION
    if hp:
        buf, size = ctypes.create_unicode_buffer(260), W.DWORD(260)
        if k32.QueryFullProcessImageNameW(hp, 0, buf, ctypes.byref(size)):
            exe = os.path.splitext(os.path.basename(buf.value))[0].lower()
        k32.CloseHandle(hp)
    return h, exe, title.value


def window_text(hwnd):
    try:
        import uiautomation as auto
    except ImportError:
        return ""
    out, total = [], 0
    try:
        for i, (ctrl, _) in enumerate(auto.WalkControl(auto.ControlFromHandle(hwnd), maxDepth=25)):
            if i > MAX_NODES or total > MAX_CHARS:
                break
            if getattr(ctrl, "IsPassword", False):  # never store password fields
                continue
            t = ctrl.Name
            if ctrl.ControlTypeName in ("EditControl", "DocumentControl"):
                try:
                    t = ctrl.GetValuePattern().Value or t
                except Exception:
                    pass
            t = (t or "").strip()
            if t and t not in out[-20:]:
                out.append(t)
                total += len(t)
    except Exception:
        pass  # windows vanish mid-walk; keep what we got
    return "\n".join(out)[:MAX_CHARS]


def record(c, app, title, content, now, last, seconds):
    """Extend the current row if nothing changed, else start a new one. Returns (key, rowid)."""
    key = hashlib.sha1(f"{app}\0{title}\0{content}".encode()).digest()
    if key == last[0]:
        c.execute("update snaps set last_seen=? where rowid=?", (now, last[1]))
    else:
        # ponytail: one row per distinct screen state; add near-duplicate merging if the DB grows too fast
        rowid = c.execute("insert into snaps(app,title,content,first_seen,last_seen) values(?,?,?,?,?)",
                          (app, title, content, now, now)).lastrowid
        last = (key, rowid)
    hour = datetime.fromtimestamp(now).strftime("%Y-%m-%dT%H")
    c.execute("insert into time_ledger values(?,?,?,?) on conflict do update set seconds=seconds+excluded.seconds",
              (hour, app, title, seconds))
    c.commit()
    return last


def watch(c):
    print(f"recording to {DB} (Ctrl+C to stop)")
    last, prev = (None, None), time.time()
    while True:
        now = time.time()
        dt, prev = min(now - prev, INTERVAL * 3), now
        fg = foreground()
        if fg and fg[1] not in SKIP_APPS and idle_seconds() < IDLE_S:
            last = record(c, fg[1], fg[2], window_text(fg[0]), now, last, dt)
        else:
            last = (None, None)
        time.sleep(INTERVAL)


# ---------- queries ----------
def search(c, q, limit=20):
    q = " ".join('"%s"' % w.replace('"', '""') for w in q.split())  # literal terms, no FTS syntax errors
    return c.execute("""select app, title, datetime(first_seen,'unixepoch','localtime'),
                        snippet(snaps, 2, '[', ']', '...', 24) from snaps where snaps match ?
                        order by rank limit ?""", (q, limit)).fetchall()


def time_spent(c, day=None):
    day = day or datetime.now().strftime("%Y-%m-%d")
    return c.execute("""select app, round(sum(seconds)/60, 1) from time_ledger where hour like ? || '%'
                        group by app order by 2 desc""", (day,)).fetchall()


def recent(c, minutes=60):
    return c.execute("""select app, title, datetime(first_seen,'unixepoch','localtime'), substr(content,1,500)
                        from snaps where last_seen > ? order by first_seen desc limit 50""",
                     (time.time() - minutes * 60,)).fetchall()


def llm(prompt):
    """One chat completion on Groq (stdlib only). Key comes from GROQ_API_KEY."""
    key = os.environ.get("GROQ_API_KEY")
    if not key:  # shells started before `setx` don't see it; read the saved user variable directly
        import winreg
        with contextlib.suppress(OSError), winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as k:
            key = winreg.QueryValueEx(k, "GROQ_API_KEY")[0]
    if not key:
        raise RuntimeError("set GROQ_API_KEY")
    req = urllib.request.Request(
        "https://api.groq.com/openai/v1/chat/completions",
        json.dumps({"model": LLM_MODEL, "reasoning_effort": "low",
                    "messages": [{"role": "user", "content": prompt}]}).encode(),
        {"Authorization": f"Bearer {key}", "Content-Type": "application/json", "User-Agent": "recall"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)["choices"][0]["message"]["content"].strip()


def summarize(c):
    this_hour = datetime.now().strftime("%Y-%m-%dT%H")
    hours = [h for (h,) in c.execute("""select distinct hour from time_ledger
             where hour < ? and hour not in (select hour from summaries) order by hour""", (this_hour,))]
    for h in hours:
        start = datetime.strptime(h, "%Y-%m-%dT%H").timestamp()
        rows = c.execute("select app, title, content from snaps where first_seen >= ? and first_seen < ?",
                         (start, start + 3600)).fetchall()
        if not rows:
            continue
        log = "\n\n".join(f"[{a}] {t}\n{(x or '')[:400]}" for a, t, x in rows)
        text = llm(f"Below is what was on my screen during {h}:00. Write 3-6 bullets on what I worked on, "
                   f"naming projects, people and documents. No preamble.\n\n{log}")
        c.execute("insert into summaries values(?,?)", (h, text))
        c.commit()
        print(f"## {h}\n{text}\n")


# ---------- MCP ----------
def serve(c):
    from mcp.server.fastmcp import FastMCP
    m = FastMCP("recall")

    @m.tool()
    def search_screen(query: str, limit: int = 20) -> list:
        """Full-text search everything seen on screen. Returns (app, title, time, snippet)."""
        return search(c, query, limit)

    @m.tool()
    def time_by_app(day: str = "") -> list:
        """Minutes per app for a day (YYYY-MM-DD, default today)."""
        return time_spent(c, day or None)

    @m.tool()
    def recent_activity(minutes: int = 60) -> list:
        """Screen states from the last N minutes, newest first."""
        return recent(c, minutes)

    @m.tool()
    def hourly_summaries(day: str = "") -> list:
        """AI summaries per hour for a day (YYYY-MM-DD, default today). Run `recall.py summarize` to fill."""
        return c.execute("select hour, text from summaries where hour like ? || '%' order by hour",
                         (day or datetime.now().strftime("%Y-%m-%d"),)).fetchall()

    m.run()


# ---------- assist: right-Shift autofill ----------
HOTKEY, VK_CTRL = 0xA3, 0x11  # 0xA3 = Right Ctrl 
u32.GetClipboardData.restype = W.HANDLE
u32.SetClipboardData.argtypes = [W.UINT, W.HANDLE]
k32.GlobalAlloc.restype = W.HGLOBAL
k32.GlobalLock.argtypes, k32.GlobalLock.restype = [W.HGLOBAL], ctypes.c_void_p
k32.GlobalUnlock.argtypes = [W.HGLOBAL]
HOOKPROC = ctypes.WINFUNCTYPE(ctypes.c_ssize_t, ctypes.c_int, W.WPARAM, W.LPARAM)
u32.SetWindowsHookExW.argtypes = [ctypes.c_int, HOOKPROC, W.HINSTANCE, W.DWORD]
u32.SetWindowsHookExW.restype = W.HHOOK
u32.CallNextHookEx.argtypes = [W.HHOOK, ctypes.c_int, W.WPARAM, W.LPARAM]
u32.CallNextHookEx.restype = ctypes.c_ssize_t


class KBD(ctypes.Structure):
    _fields_ = [("vk", W.DWORD), ("scan", W.DWORD), ("flags", W.DWORD), ("time", W.DWORD), ("extra", ctypes.c_size_t)]


def shift_tap(st, vk, down, t):
    """True when the hotkey is pressed and released alone within 0.5s (so Ctrl+C etc. never fire)."""
    if vk != HOTKEY:
        if down:
            st["clean"] = False
        return False
    if down:
        if st.get("t") is None:  # ignore auto-repeat
            st.update(t=t, clean=True)
        return False
    fired = st.get("clean") and st.get("t") is not None and t - st["t"] < 0.5
    st["t"] = None
    return bool(fired)


def keys(*vks):
    for v in vks:
        u32.keybd_event(v, 0, 0, 0)
    for v in reversed(vks):
        u32.keybd_event(v, 0, 2, 0)


def clip_get():
    for _ in range(10):
        if u32.OpenClipboard(None):
            try:
                h = u32.GetClipboardData(13)  # CF_UNICODETEXT
                if not h:
                    return ""
                s = ctypes.wstring_at(k32.GlobalLock(h))
                k32.GlobalUnlock(h)
                return s
            finally:
                u32.CloseClipboard()
        time.sleep(0.02)
    return None


def clip_set(s):
    data = (s + "\0").encode("utf-16-le")
    h = k32.GlobalAlloc(0x0002, len(data))  # GMEM_MOVEABLE
    ctypes.memmove(k32.GlobalLock(h), data, len(data))
    k32.GlobalUnlock(h)
    for _ in range(10):
        if u32.OpenClipboard(None):
            u32.EmptyClipboard()
            u32.SetClipboardData(13, h)
            u32.CloseClipboard()
            return
        time.sleep(0.02)


def uia_thread():
    try:
        import uiautomation as auto
        return auto.UIAutomationInitializerInThread()
    except ImportError:
        return contextlib.nullcontext()


def field_ok():
    """Focused control is an editable, non-password text field (assumed yes without uiautomation)."""
    try:
        import uiautomation as auto
    except ImportError:
        return True
    f = auto.GetFocusedControl()
    return bool(f) and f.ControlTypeName in ("EditControl", "DocumentControl") and not f.IsPassword


def compose(app, title, screen, draft, history):
    return llm(f"""I'm typing in a text box in {app} ("{title}").

What's on my screen right now:
<screen>{screen}</screen>

What I was looking at in the last 30 minutes:
<recent>{history}</recent>

Current text box content:
<draft>{draft}</draft>

- If the draft is empty: write what I most likely want to type here (e.g. a reply to the message on screen).
- If the draft ends with an instruction to you (like "make it shorter" or "say yes politely"): apply it and drop the instruction.
- Otherwise: fix and polish the draft, keeping my voice, language and meaning.
Return only the final text for the box. No quotes, no preamble.""")


def fill(c):
    fg = foreground()
    if not fg or fg[1] in SKIP_APPS or not field_ok():
        return
    old, seq = clip_get(), u32.GetClipboardSequenceNumber()
    keys(VK_CTRL, 0x41)  # Ctrl+A
    keys(VK_CTRL, 0x43)  # Ctrl+C
    draft = ""
    for _ in range(20):
        time.sleep(0.03)
        if u32.GetClipboardSequenceNumber() != seq:
            draft = clip_get() or ""
            break
    winsound.MessageBeep()  # "working on it"
    history = "\n".join(f"[{a}] {t}: {(x or '')[:200]}" for a, t, _, x in recent(c, 30)[:15])
    try:
        out = compose(fg[1], fg[2], window_text(fg[0]), draft, history)
    except Exception as e:
        print("assist failed:", e, flush=True)
        out = None
    if not out:
        winsound.MessageBeep(0x10)
    elif u32.GetForegroundWindow() != fg[0]:
        clip_set(out)  # you switched windows meanwhile: leave it on the clipboard instead of pasting blindly
        winsound.MessageBeep(0x40)
        return
    else:
        clip_set(out)
        keys(VK_CTRL, 0x41)
        keys(VK_CTRL, 0x56)  # Ctrl+V
        time.sleep(0.3)
    if old is not None:
        clip_set(old)


def worker(c, jobs):
    """One long-lived thread: UI Automation must be initialised once per thread, never torn down
    between taps (uiautomation caches its COM object; re-initialising per tap crashes on tap 2)."""
    with uia_thread():
        while True:
            jobs.get()
            try:
                fill(c)
            except Exception as e:
                print("assist failed:", e, flush=True)
            while not jobs.empty():  # drop taps made while we were busy
                jobs.get_nowait()


def assist(c):
    st, jobs = {}, queue.Queue()
    threading.Thread(target=worker, args=(c, jobs), daemon=True).start()

    @HOOKPROC
    def proc(n, wp, lp):
        k = ctypes.cast(lp, ctypes.POINTER(KBD)).contents
        if n == 0 and not k.flags & 0x10:  # skip our own injected keys
            down = wp in (0x100, 0x104)
            if shift_tap(st, k.vk, down, k.time / 1000):
                jobs.put(None)
        return u32.CallNextHookEx(None, n, wp, lp)

    hook = u32.SetWindowsHookExW(13, proc, None, 0)  # WH_KEYBOARD_LL
    if not hook:
        sys.exit("could not install keyboard hook")
    print("assist ready: tap RIGHT CTRL in a text box (Ctrl+C here to quit)")
    msg = W.MSG()
    while u32.GetMessageW(ctypes.byref(msg), None, 0, 0) > 0:
        u32.TranslateMessage(ctypes.byref(msg))
        u32.DispatchMessageW(ctypes.byref(msg))


def test():
    st = {}
    assert not shift_tap(st, HOTKEY, True, 0) and shift_tap(st, HOTKEY, False, 0.1)  # tap fires
    shift_tap(st, HOTKEY, True, 1); shift_tap(st, 0x41, True, 1.05)
    assert not shift_tap(st, HOTKEY, False, 1.1)  # Ctrl+A shortcut never fires
    shift_tap(st, HOTKEY, True, 2); shift_tap(st, HOTKEY, True, 2.3)
    assert not shift_tap(st, HOTKEY, False, 3)  # long hold (e.g. ctrl-click) never fires
    assert not shift_tap(st, HOTKEY, False, 4)  # stray key-up never fires
    c = db(":memory:")
    last = record(c, "code", "a.py", "hello world", 1000, (None, None), 5)
    assert record(c, "code", "a.py", "hello world", 1005, last, 5) == last  # unchanged -> same row
    assert c.execute("select count(*) from snaps").fetchone()[0] == 1
    record(c, "chrome", "docs", "goodbye-moon \"quoted\"", 1010, last, 5)
    assert search(c, 'goodbye-moon "quoted')[0][0] == "chrome"  # punctuation must not break FTS
    assert c.execute("select sum(seconds) from time_ledger").fetchone()[0] == 15
    assert foreground() is None or isinstance(foreground()[1], str)
    print("ok")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    cmd, args = (sys.argv[1] if len(sys.argv) > 1 else "help"), sys.argv[2:]
    if cmd == "test":
        test()
    elif cmd == "watch":
        watch(db())
    elif cmd == "search":
        for r in search(db(), " ".join(args)):
            print(f"{r[2]}  [{r[0]}] {r[1]}\n    {r[3]}")
    elif cmd == "time":
        for app, mins in time_spent(db(), args[0] if args else None):
            print(f"{mins:>7} min  {app}")
    elif cmd == "summarize":
        summarize(db())
    elif cmd == "mcp":
        serve(db())
    elif cmd == "assist":
        assist(db())
    else:
        print(__doc__)
