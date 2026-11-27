#!/usr/bin/env python3
"""What this build prints differently from a previous ferroreport (a release, or any binary).

    python3 tools/compat.py --old 1.3.0                  # against the 1.3.0 release
    python3 tools/compat.py --old path/to/ferroreport    # against another build
    python3 tools/compat.py --old 1.3.0 --new path/to/ferroreport --xlsx --strict

Within 1.x the output may only move closer to what JasperReports prints (Stability in the
README). tools/compare.py checks against JasperReports itself; this checks against the previous
release, so a reviewer sees what a pull request, or a release, changes.

Every fixture in fixtures/ and every template in examples/ is rendered by both binaries, with
the options of tools/compare.py (--now, rows, parameters, fonts.txt, fontsets.txt, resources),
to PDF and CSV (and XLSX with --xlsx). Then:

  - the page count;
  - every page's pixels, as tools/compare.py compares them (100 DPI; a page differs when over
    0.15% of its ink does);
  - the CSV text, exactly;
  - with --xlsx, every part of the workbook except docProps/ (which names the version).

The PDF bytes are not compared: the producer string names the version.

A version for --old (1.3.0 or v1.3.0) is downloaded from GitHub Releases, the archive for this
platform (ferroreport-linux-x86_64.tar.gz on CI), and checked against the release's SHA256SUMS
before it is unpacked; it is kept in target/compat-releases/.

Writes target/compat/report.md (one row per template), target/compat/results.json, and a diff
picture of every page that changed (red: only in the old output, blue: only in the new one).
Exits with 1 when a template could not be compared because a binary is missing or a download
fails; with --strict, also when anything changed.
"""
import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compare import MATCH_THRESHOLD, NOW, diff_page, fixture_args, load, rasterize  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
REPO = "cristianEncarnacion/ferroreport"
VERSION = re.compile(r"^v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)$")
TIMEOUT = 300  # seconds for one render


# --- The binaries ---------------------------------------------------------------------------

