#!/usr/bin/env python3
"""Renders JasperReports' own sample reports with both engines and counts what comes out the same.

    JASPER_LIB=/path/to/jasperreports/lib python3 tools/samples.py

The samples are the `demo/samples` folder of the JasperReports repository (140 templates in
71 samples for 6.20.6), fetched on the first run. They were written to show JasperReports
off, feature by feature, so they are a harder test than the reports people usually write, and
nobody chose them with ferroreport in mind.

For every template:

  1. `ferroreport doctor` says whether it loads and which of its features are not supported.
  2. Its rows are worked out the way the sample does it: the SQL query of the template is run
     on the samples' own database (demo/hsqldb, loaded into SQLite), with the parameters the
     sample's Java program passes. Templates without a query get rows with nothing in them,
     as many as the sample's program gives them (one when it does not say).
  3. JasperReports and ferroreport each render it to PDF from those same rows and parameters.
  4. The pages are compared pixel by pixel (the threshold of tools/compare.py).

and one of four results:

  same          every page is the same picture
  differs       both render it, the pictures differ (the worst page's share is given)
  not rendered  ferroreport refuses the template or fails on it (with its reason)
  no reference  JasperReports itself cannot render it from rows and parameters alone: the
                sample hands it Java objects, a live database connection, or files in other
                formats. There is nothing to compare with; ferroreport's own outcome is
                still recorded

The sample's own Java classes are compiled and given to JasperReports, since some templates
call them from their expressions. ferroreport never runs Java code, so those templates count
against it.

The font extensions inside the JasperReports jars are given to ferroreport as resource folders,
since JasperReports finds them on its class path: the core jar has one for the icon font
("JasperReports Icons") that the iconlabel sample draws its icons in. (DejaVu, the font
extension of jasperreports-fonts, is built into ferroreport.)

Writes docs/samples.json and docs/samples.md.
"""
import argparse
import json
import os
import re
import sqlite3
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from compare import MATCH_THRESHOLD, diff_page, load, rasterize  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
REPO = "https://github.com/TIBCOSoftware/jasperreports.git"
STAMP = re.compile(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(\.\d+)?$")


def fetch(tag, cache):
    """The demo folder of JasperReports at `tag`, cloned once."""
    src = cache / tag
    # Up to 6.x the repository keeps everything under jasperreports/; from 7.0 demo/ is at its top.
    for demo in (src / "jasperreports" / "demo", src / "demo"):
        if (demo / "samples").exists():
            return demo
    if not src.exists():
        cache.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "clone", "-q", "--depth", "1", "--branch", tag, "--filter=blob:none", "--sparse", REPO, str(src)], check=True)
    nested = subprocess.run(["git", "-C", str(src), "ls-tree", "-d", "--name-only", "HEAD", "jasperreports/demo"], capture_output=True, text=True, check=True).stdout.strip()
    prefix = "jasperreports/demo" if nested else "demo"
    subprocess.run(["git", "-C", str(src), "sparse-checkout", "set", f"{prefix}/samples", f"{prefix}/hsqldb"], check=True)
    return src / prefix


def database(script):
    """The samples' HSQLDB database, in SQLite."""
    db = sqlite3.connect(":memory:")
    db.row_factory = sqlite3.Row
    for line in script.read_text(encoding="utf-8").splitlines():
        if line.startswith("CREATE MEMORY TABLE "):
            line = line.replace("CREATE MEMORY TABLE PUBLIC.", "CREATE TABLE ")
            line = re.sub(r",CONSTRAINT .*?(?=,CONSTRAINT|\)$)", "", line)
        elif not line.startswith("INSERT INTO "):
            continue
        # The script writes characters outside ASCII as \uXXXX.
        line = re.sub(r"\\u([0-9a-fA-F]{4})", lambda m: chr(int(m.group(1), 16)), line)
        try:
            db.execute(line)
        except sqlite3.Error as e:
            print(f"database: skipped a line ({e}): {line[:70]}", file=sys.stderr)
    return db


