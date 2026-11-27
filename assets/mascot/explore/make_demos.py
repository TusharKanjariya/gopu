"""Writes palette + pose demo sheets (SVG) for the Gopu mascot exploration. Throwaway."""
from pathlib import Path

OUT = Path(__file__).parent
FONT = "Nunito, Segoe UI, sans-serif"

PALETTES = {
    "Espresso + Honey": dict(base="#17120F", surface="#241C17", border="#3A2E25", text="#F4EADB", muted="#A8957F",
                             mascot="#E9A94B", dk="#B97A2A", outline="#2A1D14", accent="#F2C46D", cheek="#E8846F"),
    "Graphite + Amber": dict(base="#131416", surface="#1E2024", border="#2E3137", text="#ECEDEF", muted="#8B9099",
                             mascot="#E6A23C", dk="#B5791F", outline="#1A1410", accent="#FFC766", cheek="#E58A72"),
    "Deep Teal + Gold": dict(base="#0E1C1F", surface="#16292D", border="#24403F", text="#E8F1EE", muted="#8FA8A3",
                             mascot="#F0B04A", dk="#C0832A", outline="#1B1610", accent="#F7CF7A", cheek="#EE8E78"),
    "Plum Night + Saffron": dict(base="#16121B", surface="#221B29", border="#352B3F", text="#F1ECF4", muted="#9C90A6",
                                 mascot="#EFA43F", dk="#BC7621", outline="#1D1410", accent="#FFCB6B", cheek="#EC8A7C"),
}


def tube(d, p, w=17, inner=10):
    return (f'<path d="{d}" fill="none" stroke="{p["outline"]}" stroke-width="{w}" stroke-linecap="round"/>'
            f'<path d="{d}" fill="none" stroke="{p["mascot"]}" stroke-width="{inner}" stroke-linecap="round"/>')


def face(p, eye, cheek, smile, r=4.5):
    ex, ey = eye
    return (f'<circle cx="{ex}" cy="{ey}" r="{r}" fill="{p["outline"]}"/>'
            f'<circle cx="{ex - 1.5}" cy="{ey - 1.6}" r="1.6" fill="#fff"/>'
            f'<ellipse cx="{cheek[0]}" cy="{cheek[1]}" rx="7" ry="4.5" fill="{p["cheek"]}" opacity=".75"/>'
            f'<path d="{smile}" fill="none" stroke="{p["outline"]}" stroke-width="2.5" stroke-linecap="round"/>')


def ear(p, cx, cy, rx, ry, rot=-10):
    o = p["outline"]
    return (f'<g transform="rotate({rot} {cx} {cy})"><ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" fill="{p["mascot"]}" stroke="{o}" stroke-width="4"/>'
            f'<ellipse cx="{cx - 2}" cy="{cy + 2}" rx="{rx * .6}" ry="{ry * .62}" fill="{p["cheek"]}" opacity=".5"/></g>')


def leg(p, x, y, h=30, fill=None):
    return (f'<rect x="{x}" y="{y}" width="20" height="{h}" rx="9" fill="{fill or p["mascot"]}" stroke="{p["outline"]}" stroke-width="4"/>'
            f'<path d="M{x + 5} {y + h - 6}h10" stroke="{p["accent"]}" stroke-width="2.5" stroke-linecap="round" opacity=".8"/>')


def side(p):
    o, m = p["outline"], p["mascot"]
    return (leg(p, 64, 108, 32, p["dk"]) + leg(p, 116, 108, 32, p["dk"])
            + f'<path d="M38 92q-12 6-8 20" fill="none" stroke="{o}" stroke-width="3.5" stroke-linecap="round"/>'
            + f'<ellipse cx="88" cy="96" rx="52" ry="38" fill="{m}" stroke="{o}" stroke-width="4"/>'
            + leg(p, 48, 112) + leg(p, 100, 112)
            + f'<circle cx="136" cy="70" r="36" fill="{m}" stroke="{o}" stroke-width="4"/>'
            + tube("M164 80C182 86 194 72 192 56C191 47 185 43 180 47", p)
            + f'<ellipse cx="163" cy="79" rx="6" ry="7" fill="{m}"/>'
            + ear(p, 122, 74, 19, 24)
            + face(p, (152, 62), (156, 84), "M155 94q5 4 10 0"))