def release_asset() -> str:
    """The name of the release archive that runs here."""
    system, machine = platform.system(), platform.machine().lower()
    arch = {"x86_64": "x86_64", "amd64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(machine)
    if system == "Linux" and arch:
        return f"ferroreport-linux-{arch}.tar.gz"
    if system == "Darwin" and arch:
        return f"ferroreport-macos-{arch}.tar.gz"
    sys.exit(f"compat: no release archive for {system} {machine}; pass --old the path of a binary")


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "ferroreport-compat"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return r.read()


def download(version: str, cache: Path) -> Path:
    """The `ferroreport` binary of a release, downloaded once and checked against SHA256SUMS."""
    asset = release_asset()
    folder = cache / version
    binary = folder / asset.removesuffix(".tar.gz") / "ferroreport"
    if binary.exists():
        return binary
    base = f"https://github.com/{REPO}/releases/download/v{version}"
    print(f"Downloading {asset} of {version}...", file=sys.stderr)
    try:
        sums = fetch(f"{base}/SHA256SUMS").decode()
        archive = fetch(f"{base}/{asset}")
    except OSError as e:
        sys.exit(f"compat: cannot download release {version} from {base}: {e}")
    expected = next((line.split()[0] for line in sums.splitlines() if line.split()[-1:] == [asset]), None)
    if expected is None:
        sys.exit(f"compat: {asset} is not listed in the SHA256SUMS of {version}")
    actual = hashlib.sha256(archive).hexdigest()
    if actual != expected:
        sys.exit(f"compat: {asset} of {version} does not match SHA256SUMS ({actual}, expected {expected})")
    folder.mkdir(parents=True, exist_ok=True)
    tgz = folder / asset
    tgz.write_bytes(archive)
    # Only the binary is taken out, by its known name: nothing else in the archive is written.
    member = f"{asset.removesuffix('.tar.gz')}/ferroreport"
    with tarfile.open(tgz) as tar:
        info = tar.getmember(member)
        if not info.isfile():
            sys.exit(f"compat: {member} in {asset} is not a file")
        binary.parent.mkdir(parents=True, exist_ok=True)
        with tar.extractfile(info) as src, open(binary, "wb") as dst:
            shutil.copyfileobj(src, dst)
    binary.chmod(0o755)
    return binary


def resolve(spec: str, cache: Path) -> tuple:
    """(binary, label) for --old/--new: a path, or a release version."""
    path = Path(spec)
    if path.exists():
        return path.resolve(), spec
    m = VERSION.match(spec)
    if m:
        return download(m.group(1), cache), m.group(1)
    sys.exit(f"compat: {spec} is neither a ferroreport binary nor a version such as 1.3.0")


def version_of(binary: Path) -> str:
    try:
        p = subprocess.run([str(binary), "--version"], capture_output=True, text=True, timeout=30)
    except OSError as e:
        sys.exit(f"compat: cannot run {binary}: {e}")
    return (p.stdout or p.stderr).strip()


# --- The templates --------------------------------------------------------------------------

def flat_rows(folder: Path, stem: str):
    """The rows of a flat example: <name>.json, or else those of the example its name starts
    with (invoices-short.ferro takes invoices.json), as crates/ferroreport/tests/examples.rs."""
    while True:
        if (folder / f"{stem}.json").exists():
            return folder / f"{stem}.json"
        if "-" not in stem:
            return None
        stem = stem[: stem.rfind("-")]


def is_report(ferro: Path) -> bool:
    """False for a .ferro file of styles and parts for others to import."""
    for line in ferro.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("//"):
            return not (line.startswith("styles") or line.startswith("jasperTemplate"))
    return True


def templates() -> list:
    """(name, template, function(out) -> options) for every fixture and example."""
    found = []
    for fx in sorted((ROOT / "fixtures").iterdir()):
        if (fx / "report.jrxml").exists():
            found.append((f"fixtures/{fx.name}", fx / "report.jrxml", lambda out, fx=fx: fixture_args(fx, out)))

    def options(rows, params):
        return lambda out: (["-d", str(rows)] if rows else []) + (["-p", str(params)] if params else [])

    for path in sorted((ROOT / "examples").iterdir()):
        if path.is_dir():
            template = next((path / f for f in ("report.jrxml", "report.ferro") if (path / f).exists()), None)
            if template:
                rows, params = path / "data.json", path / "params.json"
                found.append((f"examples/{path.name}", template, options(rows if rows.exists() else None, params if params.exists() else None)))
        elif path.suffix == ".ferro" and is_report(path):
            found.append((f"examples/{path.name}", path, options(flat_rows(path.parent, path.stem), None)))
    return found


# --- Comparing ------------------------------------------------------------------------------

def render(binary: Path, template: Path, output: Path, options: list):
    """None when it rendered, else what went wrong."""
    cmd = [str(binary), "render", str(template), "-o", str(output), "-q", "--now", NOW] + options
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=TIMEOUT)
    except subprocess.TimeoutExpired:
        return f"timed out after {TIMEOUT} s"
    if p.returncode != 0 or not output.exists():
        lines = (p.stderr or p.stdout).strip().splitlines()
        return lines[-1] if lines else f"exit {p.returncode}"
    return None


def compare_pages(old: Path, new: Path, out: Path, dpi: int) -> dict:
    """Pages and pixels of two PDFs; a diff picture in `out` for every page that differs."""
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        pa, pb = rasterize(old, dpi, tmp / "old"), rasterize(new, dpi, tmp / "new")
        shares = []
        for i in range(max(len(pa), len(pb))):
            a = load(pa[i]) if i < len(pa) else None
            b = load(pb[i]) if i < len(pb) else None
            # A page only one of them has is compared with a blank page of its size.
            a = a if a is not None else np.full(b.shape, 255, np.int16)
            b = b if b is not None else np.full(a.shape, 255, np.int16)
            picture = tmp / f"diff-{i + 1}.png"
            share = diff_page(a, b, picture)
            if share > MATCH_THRESHOLD or i >= min(len(pa), len(pb)):
                shutil.move(picture, out / picture.name)
            shares.append(share)
    worst = max(range(len(shares)), key=shares.__getitem__) if shares else None
    return {
        "pages_old": len(pa),
        "pages_new": len(pb),
        "pages_differing": sum(s > MATCH_THRESHOLD for s in shares),
        "worst_page": worst + 1 if worst is not None else None,
        "worst_page_diff": round(shares[worst], 5) if worst is not None else 0.0,
    }