def literal(text):
    """The value of a Java literal, when `text` is one a report parameter can take from JSON."""
    text = text.strip()
    m = re.fullmatch(r'"((?:[^"\\]|\\.)*)"', text)
    if m:
        return m.group(1).encode().decode("unicode_escape")
    m = re.fullmatch(r"(?:new (?:Integer|Long|Short)\(|Integer\.valueOf\()?(-?\d+)[lL]?\)?", text)
    if m:
        return int(m.group(1))
    m = re.fullmatch(r"(?:new (?:Double|Float)\()?(-?\d+\.\d+)[dDfF]?\)?", text)
    if m:
        return float(m.group(1))
    if text in ("true", "Boolean.TRUE"):
        return True
    if text in ("false", "Boolean.FALSE"):
        return False
    return None


def sample_params(sample):
    """What the sample's Java program passes with `parameters.put("Name", literal)`."""
    out = {}
    for java in (sample / "src").rglob("*.java") if (sample / "src").exists() else []:
        for name, value in re.findall(r'\.put\(\s*"(\w+)"\s*,\s*([^;]+?)\s*\)\s*;', java.read_text(encoding="utf-8", errors="replace")):
            v = literal(value)
            if v is not None:
                out.setdefault(name, v)
    return out


def empty_rows(sample):
    """How many empty rows the sample's program fills its reports with (`new JREmptyDataSource(n)`); one otherwise."""
    for java in (sample / "src").rglob("*.java") if (sample / "src").exists() else []:
        m = re.search(r"new JREmptyDataSource\((\d*)\)", java.read_text(encoding="utf-8", errors="replace"))
        if m:
            return int(m.group(1) or 1)
    return 1


def rows_for(template, db, params):
    """(rows, note): the rows of the template's SQL query, or None when it has none."""
    xml = template.read_text(encoding="utf-8", errors="replace")
    # The main query is the one outside any <subDataset>.
    # (JRXML 6 <subDataset>, JRXML 7 <dataset name="...">; JRXML 7 calls the query <query>.)
    main = re.sub(r"<subDataset.*?</subDataset>|<dataset name=.*?</dataset>", "", xml, flags=re.S)
    m = re.search(r"<(?:queryString|query)(?:\s+language=\"(\w+)\")?\s*>\s*<!\[CDATA\[(.*?)\]\]>", main, re.S)
    if not m or not m.group(2).strip():
        return None, None
    if (m.group(1) or "sql").lower() != "sql":
        return None, f"a {m.group(1)} query"
    declared = dict(re.findall(r'<parameter name="(\w+)"[^>]*>\s*<defaultValueExpression><!\[CDATA\[(.*?)\]\]>', main, re.S))
    values = []

    def value_of(name):
        if name in params:
            return params[name]
        return literal(declared.get(name, ""))

    sql = re.sub(r"\$P!\{(\w+)\}", lambda p: str(value_of(p.group(1)) or ""), m.group(2))

    def bind(p):
        values.append(value_of(p.group(1)))
        return "?"

    sql = re.sub(r"\$P\{(\w+)\}", bind, sql)
    if "$X{" in sql:
        return None, "a query with $X{} clauses"
    try:
        found = db.execute(sql, values).fetchall()
    except sqlite3.Error as e:
        return None, f"its query does not run on the sample database ({e})"
    fields = re.findall(r'<field name="([^"]+)"', main)
    by_lower = {f.lower(): f for f in fields}
    rows = []
    for r in found:
        row = {}
        for key in r.keys():
            v = r[key]
            if isinstance(v, str) and STAMP.match(v):
                v = v[:19].replace(" ", "T")
            row[by_lower.get(key.lower(), key)] = v
        rows.append(row)
    return rows, None


def first_reason(text):
    """The line of an engine's output that says what went wrong."""
    lines = [line.strip() for line in text.splitlines() if line.strip() and not line.startswith(("Picked up", "WARNING", "SLF4J", "log4j"))]
    for line in lines:
        if line.startswith("error: ") and "template not supported" not in line:
            return line[7:][:240]
    for line in lines:
        if "Exception" in line or "Error" in line:
            return re.sub(r"^Exception in thread \"main\" ", "", line)[:240]
    return (lines[-1] if lines else "no output")[:240]


