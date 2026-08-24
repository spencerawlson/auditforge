import re, sys, os
SP = os.environ["SP"]
afm_dir = os.path.join(SP, "afm")

# glyph name -> unicode (AGL)
name2uni = {}
with open(os.path.join(afm_dir, "glyphlist.txt"), encoding="utf-8") as fh:
    for line in fh:
        if line.startswith("#"):
            continue
        parts = line.strip().split(";")
        if len(parts) < 2:
            continue
        codes = parts[1].split()
        if len(codes) != 1:
            continue
        name2uni[parts[0]] = int(codes[0], 16)

def afm_widths(path):
    """glyph-name -> width for one AFM file"""
    out = {}
    with open(path, encoding="latin-1") as fh:
        for line in fh:
            if not line.startswith("C "):
                continue
            m_w = re.search(r"WX\s+(-?\d+)", line)
            m_n = re.search(r"N\s+([A-Za-z0-9._]+)", line)
            if m_w and m_n:
                out[m_n.group(1)] = int(m_w.group(1))
    return out

# WinAnsi (CP1252) byte -> unicode -> glyph name -> width
uni2name = {}
for n, u in name2uni.items():
    uni2name.setdefault(u, n)

def table_for(path, default_width):
    gw = afm_widths(path)
    # unicode -> width
    uw = {}
    for n, w in gw.items():
        u = name2uni.get(n)
        if u is not None:
            uw.setdefault(u, w)
    tbl = []
    for b in range(256):
        try:
            ch = bytes([b]).decode("cp1252")
        except UnicodeDecodeError:
            tbl.append(0)
            continue
        w = uw.get(ord(ch))
        tbl.append(w if w is not None else 0)
    # sanity
    if tbl[ord('A')] == 0 or tbl[ord(' ')] == 0:
        raise SystemExit("metric extraction failed for " + path)
    return tbl

fonts = [
    ("HELVETICA", "phvr8a.afm"),
    ("HELVETICA_BOLD", "phvb8a.afm"),
    ("HELVETICA_OBLIQUE", "phvro8a.afm"),
    ("HELVETICA_BOLD_OBLIQUE", "phvbo8a.afm"),
    ("COURIER", "pcrr8a.afm"),
]

lines = []
lines.append("//! Glyph advance-width tables for the PDF base-14 fonts, in 1/1000 em units.")
lines.append("//!")
lines.append("//! Generated from the Adobe Font Metrics (AFM) files for the base-14 fonts and")
lines.append("//! indexed by WinAnsiEncoding (CP1252) byte value, which is the encoding the PDF")
lines.append("//! renderer uses for all built-in font text. A width of `0` means the byte has no")
lines.append("//! glyph in WinAnsiEncoding; callers must substitute such bytes before measuring.")
lines.append("//!")
lines.append("//! Do not edit by hand - see `scripts/generate_font_metrics.py`.")
lines.append("")
for const, fn in fonts:
    tbl = table_for(os.path.join(afm_dir, fn), 0)
    lines.append("/// WinAnsi advance widths for %s." % const.replace("_", " ").title())
    lines.append("pub const %s: [u16; 256] = [" % const)
    for i in range(0, 256, 16):
        lines.append("    " + " ".join("%d," % w for w in tbl[i:i+16]))
    lines.append("];")
    lines.append("")

out = "\n".join(lines)
dest = sys.argv[1]
with open(dest, "w", encoding="utf-8", newline="\n") as fh:
    fh.write(out)
print("wrote", dest, len(out), "bytes")