def first_difference(old: str, new: str):
    """(line, old line, new line) of the first line that differs, or None."""
    a, b = old.splitlines(), new.splitlines()
    for i in range(max(len(a), len(b))):
        la = a[i] if i < len(a) else None
        lb = b[i] if i < len(b) else None
        if la != lb:
            return {"line": i + 1, "old": la, "new": lb}
    if old != new:  # only the line endings or the final newline
        return {"line": len(a) + 1, "old": repr(old[-2:]), "new": repr(new[-2:])}
    return None


def xlsx_parts(path: Path) -> dict:
    with zipfile.ZipFile(path) as z:
        return {n: z.read(n) for n in z.namelist() if not n.startswith("docProps/")}


def compare(item, old_bin, new_bin, out_root, dpi, xlsx) -> dict:
    name, template, options = item
    out = out_root / name
    out.mkdir(parents=True, exist_ok=True)
    opts = options(out)
    row = {"template": name}
    formats = ["pdf", "csv"] + (["xlsx"] if xlsx else [])
    failed = {}
    for fmt in formats:
        for side, binary in (("old", old_bin), ("new", new_bin)):
            err = render(binary, template, out / f"{side}.{fmt}", opts)
            if err:
                failed[f"{side}.{fmt}"] = err
    if failed:
        row["errors"] = failed
    changes = []
    for fmt in formats:
        o, n = f"old.{fmt}", f"new.{fmt}"
        if o in failed and n in failed:
            continue
        if o in failed or n in failed:
            changes.append(f"{fmt}: renders in {'new only' if o in failed else 'old only'}")
            continue
        if fmt == "pdf":
            row.update(compare_pages(out / o, out / n, out, dpi))
            if row["pages_old"] != row["pages_new"]:
                changes.append("pages")
            if row["worst_page_diff"] > MATCH_THRESHOLD:
                changes.append("pixels")
        elif fmt == "csv":
            diff = first_difference((out / o).read_text(encoding="utf-8"), (out / n).read_text(encoding="utf-8"))
            if diff:
                row["csv_difference"] = diff
                changes.append("csv")
        else:
            a, b = xlsx_parts(out / o), xlsx_parts(out / n)
            parts = sorted(p for p in set(a) | set(b) if a.get(p) != b.get(p))
            if parts:
                row["xlsx_parts"] = parts
                changes.append("xlsx")
    row["changes"] = changes
    if changes:
        row["result"] = "changed"
    elif failed:
        row["result"] = "fails in both"
    else:
        row["result"] = "same"
    return row


# --- The report -----------------------------------------------------------------------------

def cell(text, width=70) -> str:
    if text is None:
        return "(no line)"
    text = str(text)
    if len(text) > width:
        text = text[: width - 1] + "…"
    return "`" + text.replace("`", "'").replace("|", "\\|") + "`" if text else "(empty)"