def blob(p):
    o, m = p["outline"], p["mascot"]
    return (''.join(leg(p, x, 118, 22, p["dk"] if i % 2 else None) for i, x in enumerate((60, 76, 108, 124)))
            + f'<path d="M44 92q-12 4-9 16" fill="none" stroke="{o}" stroke-width="3.5" stroke-linecap="round"/>'
            + f'<ellipse cx="100" cy="90" rx="58" ry="46" fill="{m}" stroke="{o}" stroke-width="4"/>'
            + tube("M150 90C168 94 178 80 174 66C172 60 167 59 163 62", p)
            + f'<ellipse cx="150" cy="89" rx="6" ry="7" fill="{m}"/>'
            + ear(p, 100, 80, 18, 22)
            + face(p, (128, 74), (134, 92), "M134 102q5 4 10 0"))


def three_q(p):
    o, m = p["outline"], p["mascot"]
    return (ear(p, 140, 52, 13, 19, 20)
            + f'<ellipse cx="96" cy="124" rx="42" ry="24" fill="{m}" stroke="{o}" stroke-width="4"/>'
            + leg(p, 72, 124, 24) + leg(p, 102, 124, 24)
            + ear(p, 56, 72, 24, 30, -15)
            + f'<circle cx="100" cy="72" r="40" fill="{m}" stroke="{o}" stroke-width="4"/>'
            + tube("M118 84C122 102 142 106 150 94C154 88 152 81 146 81", p)
            + f'<ellipse cx="118" cy="83" rx="6" ry="7" fill="{m}"/>'
            + face(p, (96, 66), (82, 86), "M98 92q5 4 9 0")
            + f'<circle cx="126" cy="64" r="3.8" fill="{o}"/><circle cx="124.8" cy="62.6" r="1.3" fill="#fff"/>'
            + f'<ellipse cx="138" cy="82" rx="4" ry="3.5" fill="{p["cheek"]}" opacity=".75"/>')


def palette_card(name, p, x, y):
    sw = ''.join(f'<circle cx="{24 + i * 22}" cy="272" r="8" fill="{p[k]}" stroke="{p["border"]}" stroke-width="1.5"/>'
                 for i, k in enumerate(("base", "surface", "text", "mascot", "dk", "accent", "cheek")))
    return (f'<g transform="translate({x} {y})"><rect width="400" height="300" fill="{p["base"]}"/>'
            f'<text x="24" y="44" font-family="{FONT}" font-size="28" font-weight="800" fill="{p["text"]}">gopu<tspan fill="{p["accent"]}">.</tspan></text>'
            f'<text x="376" y="40" text-anchor="end" font-family="{FONT}" font-size="13" fill="{p["muted"]}">{name}</text>'
            f'<g transform="translate(110 46) scale(1.05)">{side(p)}</g>'
            f'<rect x="24" y="206" width="352" height="44" rx="10" fill="{p["surface"]}" stroke="{p["border"]}" stroke-width="1.5"/>'
            f'<text x="40" y="233" font-family="{FONT}" font-size="14" fill="{p["muted"]}">Reply to Maya…</text>'
            f'<rect x="268" y="216" width="96" height="24" rx="12" fill="{p["accent"]}"/>'
            f'<text x="316" y="233" text-anchor="middle" font-family="{FONT}" font-size="12" font-weight="700" fill="{p["base"]}">Gopu key</text>'
            f'{sw}</g>')


def pose_card(label, fn, p, x):
    return (f'<g transform="translate({x} 0)"><rect width="400" height="320" fill="{p["base"]}"/>'
            f'<text x="24" y="40" font-family="{FONT}" font-size="18" font-weight="700" fill="{p["text"]}">{label}</text>'
            f'<g transform="translate(60 50) scale(1.4)">{fn(p)}</g>'
            f'<rect x="300" y="250" width="48" height="48" rx="8" fill="{p["surface"]}"/>'
            f'<g transform="translate(308 262) scale(.16)">{fn(p)}</g>'
            f'<rect x="356" y="266" width="24" height="24" rx="5" fill="{p["surface"]}"/>'
            f'<g transform="translate(360 272) scale(.08)">{fn(p)}</g>'
            f'<text x="24" y="290" font-family="{FONT}" font-size="12" fill="{p["muted"]}">icon preview: 32 px · 16 px →</text></g>')