def why_no_reference(reason):
    """A plain category for why JasperReports could not render a sample from rows alone."""
    table = [
        (r"ClassNotFoundException|cannot be resolved|NoClassDefFoundError|compil", "needs Java classes that are not at hand (the sample's own, or libraries it adds)"),
        (r"SQLException|Connection|REPORT_CONNECTION|query executer", "runs SQL through a live database connection inside the report"),
        (r"FileNotFound|not found|Unable to load|Resource", "needs files that the sample's program provides"),
    ]
    for pattern, category in table:
        if re.search(pattern, reason, re.I):
            return category
    return "JasperReports fails on it without the sample's own program"


def sample_classes(sample, lib, out_root, done={}):
    """The sample's own Java classes, compiled: some templates call them from their expressions.
    Returns the folder, or None when they do not compile with the jars at hand."""
    if sample not in done:
        sources = [str(p) for p in (sample / "src").rglob("*.java")] if (sample / "src").exists() else []
        classes = out_root / "classes" / sample.name
        done[sample] = None
        if sources:
            classes.mkdir(parents=True, exist_ok=True)
            p = subprocess.run(["javac", "--release", "17", "-proc:none", "-nowarn", "-cp", f"{lib}/*", "-d", str(classes), *sources], capture_output=True, text=True)
            if p.returncode == 0:
                done[sample] = classes
    return done[sample]


def font_extensions(lib, out_root):
    """The font extensions of the jars in `lib`, each unpacked in a folder of its own (its
    jasperreports_extension.properties at the folder's root, the files it names under it), as
    JasperReports finds them on its class path. ferroreport reads them from resource folders."""
    folders = []
    for jar in sorted(Path(lib).glob("*.jar")):
        if jar.name.startswith("jasperreports-fonts-"):
            continue  # DejaVu, built into ferroreport
        with zipfile.ZipFile(jar) as z:
            try:
                props = z.read("jasperreports_extension.properties").decode("latin-1")
            except KeyError:
                continue
            files = [v.strip().lstrip("/") for k, v in (line.split("=", 1) for line in props.splitlines() if "=" in line)
                     if k.strip().startswith("net.sf.jasperreports.extension.simple.font.families.")]
            if not files:
                continue
            folder = out_root / "font-extensions" / jar.stem
            folder.mkdir(parents=True, exist_ok=True)
            (folder / "jasperreports_extension.properties").write_text(props, encoding="latin-1")
            # The fonts.xml files, and the fonts next to them.
            dirs = {f.rsplit("/", 1)[0] + "/" for f in files if "/" in f}
            for name in z.namelist():
                if not name.endswith("/") and (name in files or any(name.startswith(d) for d in dirs)):
                    target = folder / name
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes(z.read(name))
            folders.append(folder)
    return folders