def markdown(rows, old_label, old_version, new_label, new_version, xlsx) -> str:
    changed = [r for r in rows if r["result"] == "changed"]
    both = [r for r in rows if r["result"] == "fails in both"]
    lines = [
        "# What changed since the previous release",
        "",
        f"- old: `{old_label}` ({old_version})",
        f"- new: `{new_label}` ({new_version})",
        "",
        f"Every fixture and example, rendered by both to PDF and CSV{' and XLSX' if xlsx else ''}. "
        f"A template changed when its page count, the pixels of a page (over "
        f"{MATCH_THRESHOLD:.2%} of its ink, at 100 DPI, as tools/compare.py) or its CSV text"
        f"{', or a part of its workbook' if xlsx else ''} differs. "
        "Diff pictures: red is only in the old output, blue only in the new one.",
        "",
        f"**{len(rows)} templates: {len(rows) - len(changed) - len(both)} the same, {len(changed)} changed"
        + (f", {len(both)} failing in both" if both else "")
        + ".**",
        "",
        "| Template | Result | Pages | Worst page | First CSV difference |",
        "|---|---|---|---|---|",
    ]
    for r in rows:
        pages = f"{r['pages_old']} → {r['pages_new']}" if "pages_old" in r and r["pages_old"] != r["pages_new"] else str(r.get("pages_new", r.get("pages_old", "")))
        worst = ""
        if r.get("worst_page_diff", 0) > MATCH_THRESHOLD:
            pic = f"{r['template']}/diff-{r['worst_page']}.png"
            worst = f"[page {r['worst_page']}: {r['worst_page_diff']:.2%}]({pic})"
            if r["pages_differing"] > 1:
                worst += f" ({r['pages_differing']} pages)"
        csv = ""
        if "csv_difference" in r:
            d = r["csv_difference"]
            csv = f"line {d['line']}: {cell(d['old'])} → {cell(d['new'])}"
        result = r["result"]
        notes = [c for c in r["changes"] if c not in ("pages", "pixels", "csv")]
        if r.get("xlsx_parts"):
            notes = [n for n in notes if n != "xlsx"] + ["xlsx: " + ", ".join(r["xlsx_parts"][:3])]
        if notes:
            result += ": " + "; ".join(notes)
        if r["result"] != "changed" and r.get("errors"):
            result += ": " + cell(next(iter(r["errors"].values())), 60)
        lines.append(f"| {r['template']} | {result} | {pages} | {worst} | {csv} |")
    errors = [r for r in rows if r.get("errors") and r["result"] == "changed"]
    if errors:
        lines += ["", "## Errors", ""]
        for r in errors:
            for what, err in r["errors"].items():
                lines.append(f"- {r['template']}, {what}: {cell(err, 200)}")
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--old", required=True, help="a ferroreport binary, or a release version such as 1.3.0")
    ap.add_argument("--new", default=str(ROOT / "target" / "release" / "ferroreport"),
                    help="a ferroreport binary or release version (default: target/release/ferroreport)")
    ap.add_argument("--xlsx", action="store_true", help="also compare the XLSX workbooks")
    ap.add_argument("--strict", action="store_true", help="exit with 1 when anything changed")
    ap.add_argument("--only", help="only the templates whose name contains this")
    ap.add_argument("--dpi", type=int, default=100)
    ap.add_argument("--out", default=str(ROOT / "target" / "compat"))
    ap.add_argument("-j", "--jobs", type=int, default=os.cpu_count() or 2)
    args = ap.parse_args()

    if shutil.which("pdftoppm") is None:
        sys.exit("compat: pdftoppm (poppler) is needed to compare the pages")
    cache = ROOT / "target" / "compat-releases"
    old_bin, old_label = resolve(args.old, cache)
    new_bin, new_label = resolve(args.new, cache)
    old_version, new_version = version_of(old_bin), version_of(new_bin)

    out_root = Path(args.out)
    if out_root.exists():
        shutil.rmtree(out_root)
    out_root.mkdir(parents=True)
    items = [t for t in templates() if not args.only or args.only in t[0]]
    with ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        rows = list(pool.map(lambda t: compare(t, old_bin, new_bin, out_root, args.dpi, args.xlsx), items))

    for r in rows:
        status = {"same": "SAME   ", "changed": "CHANGED", "fails in both": "FAILS  "}[r["result"]]
        detail = ", ".join(r["changes"]) or (next(iter(r["errors"].values())) if r.get("errors") else "")
        print(f"{status}  {r['template']:<40} {detail}")
    (out_root / "results.json").write_text(json.dumps(rows, indent=2, ensure_ascii=False) + "\n")
    (out_root / "report.md").write_text(markdown(rows, old_label, old_version, new_label, new_version, args.xlsx), encoding="utf-8")
    changed = sum(r["result"] == "changed" for r in rows)
    print(f"\n{len(rows)} templates, {changed} changed since {old_version}: {out_root / 'report.md'}")
    return 1 if args.strict and changed else 0


if __name__ == "__main__":
    sys.exit(main())
