#!/usr/bin/env python3
"""Compares ferroreport against JasperReports on every fixture.

For each directory in fixtures/ containing report.jrxml (+ optional data.json, params.json):
  1. renders it with the reference JasperReports engine (tools/oracle) -> jasper.pdf
  2. renders it with ferroreport                                      -> ferroreport.pdf
  3. rasterizes both at the same DPI and compares them pixel by pixel.

A page "matches" when the share of differing pixels is below the threshold. Fixtures with an
APPROXIMATE file (charts, which follow JFreeChart's look without copying it pixel for pixel)
only need the same page count; they are reported as APPROX. Diff images
(red = only in Jasper, blue = only in ferroreport) are written next to the PDFs.

Usage: tools/compare.py [--oracle PATH] [--dpi N] [--only NAME] [--out DIR]
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
MATCH_THRESHOLD = 0.0015  # share of pixels allowed to differ (anti-aliasing noise)


def rasterize(pdf: Path, dpi: int, prefix: Path) -> list:
    subprocess.run(["pdftoppm", "-r", str(dpi), "-gray", "-png", str(pdf), str(prefix)], check=True)
    return sorted(prefix.parent.glob(prefix.name + "*.png"))


def load(path: Path) -> np.ndarray:
    return np.asarray(Image.open(path).convert("L"), dtype=np.int16)


def diff_page(a: np.ndarray, b: np.ndarray, out: Path) -> float:
    h, w = max(a.shape[0], b.shape[0]), max(a.shape[1], b.shape[1])
    pa = np.full((h, w), 255, np.int16)
    pb = np.full((h, w), 255, np.int16)
    pa[: a.shape[0], : a.shape[1]] = a
    pb[: b.shape[0], : b.shape[1]] = b
    delta = np.abs(pa - pb) > 64
    ink = (pa < 200) | (pb < 200)
    share = delta.sum() / max(ink.sum(), 1) if ink.any() else 0.0
    # Visual diff: grey content, red where only Jasper has ink, blue where only we do.
    img = np.stack([pa, pa, pa], axis=-1).astype(np.uint8)
    img = (img * 0.35 + 165).astype(np.uint8)
    only_a = delta & (pa < pb)
    only_b = delta & (pb < pa)
    img[only_a] = [220, 30, 30]
    img[only_b] = [30, 80, 230]
    Image.fromarray(img).save(out)
    return float(share)


NOW = "2026-01-15T10:30:00"  # the value of `new Date()` in every comparison


def fixture_args(fx: Path, out: Path) -> list:
    """The options `ferroreport render` takes for fixture `fx` (besides the template, output,
    -q and --now): its rows, parameters, fonts, font sets and resources. A fixture filled from a
    database (data.sql) gets a SQLite database built in `out`."""
    args = []
    data, params = fx / "data.json", fx / "params.json"
    if data.exists():
        args += ["-d", str(data)]
    if (fx / "data.sql").exists():
        # Filled from its own query, on a SQLite database built from data.sql.
        import sqlite3
        db = out / "data.sqlite"
        db.unlink(missing_ok=True)
        with sqlite3.connect(db) as c:
            c.executescript((fx / "data.sql").read_text())
        args += ["--db", f"sqlite:{db}"]
    if params.exists():
        args += ["-p", str(params)]
    # Fonts the fixture needs (fonts.txt: Family=file.ttf[:bold][:italic]), when installed.
    if (fx / "fonts.txt").exists():
        for spec in (fx / "fonts.txt").read_text().split():
            if Path(spec.split("=", 1)[-1].split(":")[0]).exists():
                args += ["--font", spec]
    # The fixture's font extension is read from a resource folder, as Jasper reads it
    # from its class path.
    if (fx / "jasper-extension").is_dir():
        args += ["-r", str(fx / "jasper-extension")]
    # Font sets the fixture defines (fontsets.txt: Name=Family,Family per line).
    if (fx / "fontsets.txt").exists():
        for spec in (fx / "fontsets.txt").read_text().splitlines():
            if spec.strip():
                args += ["--font-set", spec.strip()]
    return args


def run(cmd: list, cwd=None, env=None) -> tuple:
    t0 = time.perf_counter()
    p = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd, env=env)
    return p, time.perf_counter() - t0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--oracle", default=os.environ.get("JASPER_ORACLE", str(ROOT / "tools" / "oracle" / "run.sh")))
    ap.add_argument("--bin", default=str(ROOT / "target" / "release" / "ferroreport"))
    ap.add_argument("--dpi", type=int, default=100)
    ap.add_argument("--only")
    ap.add_argument("--out", default=str(ROOT / "target" / "compare"))
    args = ap.parse_args()

    out_root = Path(args.out)
    out_root.mkdir(parents=True, exist_ok=True)
    results = []
    for fx in sorted((ROOT / "fixtures").iterdir()):
        if not (fx / "report.jrxml").exists() or (args.only and args.only not in fx.name):
            continue
        out = out_root / fx.name
        out.mkdir(exist_ok=True)
        for old in out.glob("*"):
            old.unlink()
        data = fx / "data.json"
        params = fx / "params.json"
        jasper_pdf, ours_pdf = out / "jasper.pdf", out / "ferroreport.pdf"

        cmd = [args.oracle, str(fx / "report.jrxml"), str(data) if data.exists() else "-", str(jasper_pdf)]
        if params.exists():
            cmd.append(str(params))
        # Run Jasper from the fixture directory so relative resource paths resolve there.
        # A fixture filled from a database needs a JDBC driver for the reference run.
        # Groovy templates need JasperReports on Java 17 (ORACLE_JAVA_GROOVY): its Groovy 4.0.8
        # cannot compile on Java 21. The rest run on the default Java, whose dates (Java 21)
        # are the ones ferroreport follows.
        env = dict(os.environ)
        if os.environ.get("ORACLE_JAVA_GROOVY") and 'language="groovy"' in (fx / "report.jrxml").read_text(errors="replace"):
            env["ORACLE_JAVA"] = os.environ["ORACLE_JAVA_GROOVY"]
        # A fixture's own JasperReports font extension (font sets), as fontsets.txt below.
        if (fx / "jasper-extension").is_dir():
            env["ORACLE_CLASSPATH"] = str(fx / "jasper-extension")
        pj, tj = run(cmd, cwd=fx, env=env) if not (fx / "data.sql").exists() else (None, 0.0)
        cmd = [args.bin, "render", str(fx / "report.jrxml"), "-o", str(ours_pdf), "-q", "--now", NOW]
        po, to = run(cmd + fixture_args(fx, out))

        row = {"fixture": fx.name, "jasper_s": round(tj, 3), "ferroreport_s": round(to, 4)}
        skip_note = fx / "ORACLE-SKIP"
        if not jasper_pdf.exists() and skip_note.exists() and po.returncode == 0:
            row["skipped"] = skip_note.read_text().strip().splitlines()[0]
        elif not jasper_pdf.exists():
            row["error"] = "jasper failed: " + ((pj.stdout + pj.stderr)[-500:] if pj else "no reference run")
        elif po.returncode != 0:
            row["error"] = "ferroreport failed: " + po.stderr[-500:]
        else:
            with tempfile.TemporaryDirectory() as tmp:
                tmp = Path(tmp)
                pa = rasterize(jasper_pdf, args.dpi, tmp / "j")
                pb = rasterize(ours_pdf, args.dpi, tmp / "o")
                shares = []
                for i in range(max(len(pa), len(pb))):
                    if i < len(pa) and i < len(pb):
                        shares.append(diff_page(load(pa[i]), load(pb[i]), out / f"diff-{i + 1}.png"))
                    else:
                        shares.append(1.0)
                row.update(
                    pages_jasper=len(pa),
                    pages_ferroreport=len(pb),
                    worst_page_diff=round(max(shares) if shares else 0.0, 5),
                    pages_matching=sum(s <= MATCH_THRESHOLD for s in shares),
                )
                row["match"] = len(pa) == len(pb) and all(s <= MATCH_THRESHOLD for s in shares)
                # Fixtures marked APPROXIMATE (charts) are checked for page count only: they
                # follow JFreeChart's look but are not drawn pixel for pixel.
                if (fx / "APPROXIMATE").exists() and not row["match"]:
                    row["approximate"] = len(pa) == len(pb)
        results.append(row)
        if "approximate" in row:
            status = "APPROX" if row["approximate"] else "DIFF "
        else:
            status = "MATCH" if row.get("match") else ("ERROR" if "error" in row else ("SKIP " if "skipped" in row else "DIFF "))
        detail = row.get("error") or row.get("skipped") or (f"pages {row.get('pages_jasper')}/{row.get('pages_ferroreport')}, worst diff {row.get('worst_page_diff')}")
        print(f"{status}  {fx.name:<32} {detail}")

    (out_root / "results.json").write_text(json.dumps(results, indent=2))
    ok = sum(1 for r in results if r.get("match"))
    approx = sum(1 for r in results if r.get("approximate"))
    compared = sum(1 for r in results if "skipped" not in r and "approximate" not in r)
    print(f"\n{ok}/{compared} compared fixtures render identically to JasperReports "
          f"(threshold {MATCH_THRESHOLD:.2%} of ink pixels; {approx} approximate charts; "
          f"{len(results) - compared - approx} skipped)")
    return 0 if ok == compared and all(r.get("approximate", True) for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