def sheet(w, h, body):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">{body}</svg>\n'


cards = ''.join(palette_card(n, p, (i % 2) * 400, (i // 2) * 300) for i, (n, p) in enumerate(PALETTES.items()))
(OUT / "demo-palettes.svg").write_text(sheet(800, 600, cards), encoding="utf-8")

plum = PALETTES["Plum Night + Saffron"]
poses = ''.join(pose_card(l, f, plum, i * 400) for i, (l, f) in
                enumerate((("A · Side, trunk raised", side), ("B · Three-quarter", three_q), ("C · Round blob", blob))))
(OUT / "demo-poses.svg").write_text(sheet(1200, 320, poses), encoding="utf-8")


# --- mascot colour options (the elephant itself), shown on light + dark host apps ---
MASCOTS = {
    "A · Marigold": dict(mascot="#F5A524", dk="#C77D0A", outline="#2E1C0C", accent="#FFE2A3", cheek="#F07C6C"),
    "B · Burnt Saffron": dict(mascot="#E8891C", dk="#B2600B", outline="#2B170A", accent="#FFD29A", cheek="#F06A5E"),
    "C · Honey Bronze": dict(mascot="#C98A35", dk="#966119", outline="#24170C", accent="#F3D7A8", cheek="#E27A68"),
    "D · Coral Peach": dict(mascot="#F28A6B", dk="#C9603F", outline="#2E1610", accent="#FFD9C9", cheek="#E0566A"),
}


def head(p):
    """Side-view head only: the caret-sized buddy (box ~70x60)."""
    o, m = p["outline"], p["mascot"]
    return (f'<circle cx="30" cy="32" r="24" fill="{m}" stroke="{o}" stroke-width="4"/>'
            + tube("M48 38C60 42 68 34 66 22C65 16 61 14 58 17", p, 14, 7)
            + f'<ellipse cx="47" cy="37" rx="5" ry="6" fill="{m}"/>'
            + ear(p, 20, 34, 13, 17)
            + f'<circle cx="40" cy="26" r="3.6" fill="{o}"/><circle cx="39" cy="24.8" r="1.2" fill="#fff"/>'
            + f'<ellipse cx="43" cy="40" rx="4.5" ry="3" fill="{p["cheek"]}" opacity=".8"/>')


def host(x, y, dark, p, text):
    bg, box, bd, tx = (("#1E1F22", "#2B2D31", "#3F4147", "#DBDEE1") if dark else ("#FFFFFF", "#FFFFFF", "#2F6FDE", "#1A1A1A"))
    return (f'<g transform="translate({x} {y})"><rect width="200" height="90" fill="{bg}"/>'
            f'<rect x="34" y="30" width="150" height="32" rx="6" fill="{box}" stroke="{bd}" stroke-width="1.5"/>'
            f'<text x="44" y="51" font-family="{FONT}" font-size="12" fill="{tx}">{text}</text>'
            f'<g transform="translate(8 34) scale(.36)">{head(p)}</g></g>')


def mascot_card(name, p, x):
    hx = ''.join(f'<rect x="{12 + i * 30}" y="372" width="24" height="16" rx="3" fill="{p[k]}"/>'
                 for i, k in enumerate(("mascot", "dk", "outline", "accent", "cheek")))
    return (f'<g transform="translate({x} 0)"><rect width="400" height="400" fill="#F4F1EC"/>'
            f'<text x="16" y="30" font-family="{FONT}" font-size="18" font-weight="700" fill="#222">{name}</text>'
            f'<text x="16" y="50" font-family="{FONT}" font-size="12" fill="#666">{p["mascot"]}</text>'
            f'<g transform="translate(20 60) scale(1.1)">{side(p)}</g>'
            f'<g transform="translate(280 140) scale(1.4)">{head(p)}</g>'
            + host(0, 270, False, p, "Alex") + host(200, 270, True, p, "Message @Maya")
            + f'{hx}</g>')


(OUT / "demo-mascot-colors.svg").write_text(
    sheet(1600, 400, ''.join(mascot_card(n, p, i * 400) for i, (n, p) in enumerate(MASCOTS.items()))), encoding="utf-8")