def run(cmd, cwd=None, timeout=120):
    started = time.perf_counter()
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd, timeout=timeout)
        return p.returncode, p.stdout + p.stderr, time.perf_counter() - started
    except subprocess.TimeoutExpired:
        return 124, f"error: no answer in {timeout} s", timeout


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--tag", default="6.20.6", help="the JasperReports release whose samples are used")
    ap.add_argument("--samples", help="a demo/ folder already on disk, instead of fetching one")
    ap.add_argument("--bin", default=str(ROOT / "target" / "release" / "ferroreport"))
    ap.add_argument("--lib", default=os.environ.get("JASPER_LIB"), help="folder with the JasperReports jars (or JASPER_LIB)")
    ap.add_argument("--only", help="only templates whose path contains this")
    ap.add_argument("--out", default=str(ROOT / "target" / "samples"))
    ap.add_argument("--dpi", type=int, default=100)
    ap.add_argument("--no-write", action="store_true", help="do not update docs/samples.json and docs/samples.md")
    args = ap.parse_args()
    # The Java runtime's logical fonts (SansSerif...) as on a stock Linux server: DejaVu.
    os.environ["FONTCONFIG_FILE"] = str(ROOT / "tools" / "oracle" / "fonts.conf")
    if not args.lib:
        raise SystemExit("set JASPER_LIB to a folder with the JasperReports jars")
    demo = (Path(args.samples) if args.samples else fetch(args.tag, ROOT / "target" / "jasper-samples")).resolve()
    db = database(demo / "hsqldb" / "test.script")
    oracle = ROOT / "target" / "oracle"
    if not (oracle / "Oracle.class").exists():
        oracle.mkdir(parents=True, exist_ok=True)
        subprocess.run(["javac", "--release", "17", "-proc:none", "-nowarn", "-cp", f"{args.lib}/*", "-d", str(oracle), str(ROOT / "tools" / "oracle" / "Oracle.java")], check=True)
    out_root = Path(args.out).resolve()
    fonts = font_extensions(args.lib, out_root)
    results = []
    for template in sorted((demo / "samples").rglob("*.jrxml")):
        rel = template.relative_to(demo / "samples")
        if args.only and args.only not in str(rel):
            continue
        sample = demo / "samples" / rel.parts[0]
        out = out_root / "-".join(rel.with_suffix("").parts)
        out.mkdir(parents=True, exist_ok=True)
        for old in out.glob("*"):
            old.unlink()
        row = {"template": str(rel)}

        code, text, _ = run([args.bin, "doctor", str(template)])
        issues = [line.strip()[2:] for line in text.splitlines() if line.strip().startswith("- ")]
        # Rows come from the application in ferroreport: a query in the template is not a missing feature.
        row["issues"] = [i for i in issues if "query is ignored" not in i]

        params = sample_params(sample)
        rows, note = rows_for(template, db, params)
        if rows is None and note is None:
            # No query: the samples fill these with rows that have nothing in them.
            rows = [{} for _ in range(empty_rows(sample))]
        data_file, params_file = out / "data.json", out / "params.json"
        params_file.write_text(json.dumps(params))
        if rows is not None:
            data_file.write_text(json.dumps(rows))
            row["rows"] = len(rows)
        resources = [str(sample), str(sample / "src"), str(template.parent), *map(str, fonts)]

        jasper_pdf, ours_pdf = out / "jasper.pdf", out / "ferroreport.pdf"
        classes = sample_classes(sample, args.lib, out_root)
        classpath = os.pathsep.join([str(oracle), f"{args.lib}/*", *([str(classes)] if classes else []), str(sample / "src"), str(template.parent), str(sample)])
        # ORACLE_JAVA: a Java 17 runtime lets JasperReports 6.20.6 compile Groovy expressions.
        # JasperReports 7 needs Java 21 (its JFreeChart is built for it): leave it unset there.
        # The rows' dates are UTC, and so is the Java runtime's own time zone, which methods such
        # as Date.getMonth() use, whatever the machine's is.
        jcode, jtext, jsec = run([os.environ.get("ORACLE_JAVA", "java"), "-Djava.awt.headless=true", "-Duser.timezone=UTC", f"-Duser.home={out_root / '.java-home'}", "-cp", classpath, "Oracle", str(template),
                                  str(data_file) if rows is not None else "-", str(jasper_pdf), str(params_file)], cwd=sample)
        cmd = [args.bin, "render", str(template), "-o", str(ours_pdf), "-q", "-p", str(params_file), "--timeout", "120"]
        for r in resources:
            cmd += ["-r", r]
        if rows is not None:
            cmd += ["-d", str(data_file)]
        ocode, otext, osec = run(cmd, cwd=sample)
        row["ferroreport_seconds"] = round(osec, 3)
        row["jasper_seconds"] = round(jsec, 2)
        ours_ok = ocode == 0 and ours_pdf.exists()
        if not ours_ok:
            row["ferroreport_error"] = first_reason(otext)

        if jcode != 0 or not jasper_pdf.exists():
            reason = note or first_reason(jtext)
            row.update(result="no reference", reason=why_no_reference(reason) if not note else f"its rows come from {note}", detail=reason)
            row["ferroreport_renders"] = ours_ok
        elif not ours_ok:
            row.update(result="not rendered", reason=row.pop("ferroreport_error"))
        else:
            with tempfile.TemporaryDirectory() as tmp:
                tmp = Path(tmp)
                pa, pb = rasterize(jasper_pdf, args.dpi, tmp / "j"), rasterize(ours_pdf, args.dpi, tmp / "o")
                shares = [diff_page(load(pa[i]), load(pb[i]), out / f"diff-{i + 1}.png") if i < len(pa) and i < len(pb) else 1.0
                          for i in range(max(len(pa), len(pb)))]
                # Blank pages are no evidence of anything: a sample that draws nothing without
                # its own program has no reference to compare with.
                blank = not any((load(p) < 200).any() for p in pa)
            same = len(pa) == len(pb) and all(s <= MATCH_THRESHOLD for s in shares)
            if blank:
                row.update(result="no reference", reason="JasperReports draws nothing for it without the sample's own program",
                           detail=f"{len(pa)} blank page(s)", ferroreport_renders=True)
            else:
                row.update(result="same" if same else "differs", pages_jasper=len(pa), pages_ferroreport=len(pb),
                           worst_page=round(max(shares) if shares else 0.0, 4))
            if row["result"] == "differs":
                row["reason"] = "; ".join(row["issues"][:3]) or ("a different number of pages" if len(pa) != len(pb) else "")
        results.append(row)
        print(f"{row['result']:13} {rel}  {row.get('reason', '')[:100]}" + (f"  [{row['detail'][:150]}]" if args.only and "detail" in row else ""), flush=True)

    counts = {k: sum(r["result"] == k for r in results) for k in ("same", "differs", "not rendered", "no reference")}
    compared = counts["same"] + counts["differs"] + counts["not rendered"]
    print(f"\n{len(results)} templates: {counts}")
    print(f"{counts['same']} of the {compared} that JasperReports renders from rows alone come out the same")
    if args.no_write or args.only:
        return
    summary = {"jasperreports": args.tag, "templates": len(results), **{k.replace(" ", "_"): v for k, v in counts.items()},
               "compared": compared, "threshold": MATCH_THRESHOLD, "results": results}
    # The release the comparison is made against is in the name, but for the default one.
    stem = "samples" if args.tag == ap.get_default("tag") else f"samples-{args.tag}"
    (ROOT / "docs" / f"{stem}.json").write_text(json.dumps(summary, indent=1, ensure_ascii=False) + "\n")
    (ROOT / "docs" / f"{stem}.md").write_text(markdown(summary))


def markdown(s):
    lines = [
        "# JasperReports' own samples, rendered by ferroreport",
        "",
        f"The {s['templates']} templates of the `demo/samples` folder of JasperReports {s['jasperreports']}, rendered by both engines from",
        "the same rows and parameters and compared page by page. Made by `tools/samples.py`; nothing here is written by hand.",
        "",
        "| Result | Templates | Meaning |",
        "|---|---|---|",
        f"| same | {s['same']} | every page is the same picture |",
        f"| differs | {s['differs']} | both engines render it, the pictures differ |",
        f"| not rendered | {s['not_rendered']} | ferroreport refuses the template or fails on it |",
        f"| no reference | {s['no_reference']} | JasperReports cannot render it from rows alone (it needs the sample's Java classes, a database connection inside the report, or other files), so there is nothing to compare with |",
        "",
        f"**{s['same']} of the {s['compared']} templates that can be compared come out the same.** JasperReports runs with the",
        "Java runtime's logical fonts (SansSerif, Serif, Monospaced) on DejaVu, as on a stock Linux server (`tools/oracle/fonts.conf`):",
        "text that names no font is laid out with them and written in PDF's built-in Helvetica, Times or Courier, by both engines.",
        "",
        "These samples were written to show every feature of JasperReports, one per sample: charts of every kind, maps, forms,",
        "Java scriptlets, database connections inside the report. They are a much harder test than the reports people usually",
        "write, which is what `fixtures/` holds (all pixel-identical). Use `ferroreport doctor your.jrxml` to see where yours stands.",
        "",
        "| Template | Result | Pages | Why |",
        "|---|---|---|---|",
    ]
    for r in s["results"]:
        pages = f"{r['pages_ferroreport']}/{r['pages_jasper']}" if "pages_jasper" in r else ""
        why = r.get("reason", "")
        if r["result"] == "differs":
            why = f"worst page differs in {r['worst_page'] * 100:.1f}% of its ink" + (f": {why}" if why else "")
        if r["result"] == "no reference":
            why += "; ferroreport renders it" if r.get("ferroreport_renders") else "; ferroreport: " + r.get("ferroreport_error", "does not render it")
        lines.append(f"| `{r['template']}` | {r['result']} | {pages} | {why.replace('|', '/')} |")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    main()
