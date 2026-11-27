//! `ferroreport` command-line interface.

use ferroreport::model::{Band, ElementKind, Report};
use ferroreport::Renderer;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

#[cfg(feature = "db")]
mod db;
mod pixels;
mod serve;
mod signing;
mod spool;

const USAGE: &str = "\
ferroreport — report engine: PDF, Excel, Word, PowerPoint, OpenDocument, CSV and HTML from JasperReports JRXML templates
or .ferro templates, no JVM required

USAGE:
    ferroreport render <template.jrxml|template.ferro> [OPTIONS]
    ferroreport doctor <template.jrxml|template.ferro>
    ferroreport serve [OPTIONS]
    ferroreport convert <template> [-o <output>] [--to ferro|jrxml]
    ferroreport migrate <folder|template...> [--report <file.md|file.json>] [--resources <folder>] [--strict] [--jasper <lib> [--keep <folder>] [--strict-pixels]] [--to ferro|jrxml --out <folder>]

RENDER OPTIONS:
    -d, --data <file.json>      Rows: a JSON array of objects, or one JSON object per line
                                (omit for reports without data; - reads stdin). Rows are read
                                as they are needed, so the file can be larger than memory
    -p, --params <file.json>    JSON object with parameter values
    -o, --output <file>         Output file (default: <template>.pdf; - writes stdout). The
                                extension picks the format: .pdf, .xlsx, .docx, .pptx, .ods,
                                .odt, .csv or .html
    -f, --format <pdf|xlsx|docx|pptx|ods|odt|csv|html>
                                Output format, when the file name does not say it. html is a
                                web page that looks like the PDF, for previews
        --xlsx-numbers          In XLSX and ODS, write numbers as numbers, and in ODS dates as
                                dates (default: as formatted text, like JasperReports)
        --csv-safe              In CSV, put an apostrophe before values a spreadsheet would
                                run as a formula (use it when the data comes from other people)
        --pdfa <2b|2u|2a|3b|3u|3a>
                                Write PDF/A, for archiving (every font embedded, sRGB, the
                                conformance in the metadata; level a is also tagged)
        --attach <file[:mime]>  Carry a file inside the PDF (repeatable; PDF/A-3 allows it),
                                such as the XML of an electronic invoice
        --tagged                In PDF, say what the content is (tagged PDF) for screen readers:
                                texts as paragraphs, images as figures, the rest as decoration
        --max-pages <n>         Stop if the report goes over this many pages
        --max-rows <n>          Stop if the report reads more than this many rows (the rows
                                of its subreports, tables, lists, charts and crosstabs too)
        --timeout <seconds>     Stop if the render takes longer than this
        --max-row <MB>          Stop at a row of data larger than this (default: 256)
    -w, --watch                 Render again whenever the template, its folder, the rows or the
                                parameters change (stop with Ctrl+C)
        --sign-key <key>        Sign the PDF (PAdES B-B) with this private key: PEM (PKCS#8 RSA
                                or ECDSA P-256) or a .p12/.pfx, whose password goes in
                                FERROREPORT_SIGN_PASSWORD (or FERROREPORT_SIGN_KEY)
        --sign-cert <cert.pem>  The signer's certificate and its chain (repeatable; or
                                FERROREPORT_SIGN_CERT)
        --sign-reason <text>    Why it is signed; also --sign-name, --sign-location,
                                --sign-contact
        --db <url>              Fill the report from its own SQL query, and its subreports from
                                theirs: postgres://, mysql://, sqlserver://, sqlite:file.db (a
                                jdbc: URL works too). Or FERROREPORT_DB; the password goes in
                                FERROREPORT_DB_PASSWORD
        --user-password <pw>    Encrypt the PDF (AES-256); this password opens it (an empty one opens
                                it without asking). Also FERROREPORT_PDF_USER_PASSWORD, which
                                keeps it out of the list of processes
        --owner-password <pw>   Lifts the permissions (also FERROREPORT_PDF_OWNER_PASSWORD;
                                without one, nobody can lift them)
        --permissions <list>    What readers allow without the owner password: print, copy,
                                modify, annotate, fill, accessibility, assemble, all (comma
                                separated; default none). Encrypts the PDF too
    -l, --locale <tag>          Locale for numbers and dates (default: en_US)
        --font <Family=file.ttf[:bold][:italic]>
                                Register an extra font (repeatable)
        --font-set <Name=Family,Family,...>
                                Define a font set, as in a JasperReports font extension: text
                                in font Name is drawn in the first of the families that has
                                its characters; the first family is the primary one (repeatable)
        --font-fallback         Draw characters a text's font lacks in another font that has
                                them: the --font families, then DejaVu Sans, Serif, Sans Mono
    -r, --resources <dir>       Extra directory for images and other resources (repeatable;
                                the template's own directory is always searched). A JasperReports
                                font extension at its root (jasperreports_extension.properties)
                                is read, as from JasperReports' class path
        --now <YYYY-MM-DDTHH:MM:SS>
                                Fixed value for `new Date()`
    -q, --quiet                 Do not print timing information

DOCTOR:
    Lists the features of a template and whether ferroreport can render them.

MIGRATE:
    Checks every template of a folder (and its subfolders) before moving to ferroreport:
    whether it loads, what it uses, what is not supported yet, and whether it renders with
    no rows (pictures are looked for next to each template, in its parent folder and in
    every --resources folder). Prints a summary; --report writes every template's result as Markdown or JSON.
    With --to and --out it also writes each template converted, in the same folders.
    With --jasper <folder of JasperReports jars> (and a JDK 11 or newer on the PATH), every
    template that loads is also rendered by JasperReports and by ferroreport from the same rows
    (<name>.json or data.json next to it, parameters from <name>.params.json or params.json):
    pages, every text and number (their CSV exports) and times are compared. With pdftoppm
    (poppler) on the PATH, every page's pixels too, as tools/compare.py does (100 DPI, a page
    differs when over 0.15% of its ink does); the worst page is given. --keep <folder>
    keeps both PDFs and CSVs of each, and the diff picture of each page that differs (red: only
    JasperReports, blue: only ferroreport). Nothing is written next to your templates.
    Exits with 1 when a template cannot be loaded; with --strict, also when one uses something
    not supported yet, stops when rendered with no rows, or differs from JasperReports (pages,
    texts and numbers). --strict-pixels (with --jasper) also exits with 1 when a page of a
    template both engines render is a different picture, or when the pictures could not be
    compared (no pdftoppm). A template one engine cannot render is left to --strict.

CONVERT:
    Rewrites a template: JRXML (JasperReports 6 or 7) as a .ferro template, ferroreport's own
    format, or a .ferro template or a JRXML 6 one as JRXML 7. The output's extension (or --to)
    picks the format; without either, a JRXML becomes .ferro and a .ferro becomes JRXML.
    Templates are rendered the same in either format.

SERVE OPTIONS (HTTP API for any language: POST /render, POST /render/<name>, POST /inspect;
add format xlsx, docx, pptx, ods, odt, csv or html to the request for Excel, Word,
PowerPoint, OpenDocument, CSV or a web preview):
        --host <addr>           Address to listen on (default: 127.0.0.1; 0.0.0.0 in Docker)
    -P, --port <n>              Port (default: 8080)
    -t, --templates <dir>       Directory of templates for POST /render/<name>
    -r, --resources <dir>       Extra directory for images and subreports (repeatable)
        --token <secret>        Require `Authorization: Bearer <secret>` (or FERROREPORT_TOKEN).
                                Needed to listen beyond localhost, unless:
        --allow-anonymous       Listen beyond localhost without a token (or
                                FERROREPORT_ALLOW_ANONYMOUS=1): only for ports your own
                                services alone can reach
        --rate-limit <n>        Most requests per minute from one client address (answers 429)
        --sign-key <key>        Sign every PDF (PAdES B-B): a PEM key, or a .p12 with its password
                                in FERROREPORT_SIGN_PASSWORD; --sign-cert, --sign-reason... as
                                in render
        --db <url>              The database the SQL queries of templates from --templates run
                                on (or FERROREPORT_DB; password in FERROREPORT_DB_PASSWORD)
        --spool-memory <MB>     How much of one document is kept in memory until it is complete
                                (default: 32; the rest goes to a file)
        --spool-dir <dir>       Where those files go (default: the system's temporary directory;
                                or FERROREPORT_SPOOL_DIR)
        --spool-disk <MB>       Disk all requests together may use for them (default: 4096)
    Large reports: POST with Content-Type application/x-ndjson, the request on the first line
    and one row per line after it. Add `Prefer: respond-streaming` to get the document as it
    renders instead of once it is complete.
        --max-body <MB>         Largest request accepted (default: 50)
        --workers <n>           Requests rendered in parallel (default: CPU count)
        --timeout <seconds>     Longest a render may take (default: 60; 0 for no limit)
        --max-pages <n>         Most pages a report may have (default: no limit)
        --max-rows <n>          Most rows a report may read, its subreports' and datasets'
                                included (default: no limit)
        --cache <n>             Parsed templates kept in memory (default: 128; 0 for none)
        --access-log [json]     Print a line per request to standard error: method, path,
                                status and time; with json (or FERROREPORT_ACCESS_LOG=json), one
                                JSON object per line that also has the time, the request ID, the
                                template, format, pages, rows and bytes, and the error. Never the
                                token, the body, parameter values or rows, except that an error
                                message can quote the value that could not be read
    GET /health answers without a token; GET /metrics gives counters for Prometheus. Every
    response carries X-Request-Id: the request's own (printable ASCII, 128 characters at most)
    or a new one.
";

fn main() -> ExitCode {
    // Work on a thread with a large stack: templates are processed recursively, and the main
    // thread's stack is small on some systems (1 MB on Windows).
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .ok()
        .and_then(|t| t.join().ok())
        .unwrap_or(ExitCode::FAILURE)
}

fn run() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("render") if args.iter().any(|a| a == "--watch" || a == "-w") => watch(&args[1..]),
        Some("render") => render(&args[1..]),
        Some("doctor") => doctor(&args[1..]),
        Some("serve") => serve(&args[1..]),
        Some("convert") => convert(&args[1..]),
        Some("migrate") => migrate(&args[1..]),
        Some("-V") | Some("--version") => {
            println!("ferroreport {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type CliResult = Result<(), Box<dyn std::error::Error>>;

fn read_json(path: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let text = if path == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| format!("stdin: {e}"))?;
        s
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?
    };
    Ok(serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?)
}

/// The text with its first letter in capitals.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or(String::new(), |c| c.to_uppercase().chain(chars).collect())
}

/// A temporary file that goes away with the value.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// One line on how a template came out in both engines.
fn describe(c: &Compared) -> String {
    if let Some(e) = &c.jasper_error {
        return format!("JasperReports could not render it: {e}");
    }
    if let Some(e) = &c.ours_error {
        return format!("JasperReports rendered it ({} pages); ferroreport stopped: {e}", c.jasper_pages);
    }
    let rows = c.rows.as_deref().map_or("with no rows".to_string(), |r| format!("with the rows of {r}"));
    let text = if c.csv_diff.0 == 0 {
        "every text and number the same".to_string()
    } else {
        format!("{} of {} lines of text differ", c.csv_diff.0, c.csv_diff.1)
    };
    let plural = |n: usize| if n == 1 { "1 page".to_string() } else { format!("{n} pages") };
    let pages = if c.jasper_pages == 0 && c.ours_pages == 0 {
        "no pages in either (give it rows to compare more)".to_string()
    } else if c.jasper_pages == c.ours_pages {
        format!("{} in both", plural(c.ours_pages))
    } else {
        format!("JasperReports {}, ferroreport {}", plural(c.jasper_pages), plural(c.ours_pages))
    };
    let pictures = match (&c.pictures, &c.pictures_note) {
        // Blank pages (a report with no pages still has one in its PDF) say nothing.
        (Some(p), _) if p.worst_page.is_none() || (c.jasper_pages == 0 && c.ours_pages == 0) => String::new(),
        (Some(p), _) => {
            let worst =
                format!("the worst, page {}, differs in {:.2}% of its ink", p.worst_page.unwrap_or(0), p.worst_share * 100.0);
            if p.same() {
                format!(", every page the same picture ({worst})")
            } else {
                format!(", {} a different picture ({worst})", plural(p.pages_differing))
            }
        }
        (None, Some(note)) => format!(", pictures not compared ({note})"),
        (None, None) => String::new(),
    };
    format!("{rows}: {pages}, {text}{pictures}; JasperReports {:.0} ms, ferroreport {:.1} ms", c.jasper_ms, c.ours_ms)
}

/// A note that only says rows come from the caller, not from the template's query (one in a
/// language other than SQL: SQL queries run with `--db`).
fn about_data(warning: &str) -> bool {
    warning.contains("query is not run")
}

/// What `migrate` found in one template.
struct Checked {
    path: String,
    version: &'static str,
    name: String,
    /// The template cannot be loaded: why.
    error: Option<String>,
    warnings: Vec<String>,
    elements: std::collections::BTreeMap<String, usize>,
    /// Rendering it with no rows failed: why.
    render: Option<String>,
    converted: Option<String>,
    file: PathBuf,
    /// What JasperReports made of it next to ferroreport (`--jasper`).
    jasper: Option<Compared>,
}

/// One template rendered by both engines with the same rows.
struct Compared {
    /// JasperReports could not render it: why.
    jasper_error: Option<String>,
    /// ferroreport could not render it with those rows: why.
    ours_error: Option<String>,
    jasper_pages: usize,
    ours_pages: usize,
    /// Fill and PDF export, in milliseconds.
    jasper_ms: f64,
    ours_ms: f64,
    /// Lines of the CSV exports that differ, of the longer one.
    csv_diff: (usize, usize),
    rows: Option<String>,
    /// The pages compared pixel by pixel, as tools/compare.py does.
    pictures: Option<pixels::Pictures>,
    /// Why the pages were not compared pixel by pixel.
    pictures_note: Option<String>,
}

impl Compared {
    fn same(&self) -> bool {
        self.jasper_error.is_none() && self.ours_error.is_none() && self.jasper_pages == self.ours_pages && self.csv_diff.0 == 0
    }
}

/// The rows and parameters next to a template: `<name>.json` or `data.json`, and
/// `<name>.params.json` or `params.json`.
fn data_beside(file: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let dir = file.parent().unwrap_or(Path::new("."));
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let first = |names: [String; 2]| names.into_iter().map(|n| dir.join(n)).find(|p| p.is_file());
    (first([format!("{stem}.json"), "data.json".into()]), first([format!("{stem}.params.json"), "params.json".into()]))
}

/// Renders every template that loads with JasperReports (the jars in `lib`, run with the
/// `java` of the machine) and with ferroreport, from the same rows, and compares pages, the
/// CSV export (every text and number, in place), the pictures of the pages (with `pdftoppm`,
/// when it is on the PATH) and times.
fn compare_with_jasper(
    results: &mut [Checked],
    lib: &Path,
    resources: &[PathBuf],
    keep: Option<&Path>,
    base: &Path,
) -> CliResult {
    use std::io::{BufRead, Write};
    let work = std::env::temp_dir().join(format!("ferroreport-migrate-{}", std::process::id()));
    let _cleanup = RemoveDirOnDrop(work.clone());
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let program = work.join("MigrateCheck.java");
    std::fs::write(&program, include_str!("MigrateCheck.java")).map_err(|e| format!("{}: {e}", program.display()))?;
    let mut jobs = String::new();
    for (i, r) in results.iter().enumerate() {
        if r.error.is_some() {
            continue;
        }
        // Jasper writes its compiled subreports and both outputs here; the template's own
        // folder is only read.
        let dir = work.join(i.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let source_dir = r.file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let mut template = r.file.clone();
        if r.version == ".ferro" {
            // Jasper reads the same report as JRXML 7.
            let stem = r.file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            template = dir.join(format!("{stem}.jrxml"));
            let source = std::fs::read_to_string(&r.file).map_err(|e| format!("{}: {e}", r.file.display()))?;
            match ferro_to_jrxml(&source, source_dir) {
                Ok(jrxml) => std::fs::write(&template, jrxml).map_err(|e| format!("{}: {e}", template.display()))?,
                Err(_) => continue,
            }
        }
        let (rows, params) = data_beside(&r.file);
        let path = |p: &Option<PathBuf>| p.as_ref().map_or("-".to_string(), |p| p.to_string_lossy().into_owned());
        jobs.push_str(&format!(
            "{i}\t{}\t{}\t{}\t{}\t{}\n",
            template.display(),
            path(&rows),
            path(&params),
            dir.join("jasper").display(),
            source_dir.display()
        ));
    }
    // Rasterizing a PDF needs poppler's pdftoppm; without it the pictures are not compared.
    let rasterize = pixels::pdftoppm_available();
    if !rasterize {
        eprintln!("pdftoppm (poppler) is not on the PATH: the pages are not compared pixel by pixel");
    }
    let separator = if cfg!(windows) { ";" } else { ":" };
    let classpath = format!("{}{separator}{}", lib.join("*").display(), work.display());
    // UTC, as MigrateCheck's REPORT_TIME_ZONE and ferroreport's defaults: expressions that use
    // the runtime's own zone (new SimpleDateFormat(...), HOUR, DATEFORMAT...) would otherwise be
    // shifted in JasperReports only, on a machine outside UTC.
    let mut child = std::process::Command::new("java")
        .args(["-Djava.awt.headless=true", "-Duser.timezone=UTC", "-cp", &classpath])
        .arg(&program)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("--jasper needs Java 11 or newer (a JDK) on the PATH: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("java: no standard input")?;
    let feeder = std::thread::spawn(move || stdin.write_all(jobs.as_bytes()));
    let stdout = child.stdout.take().ok_or("java: no standard output")?;
    let mut answered = std::collections::HashMap::new();
    for line in std::io::BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("java: {e}"))?;
        let f: Vec<&str> = line.splitn(6, '\t').collect();
        if f.len() == 6 {
            if let Ok(i) = f[0].parse::<usize>() {
                answered.insert(
                    i,
                    (f[1] == "ok", f[2].parse::<usize>().unwrap_or(0), f[4].parse::<f64>().unwrap_or(0.0), f[5].to_string()),
                );
            }
        }
    }
    let _ = feeder.join();
    let mut stderr = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut e, &mut stderr);
    }
    let status = child.wait().map_err(|e| format!("java: {e}"))?;
    if answered.is_empty() && !status.success() {
        let why: Vec<&str> = stderr.lines().filter(|l| !l.starts_with("Picked up") && !l.trim().is_empty()).take(6).collect();
        return Err(format!("JasperReports could not be run from {}:\n{}", lib.display(), why.join("\n")).into());
    }
    for (i, r) in results.iter_mut().enumerate() {
        let Some((ok, pages, ms, message)) = answered.remove(&i) else { continue };
        let dir = work.join(i.to_string());
        let (rows_file, params_file) = data_beside(&r.file);
        let mut compared = Compared {
            jasper_error: (!ok).then_some(message),
            ours_error: None,
            jasper_pages: pages,
            ours_pages: 0,
            jasper_ms: ms,
            ours_ms: 0.0,
            csv_diff: (0, 0),
            rows: rows_file.as_ref().map(|p| p.to_string_lossy().into_owned()),
            pictures: None,
            pictures_note: None,
        };
        // ferroreport, from the same rows and with the same defaults.
        let source_dir = r.file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
        let mut renderer = Renderer::new().resource_dir(&source_dir);
        for res in resources {
            renderer = renderer.resource_dir(res);
        }
        let mut ours = || -> ferroreport::Result<(usize, f64)> {
            let source = std::fs::read_to_string(&r.file).map_err(|e| ferroreport::Error::Io(e.to_string()))?;
            let report = renderer.parse(&source)?;
            let params_json = match &params_file {
                Some(p) => serde_json::from_slice(&std::fs::read(p).map_err(|e| ferroreport::Error::Io(e.to_string()))?)
                    .map_err(|e| ferroreport::Error::Data(e.to_string()))?,
                None => serde_json::Value::Object(Default::default()),
            };
            let rows = |report: &ferroreport::Report| -> ferroreport::Result<ferroreport::RowIter<'static>> {
                Ok(match &rows_file {
                    Some(p) => Box::new(ferroreport::JsonRows::new(
                        report,
                        std::fs::File::open(p).map_err(|e| ferroreport::Error::Io(e.to_string()))?,
                    )),
                    None => Box::new(std::iter::empty()),
                })
            };
            let pdf = std::fs::File::create(dir.join("ferroreport.pdf")).map_err(|e| ferroreport::Error::Io(e.to_string()))?;
            let started = std::time::Instant::now();
            let params = ferroreport::params_from_json(&report, &params_json)?;
            let stats =
                renderer.render_to(&report, rows(&report)?, params, ferroreport::Format::Pdf, std::io::BufWriter::new(pdf))?;
            let ms = started.elapsed().as_secs_f64() * 1e3;
            let csv = std::fs::File::create(dir.join("ferroreport.csv")).map_err(|e| ferroreport::Error::Io(e.to_string()))?;
            let params = ferroreport::params_from_json(&report, &params_json)?;
            renderer.render_to(&report, rows(&report)?, params, ferroreport::Format::Csv, std::io::BufWriter::new(csv))?;
            Ok((stats.pages, ms))
        };
        match ours() {
            Ok((pages, ms)) => {
                compared.ours_pages = pages;
                compared.ours_ms = ms;
            }
            Err(e) => compared.ours_error = Some(e.to_string()),
        }
        if compared.jasper_error.is_none() && compared.ours_error.is_none() {
            let theirs = std::fs::read_to_string(dir.join("jasper.csv")).unwrap_or_default();
            let mine = std::fs::read_to_string(dir.join("ferroreport.csv")).unwrap_or_default();
            let (a, b): (Vec<&str>, Vec<&str>) = (theirs.lines().collect(), mine.lines().collect());
            let n = a.len().max(b.len());
            let differ = (0..n).filter(|&k| a.get(k) != b.get(k)).count();
            compared.csv_diff = (differ, n);
        }
        // Where the template is under the folder given to migrate, in --keep.
        let target = keep.map(|keep| {
            let stem = r.file.with_extension("");
            let rel = stem.strip_prefix(base).unwrap_or(&stem).to_path_buf();
            let rel = rel.strip_prefix("/").unwrap_or(&rel).to_path_buf();
            let target = keep.join(rel);
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            target
        });
        if compared.jasper_error.is_none() && compared.ours_error.is_none() {
            if !rasterize {
                compared.pictures_note = Some("pdftoppm is not on the PATH".into());
            } else {
                match pixels::compare_pdfs(&dir.join("jasper.pdf"), &dir.join("ferroreport.pdf"), &dir, target.as_deref()) {
                    Ok(p) => compared.pictures = Some(p),
                    Err(e) => compared.pictures_note = Some(e),
                }
            }
        }
        if let Some(target) = target {
            for (from, suffix) in [
                ("jasper.pdf", "jasper.pdf"),
                ("ferroreport.pdf", "ferroreport.pdf"),
                ("jasper.csv", "jasper.csv"),
                ("ferroreport.csv", "ferroreport.csv"),
            ] {
                let mut name = target.as_os_str().to_os_string();
                name.push(format!(".{suffix}"));
                let _ = std::fs::copy(dir.join(from), PathBuf::from(name));
            }
        }
        r.jasper = Some(compared);
    }
    Ok(())
}

struct RemoveDirOnDrop(PathBuf);

impl Drop for RemoveDirOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn find_templates(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else { return };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if p.is_dir() {
            find_templates(&p, out);
        } else if matches!(p.extension().and_then(|e| e.to_str()), Some("jrxml" | "ferro")) {
            out.push(p);
        }
    }
}

fn migrate(args: &[String]) -> CliResult {
    let mut inputs = Vec::new();
    let mut report_file = None;
    let mut to = None;
    let mut out_dir = None;
    let mut resources: Vec<PathBuf> = Vec::new();
    let mut strict = false;
    let mut strict_pixels = false;
    let mut jasper: Option<PathBuf> = None;
    let mut keep: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--report" => report_file = it.next().cloned(),
            "--to" => to = it.next().cloned(),
            "--out" => out_dir = it.next().cloned(),
            "--resources" => resources.push(it.next().ok_or("--resources needs a folder")?.into()),
            "--strict" => strict = true,
            "--strict-pixels" => strict_pixels = true,
            "--jasper" => jasper = Some(it.next().ok_or("--jasper needs the folder of the JasperReports jars")?.into()),
            "--keep" => keep = Some(it.next().ok_or("--keep needs a folder")?.into()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}").into()),
            s => inputs.push(PathBuf::from(s)),
        }
    }
    if inputs.is_empty() {
        return Err("migrate needs a folder or templates: ferroreport migrate reports/".into());
    }
    if strict_pixels && jasper.is_none() {
        return Err("--strict-pixels compares pages with JasperReports': it needs --jasper <lib>".into());
    }
    if to.is_some() != out_dir.is_some() {
        return Err("--to and --out go together: ferroreport migrate reports/ --to ferro --out converted/".into());
    }
    let mut files = Vec::new();
    for input in &inputs {
        find_templates(input, &mut files);
    }
    let base = inputs.first().filter(|p| p.is_dir()).cloned().unwrap_or_default();
    let mut results = Vec::with_capacity(files.len());
    for file in &files {
        let path = file.to_string_lossy().into_owned();
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                results.push(Checked {
                    path,
                    version: "?",
                    name: String::new(),
                    error: Some(e.to_string()),
                    warnings: Vec::new(),
                    elements: Default::default(),
                    render: None,
                    converted: None,
                    file: file.clone(),
                    jasper: None,
                });
                continue;
            }
        };
        let version = if ferroreport::template::is_template(&source) {
            ".ferro"
        } else {
            match ferroreport::convert::jrxml_version(&source) {
                Ok(ferroreport::convert::JrxmlVersion::V6) => "JRXML 6",
                Ok(ferroreport::convert::JrxmlVersion::V7) => "JRXML 7",
                Err(_) => "?",
            }
        };
        let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        // Projects often keep pictures next to the folder of their templates.
        let mut renderer = Renderer::new().resource_dir(dir);
        if let Some(parent) = dir.parent().filter(|d| !d.as_os_str().is_empty()) {
            renderer = renderer.resource_dir(parent);
        }
        for r in &resources {
            renderer = renderer.resource_dir(r);
        }
        let font_problems = renderer.load_font_extensions();
        let mut renderer =
            renderer.limits(ferroreport::Limits { max_pages: Some(50), timeout: Some(std::time::Duration::from_secs(10)) });
        let mut checked = Checked {
            path,
            version,
            name: String::new(),
            error: None,
            warnings: Vec::new(),
            elements: Default::default(),
            render: None,
            converted: None,
            file: file.clone(),
            jasper: None,
        };
        match renderer.parse(&source) {
            Err(e) => checked.error = Some(e.to_string()),
            Ok(report) => {
                checked.name = report.name.clone();
                checked.warnings = report.warnings.clone();
                checked.warnings.extend(font_problems);
                let bands = [
                    &report.background,
                    &report.title,
                    &report.page_header,
                    &report.column_header,
                    &report.column_footer,
                    &report.page_footer,
                    &report.last_page_footer,
                    &report.summary,
                    &report.no_data,
                ];
                for b in bands.into_iter().flatten().chain(report.detail.iter()) {
                    count_elements(b, &mut checked.elements);
                }
                for g in &report.groups {
                    for b in g.header.iter().chain(&g.footer) {
                        count_elements(b, &mut checked.elements);
                    }
                }
                // With no rows, every expression of the title, page bands and summary runs.
                let empty: ferroreport::RowIter = Box::new(std::iter::empty());
                if let Err(e) = renderer.render_to(&report, empty, Default::default(), ferroreport::Format::Pdf, std::io::sink())
                {
                    checked.render = Some(e.to_string());
                }
            }
        }
        if let (Some(to), Some(out)) = (&to, &out_dir) {
            let converted = match to.as_str() {
                "ferro" if version == ".ferro" => Ok(source.clone()),
                "ferro" => ferroreport::template::from_jrxml(&source),
                "jrxml" if version == ".ferro" => ferro_to_jrxml(&source, file.parent().unwrap_or(Path::new("."))),
                "jrxml" => ferroreport::convert::jrxml_to_v7(&source),
                other => return Err(format!("--to {other}: say ferro or jrxml").into()),
            };
            match converted {
                Ok(text) => {
                    let rel = file.strip_prefix(&base).unwrap_or(file);
                    let target = Path::new(out).join(rel).with_extension(if to == "ferro" { "ferro" } else { "jrxml" });
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                    std::fs::write(&target, text).map_err(|e| format!("{}: {e}", target.display()))?;
                    checked.converted = Some(target.to_string_lossy().into_owned());
                }
                Err(e) => {
                    checked.error.get_or_insert_with(|| format!("cannot be converted: {e}"));
                }
            }
        }
        results.push(checked);
    }
    if let Some(lib) = &jasper {
        eprintln!(
            "rendering {} templates with JasperReports and ferroreport...",
            results.iter().filter(|r| r.error.is_none()).count()
        );
        compare_with_jasper(&mut results, lib, &resources, keep.as_deref(), &base)?;
    }
    let failed = results.iter().filter(|r| r.error.is_some()).count();
    let partial = results.iter().filter(|r| r.error.is_none() && r.warnings.iter().any(|w| !about_data(w))).count();
    let data_only = results
        .iter()
        .filter(|r| r.error.is_none() && !r.warnings.is_empty() && r.warnings.iter().all(|w| about_data(w)))
        .count();
    let render_fail = results.iter().filter(|r| r.error.is_none() && r.render.is_some()).count();
    let ready = results.len() - failed - partial - data_only;
    let status = |r: &Checked| {
        if r.error.is_some() {
            ("✗", "cannot be loaded")
        } else if r.warnings.iter().any(|w| !about_data(w)) {
            ("!", "partly supported")
        } else if !r.warnings.is_empty() {
            ("✓", "supported; its rows come from you")
        } else {
            ("✓", "supported")
        }
    };
    for r in &results {
        let (mark, _) = status(r);
        println!("{mark} {} ({})", r.path, r.version);
        if let Some(e) = &r.error {
            println!("    cannot be loaded: {e}");
        }
        for w in &r.warnings {
            println!("    - {w}");
        }
        if let Some(e) = &r.render {
            println!("    with no rows: {e}");
        }
        if let Some(c) = &r.jasper {
            println!("    {}", describe(c));
        }
    }
    println!(
        "\n{} templates: {ready} supported, {data_only} supported once you pass the rows their non-SQL query gave, {partial} use features not supported yet, {failed} cannot be loaded; {render_fail} stop when rendered with no rows (they may need rows, parameters or pictures)",
        results.len()
    );
    let compared: Vec<&Compared> = results.iter().filter_map(|r| r.jasper.as_ref()).collect();
    // The page that differs most of all, in the share of its ink tools/compare.py gives.
    let pictured: Vec<(&Checked, &pixels::Pictures)> =
        results.iter().filter_map(|r| Some((r, r.jasper.as_ref()?.pictures.as_ref()?))).collect();
    let worst =
        pictured.iter().filter(|(_, p)| p.worst_page.is_some()).max_by(|a, b| a.1.worst_share.total_cmp(&b.1.worst_share));
    let pictures_differ = pictured.iter().filter(|(_, p)| !p.same()).count();
    let pictures_summary = if let Some((r, p)) = worst {
        Some(format!(
            "pictures, pixel by pixel at {} DPI: {pictures_differ} of {} templates differ in more than {:.2}% of a page's ink; the worst page is page {} of {}, {:.2}% of its ink",
            pixels::DPI,
            pictured.len(),
            pixels::MATCH_THRESHOLD * 100.0,
            p.worst_page.unwrap_or(0),
            r.path,
            p.worst_share * 100.0
        ))
    } else if pictured.is_empty() {
        compared.iter().find_map(|c| c.pictures_note.as_ref()).map(|note| format!("pictures not compared: {note}"))
    } else {
        None
    };
    if jasper.is_some() {
        let same = compared.iter().filter(|c| c.same()).count();
        let jasper_failed = compared.iter().filter(|c| c.jasper_error.is_some()).count();
        let differ = compared.len() - same - jasper_failed;
        let (theirs, ours): (f64, f64) = compared
            .iter()
            .filter(|c| c.jasper_error.is_none() && c.ours_error.is_none())
            .fold((0.0, 0.0), |(a, b), c| (a + c.jasper_ms, b + c.ours_ms));
        println!(
            "compared with JasperReports: {same} the same pages and text, {differ} differ, {jasper_failed} JasperReports could not render; filling and writing the PDFs took JasperReports {:.0} ms in all (its first template includes starting up), ferroreport {:.0} ms",
            theirs,
            ours
        );
        if let Some(line) = &pictures_summary {
            println!("{line}");
        }
    }
    if let Some(file) = report_file {
        let text = if file.ends_with(".json") {
            let list: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "path": r.path, "version": r.version, "name": r.name,
                        "loads": r.error.is_none(), "error": r.error,
                        "unsupported": r.warnings, "elements": r.elements,
                        "rendersWithoutRows": r.error.is_none() && r.render.is_none(), "renderError": r.render,
                        "converted": r.converted,
                        "jasper": r.jasper.as_ref().map(|c| serde_json::json!({
                            "same": c.same(), "rows": c.rows,
                            "jasperError": c.jasper_error, "ferroreportError": c.ours_error,
                            "jasperPages": c.jasper_pages, "ferroreportPages": c.ours_pages,
                            "jasperMs": (c.jasper_ms * 10.0).round() / 10.0, "ferroreportMs": (c.ours_ms * 10.0).round() / 10.0,
                            "csvLinesDiffering": c.csv_diff.0, "csvLines": c.csv_diff.1,
                            "pixelsCompared": c.pictures.is_some(), "pixelsNote": c.pictures_note,
                            "worstPage": c.pictures.as_ref().and_then(|p| p.worst_page),
                            "worstPageDiff": c.pictures.as_ref().map(|p| (p.worst_share * 1e5).round() / 1e5),
                            "pagesDiffering": c.pictures.as_ref().map(|p| p.pages_differing),
                            "diffImages": c.pictures.as_ref().map_or(Vec::new(), |p| p.diff_files.clone()),
                        })),
                    })
                })
                .collect();
            let pictures = (!pictured.is_empty()).then(|| {
                serde_json::json!({
                    "dpi": pixels::DPI, "threshold": pixels::MATCH_THRESHOLD,
                    "compared": pictured.len(), "differ": pictures_differ,
                    "worstTemplate": worst.map(|(r, _)| &r.path),
                    "worstPage": worst.and_then(|(_, p)| p.worst_page),
                    "worstPageDiff": worst.map(|(_, p)| (p.worst_share * 1e5).round() / 1e5),
                })
            });
            serde_json::to_string_pretty(&serde_json::json!({
                "templates": results.len(), "supported": ready, "supportedWithRows": data_only, "partial": partial, "failed": failed,
                "pixels": pictures, "results": list
            }))?
        } else {
            let mut md = format!(
                "# Migration report\n\n{} templates: **{ready}** supported, **{data_only}** supported once you pass the rows their non-SQL query gave, **{partial}** use features not supported yet, **{failed}** cannot be loaded.\n\n| Template | Format | Result | Notes |\n|---|---|---|---|\n",
                results.len()
            );
            if let Some(line) = &pictures_summary {
                // After the first paragraph, before the table.
                let at = md.find("\n\n| Template").unwrap_or(md.len());
                md.insert_str(at, &format!("\n\n{}.", capitalize(line)));
            }
            for r in &results {
                let (_, result) = status(r);
                let mut notes: Vec<String> = r.error.iter().cloned().collect();
                notes.extend(r.warnings.iter().cloned());
                if let Some(e) = &r.render {
                    notes.push(format!("with no rows: {e}"));
                }
                if let Some(c) = &r.jasper {
                    notes.push(describe(c));
                }
                let notes = notes.join("; ").replace('|', "\\|").replace('\n', " ");
                md.push_str(&format!("| `{}` | {} | {result} | {notes} |\n", r.path, r.version));
            }
            md
        };
        std::fs::write(&file, text).map_err(|e| format!("{file}: {e}"))?;
        println!("report written to {file}");
    }
    if failed > 0 {
        return Err(format!("{failed} template(s) cannot be loaded").into());
    }
    let different = compared.iter().filter(|c| !c.same() && c.jasper_error.is_none()).count();
    if strict && (partial > 0 || render_fail > 0 || different > 0) {
        return Err(format!(
            "--strict: {partial} template(s) use features not supported yet, {render_fail} stop when rendered with no rows, {different} differ from JasperReports"
        )
        .into());
    }
    if strict_pixels {
        let checked: Vec<(&str, &Compared)> =
            results.iter().filter_map(|r| Some((r.path.as_str(), r.jasper.as_ref()?))).collect();
        strict_pixels_check(&checked)?;
    }
    Ok(())
}

/// What `--strict-pixels` says of the templates compared with JasperReports: `Err` with the
/// templates whose pages are a different picture (each with its worst page and share), or with
/// why the pictures could not be compared. A template either engine could not render is left
/// to the other checks.
fn strict_pixels_check(compared: &[(&str, &Compared)]) -> Result<(), String> {
    let rendered = || compared.iter().filter(|(_, c)| c.jasper_error.is_none() && c.ours_error.is_none());
    let not_compared: Vec<String> = rendered()
        .filter(|(_, c)| c.pictures.is_none())
        .map(|(path, c)| format!("{path}: {}", c.pictures_note.as_deref().unwrap_or("not compared")))
        .collect();
    if !not_compared.is_empty() {
        return Err(format!(
            "--strict-pixels: the pages of {} template(s) could not be compared pixel by pixel (pdftoppm, from poppler, must be on the PATH):\n  {}",
            not_compared.len(),
            not_compared.join("\n  ")
        ));
    }
    let differing: Vec<String> = rendered()
        .filter_map(|(path, c)| {
            let p = c.pictures.as_ref()?;
            (p.pages_differing > 0).then(|| {
                let page = p.worst_page.map_or(String::new(), |n| format!("page {n}, "));
                format!("{path}: {} page(s) differ; the worst, {page}{:.2}% of its ink", p.pages_differing, p.worst_share * 100.0)
            })
        })
        .collect();
    if differing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "--strict-pixels: {} template(s) have pages that are a different picture (over {:.2}% of a page's ink):\n  {}",
            differing.len(),
            pixels::MATCH_THRESHOLD * 100.0,
            differing.join("\n  ")
        ))
    }
}

/// The MIME type of an attached file, by its extension.
fn guess_mime(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("xml") => "text/xml",
        Some("json") => "application/json",
        Some("csv") => "text/csv",
        Some("txt") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("ods") => "application/vnd.oasis.opendocument.spreadsheet",
        _ => "application/octet-stream",
    }
}

fn convert(args: &[String]) -> CliResult {
    let mut input = None;
    let mut output = None;
    let mut to = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-o" | "--output" => output = it.next().cloned(),
            "-t" | "--to" => to = it.next().cloned(),
            s if s.starts_with('-') && s != "-" => return Err(format!("unknown option {s}").into()),
            s => input = Some(s.to_string()),
        }
    }
    let input = input.ok_or("convert needs a template: ferroreport convert <template> [-o <output>] [--to ferro|jrxml]")?;
    let source = if input == "-" {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(|e| format!("stdin: {e}"))?;
        s
    } else {
        std::fs::read_to_string(&input).map_err(|e| format!("{input}: {e}"))?
    };
    let from_template = ferroreport::template::is_template(&source);
    // What to write: --to, else the output's extension, else the other format.
    let extension =
        output.as_deref().and_then(|o| Path::new(o).extension()).and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let to = match (to.as_deref(), extension.as_deref()) {
        (Some("ferro"), _) | (None, Some("ferro")) => "ferro",
        (Some("jrxml" | "jrtx"), _) | (None, Some("jrxml" | "jrtx" | "xml")) => "jrxml",
        (Some(other), _) => return Err(format!("--to {other}: say ferro or jrxml").into()),
        (None, _) if from_template => "jrxml",
        (None, _) => "ferro",
    };
    let input_dir = Path::new(&input).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let converted = match (from_template, to) {
        (true, "jrxml") => ferro_to_jrxml(&source, input_dir)?,
        (true, _) => ferroreport::template::from_jrxml(&ferro_to_jrxml(&source, input_dir)?)?,
        (false, "jrxml") => ferroreport::convert::jrxml_to_v7(&source)?,
        (false, _) => ferroreport::template::from_jrxml(&source)?,
    };
    match output.as_deref() {
        None | Some("-") => print!("{converted}"),
        Some(path) => std::fs::write(path, converted).map_err(|e| format!("{path}: {e}"))?,
    }
    Ok(())
}

fn render(args: &[String]) -> CliResult {
    let mut template = None;
    let mut data = None;
    let mut params = None;
    let mut output = None;
    let mut locale = "en_US".to_string();
    let mut fonts = Vec::new();
    let mut font_sets = Vec::new();
    let mut font_fallback = false;
    let mut now = None;
    let mut quiet = false;
    let mut resources = Vec::new();
    let mut format = None;
    let mut xlsx_numbers = false;
    let mut csv_safe = false;
    let mut tagged = false;
    let mut pdfa = None;
    let mut max_row = ferroreport::data::MAX_ROW_BYTES;
    let mut user_password = std::env::var("FERROREPORT_PDF_USER_PASSWORD").ok();
    let mut owner_password = std::env::var("FERROREPORT_PDF_OWNER_PASSWORD").ok();
    let mut permissions: Option<String> = None;
    let mut attachments = Vec::new();
    let mut limits = ferroreport::Limits::default();
    let mut max_rows = None;
    let mut db = std::env::var("FERROREPORT_DB").ok().filter(|u| !u.is_empty());
    let mut sign = SignOptions::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        if sign.option(a, &mut value)? {
            continue;
        }
        match a.as_str() {
            "-d" | "--data" => data = Some(value(a)?),
            "--db" => db = Some(value(a)?),
            "-f" | "--format" => format = Some(value(a)?.to_ascii_lowercase()),
            "--xlsx-numbers" => xlsx_numbers = true,
            "--csv-safe" => csv_safe = true,
            "--tagged" => tagged = true,
            "--pdfa" => {
                let v = value(a)?;
                pdfa =
                    Some(ferroreport::pdf::PdfA::parse(&v).ok_or_else(|| format!("--pdfa {v}: say 2b, 2u, 2a, 3b, 3u or 3a"))?);
            }
            "--attach" => attachments.push(value(a)?),
            "--user-password" => user_password = Some(value(a)?),
            "--owner-password" => owner_password = Some(value(a)?),
            "--permissions" => permissions = Some(value(a)?),
            "--max-row" => {
                let mb: f64 = value(a)?.parse().map_err(|_| "--max-row expects megabytes")?;
                max_row = (mb.max(0.001) * 1048576.0) as usize;
            }
            "--max-pages" => limits.max_pages = Some(value(a)?.parse().map_err(|_| "--max-pages expects a number")?),
            "--max-rows" => max_rows = Some(value(a)?.parse().map_err(|_| "--max-rows expects a number")?),
            "--timeout" => {
                let secs: f64 = value(a)?.parse().map_err(|_| "--timeout expects seconds")?;
                limits.timeout = Some(std::time::Duration::from_secs_f64(secs.max(0.0)));
            }
            "-p" | "--params" => params = Some(value(a)?),
            "-o" | "--output" => output = Some(value(a)?),
            "-l" | "--locale" => locale = value(a)?,
            "--font" => fonts.push(value(a)?),
            "--font-set" => font_sets.push(value(a)?),
            "--font-fallback" => font_fallback = true,
            "--now" => now = Some(value(a)?),
            "-q" | "--quiet" => quiet = true,
            "-r" | "--resources" => resources.push(value(a)?),
            s if s.starts_with('-') => return Err(format!("unknown option {s}").into()),
            s => template = Some(s.to_string()),
        }
    }
    let template = template.ok_or("missing template path")?;
    let format = match format {
        Some(f) => ferroreport::Format::parse(&f)
            .ok_or_else(|| format!("unknown format {f} (pdf, xlsx, docx, pptx, ods, odt, csv or html)"))?,
        None => output
            .as_deref()
            .and_then(|o| PathBuf::from(o).extension().and_then(|e| ferroreport::Format::parse(&e.to_string_lossy())))
            .unwrap_or_default(),
    };
    let output = output.map(PathBuf::from).unwrap_or_else(|| PathBuf::from(&template).with_extension(format.extension()));

    let started = Instant::now();
    let source = std::fs::read_to_string(&template).map_err(|e| format!("{template}: {e}"))?;
    let params_json = match &params {
        Some(p) => read_json(p)?,
        None => serde_json::Value::Object(Default::default()),
    };
    let template_dir = PathBuf::from(&template).parent().map(PathBuf::from).unwrap_or_default();
    let template_dir = if template_dir.as_os_str().is_empty() { PathBuf::from(".") } else { template_dir };
    let mut renderer = Renderer::new()
        .locale(&locale)
        .resource_dir(template_dir)
        .limits(limits)
        .max_rows(max_rows)
        .xlsx_options(ferroreport::xlsx::XlsxOptions { detect_cell_type: xlsx_numbers })
        .tagged(tagged)
        .pdfa(pdfa)
        .csv_options(ferroreport::csv::CsvOptions { guard_formulas: csv_safe, ..Default::default() });
    if user_password.is_some() || owner_password.is_some() || permissions.is_some() {
        let permissions = ferroreport::crypt::Permissions::parse(permissions.as_deref().unwrap_or(""))?;
        renderer = renderer.encrypt(Some(ferroreport::crypt::Encryption {
            user_password: user_password.unwrap_or_default(),
            owner_password,
            permissions,
        }));
    }
    for spec in &attachments {
        // file[:mime type]
        let (path, mime) = match spec.rsplit_once(':').filter(|(_, m)| m.contains('/')) {
            Some((p, m)) => (p, m.to_string()),
            None => (spec.as_str(), guess_mime(spec).to_string()),
        };
        let data = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
        renderer = renderer.attach(ferroreport::pdf::Attachment {
            name,
            data: std::sync::Arc::new(data),
            mime,
            description: None,
            relationship: "Data".into(),
        });
    }
    for dir in &resources {
        renderer = renderer.resource_dir(dir);
    }
    // Font extensions in the resource folders, as JasperReports finds them on its class path.
    for problem in renderer.load_font_extensions() {
        eprintln!("warning: {problem}");
    }
    if let Some(url) = &db {
        renderer = renderer.connection(Some(connect_db(url)?));
    }
    renderer = renderer.sign(sign.signature()?);
    // Parsed here, where the resource folders are known: style templates are read from them.
    let report = renderer.parse(&source)?;
    // Without rows of its own, a report with a query is filled from the database.
    let from_db = db.is_some() && data.is_none() && report.query.is_some();
    if let Some(n) = now {
        let dt = ferroreport::data::parse_datetime(&n).ok_or("--now expects YYYY-MM-DDTHH:MM:SS")?;
        renderer = renderer.now(dt);
    }
    for spec in &fonts {
        let (family, rest) = spec.split_once('=').ok_or("--font expects Family=file.ttf")?;
        let mut parts = rest.split(':');
        let path = parts.next().unwrap_or_default();
        let flags: Vec<&str> = parts.collect();
        let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        renderer.register_font(family, flags.contains(&"bold"), flags.contains(&"italic"), bytes)?;
    }
    for spec in &font_sets {
        let (name, families) = spec.split_once('=').ok_or("--font-set expects Name=Family,Family")?;
        let families: Vec<&str> = families.split(',').map(str::trim).filter(|f| !f.is_empty()).collect();
        renderer.register_font_set(name, &families)?;
    }
    renderer.set_font_fallback(font_fallback);
    let params = ferroreport::params_from_json(&report, &params_json)?;
    let parsed = started.elapsed();
    // Rows are read as they are needed and pages are written as they are finished, so the
    // size of the data does not decide how much memory the render takes.
    // A large workbook or OpenDocument file reads its rows more than once (see
    // `Renderer::render_rows_to`), so rows from standard input are put in a file first for
    // those formats.
    let mut kept = None;
    let data = match data.as_deref() {
        Some("-") if matches!(format, ferroreport::Format::Xlsx | ferroreport::Format::Ods | ferroreport::Format::Odt) => {
            let path = std::env::temp_dir().join(format!("ferroreport-rows-{}.json", std::process::id()));
            let keep = RemoveOnDrop(path.clone());
            let mut file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            std::io::copy(&mut std::io::stdin().lock(), &mut file).map_err(|e| format!("stdin: {e}"))?;
            kept = Some(keep);
            Some(path.to_string_lossy().into_owned())
        }
        other => other.map(str::to_string),
    };
    let mut rows = || -> ferroreport::Result<ferroreport::RowIter> {
        Ok(match data.as_deref() {
            Some("-") => Box::new(ferroreport::JsonRows::new(&report, std::io::stdin().lock()).max_row_bytes(max_row)),
            Some(path) => {
                let file = std::fs::File::open(path).map_err(|e| ferroreport::Error::Io(format!("{path}: {e}")))?;
                Box::new(ferroreport::JsonRows::new(&report, file).max_row_bytes(max_row))
            }
            None => Box::new(std::iter::empty()),
        })
    };
    let to_stdout = output.as_os_str() == "-";
    let stats = if to_stdout {
        quiet = true;
        let out = std::io::BufWriter::new(std::io::stdout().lock());
        if from_db {
            renderer.render_query_to(&report, params, format, out)?
        } else {
            renderer.render_rows_to(&report, &mut rows, params, format, out)?
        }
    } else {
        // Written beside the output and moved into place, so a failed render leaves no
        // half-written file behind.
        let mut name = output.file_name().unwrap_or_default().to_os_string();
        name.push(".part");
        let part = output.with_file_name(name);
        let file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", output.display()))?;
        let out = std::io::BufWriter::with_capacity(1 << 16, file);
        let result = if from_db {
            renderer.render_query_to(&report, params, format, out)
        } else {
            renderer.render_rows_to(&report, &mut rows, params, format, out)
        };
        match result {
            Ok(stats) => {
                std::fs::rename(&part, &output).map_err(|e| format!("{}: {e}", output.display()))?;
                stats
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                return Err(e.into());
            }
        }
    };
    let total = started.elapsed();
    drop(kept);

    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    for (from, to) in renderer.font_substitutions() {
        eprintln!("warning: font `{from}` is not available, using `{to}` (register it with --font)");
    }
    if !quiet {
        eprintln!(
            "{} -> {} ({} pages, {} rows) parse {:.1} ms, render {:.1} ms",
            template,
            output.display(),
            stats.pages,
            stats.rows,
            parsed.as_secs_f64() * 1e3,
            (total - parsed).as_secs_f64() * 1e3,
        );
    }
    Ok(())
}

/// A `.ferro` template as JRXML, its `import`s read from `dir`.
fn ferro_to_jrxml(source: &str, dir: &Path) -> ferroreport::Result<String> {
    let load = |name: &str| -> Option<Vec<u8>> {
        let path = dir.join(name);
        // Only below the template's folder.
        let inside = path.canonicalize().ok()?.starts_with(dir.canonicalize().ok()?);
        inside.then(|| std::fs::read(&path).ok()).flatten()
    };
    ferroreport::template::to_jrxml_with(source, Some(&load))
}

/// `render --watch`: renders, then again whenever the template, its folder (subreports, images,
/// style templates), the rows, the parameters or a resource folder change.
fn watch(args: &[String]) -> CliResult {
    let args: Vec<String> = args.iter().filter(|a| *a != "--watch" && *a != "-w").cloned().collect();
    let mut watched: Vec<PathBuf> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-d" | "--data" | "-p" | "--params" | "-r" | "--resources" => watched.extend(it.next().map(PathBuf::from)),
            "-o" | "--output" | "-f" | "--format" | "-l" | "--locale" | "--font" | "--font-set" | "--now" | "--db" | "--pdfa"
            | "--attach" | "--max-row" | "--max-pages" | "--max-rows" | "--timeout" | "--user-password" | "--owner-password"
            | "--permissions" => {
                it.next();
            }
            s if s.starts_with("--sign-") => {
                it.next();
            }
            s if !s.starts_with('-') => {
                let template = PathBuf::from(s);
                watched.push(template.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf());
                watched.push(template);
            }
            _ => {}
        }
    }
    watched.retain(|p| p.as_os_str() != "-");
    // The newest modification time among the files watched (a folder's direct files).
    let stamp = |paths: &[PathBuf]| -> Option<std::time::SystemTime> {
        let mut newest = None;
        let mut see = |p: &Path| {
            if let Ok(t) = std::fs::metadata(p).and_then(|m| m.modified()) {
                newest = newest.max(Some(t));
            }
        };
        for p in paths {
            see(p);
            if p.is_dir() {
                for entry in std::fs::read_dir(p).into_iter().flatten().flatten() {
                    let path = entry.path();
                    if path.is_file() && !path.to_string_lossy().ends_with(".part") {
                        see(&path);
                    }
                }
            }
        }
        newest
    };
    eprintln!("watching {} (Ctrl+C to stop)", watched.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "));
    let mut last = None;
    loop {
        let now = stamp(&watched);
        if now != last {
            last = now;
            if let Err(e) = render(&args) {
                eprintln!("error: {e}");
            }
            // What the render wrote (the output, when it is beside the template) is not a change.
            last = stamp(&watched).max(last);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

/// `--sign-key`, `--sign-cert` and the signature's details.
#[derive(Default)]
struct SignOptions {
    key: Option<String>,
    certs: Vec<String>,
    name: Option<String>,
    reason: Option<String>,
    location: Option<String>,
    contact: Option<String>,
}

impl SignOptions {
    /// Takes `--sign-*` options; false for any other.
    fn option(&mut self, a: &str, value: &mut dyn FnMut(&str) -> Result<String, String>) -> Result<bool, String> {
        match a {
            "--sign-key" => self.key = Some(value(a)?),
            "--sign-cert" => self.certs.push(value(a)?),
            "--sign-name" => self.name = Some(value(a)?),
            "--sign-reason" => self.reason = Some(value(a)?),
            "--sign-location" => self.location = Some(value(a)?),
            "--sign-contact" => self.contact = Some(value(a)?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn signature(self) -> Result<Option<ferroreport::sign::Signature>, Box<dyn std::error::Error>> {
        let Some(key) = self.key.or_else(|| std::env::var("FERROREPORT_SIGN_KEY").ok().filter(|k| !k.is_empty())) else {
            if !self.certs.is_empty() {
                return Err("--sign-cert needs --sign-key".into());
            }
            return Ok(None);
        };
        let mut certs = self.certs;
        if certs.is_empty() {
            certs.extend(std::env::var("FERROREPORT_SIGN_CERT").ok().filter(|c| !c.is_empty()));
        }
        Ok(Some(ferroreport::sign::Signature {
            signer: signing::load(&key, &certs)?,
            name: self.name,
            reason: self.reason,
            location: self.location,
            contact: self.contact,
        }))
    }
}

/// Opens the database a report's queries run on (`--db`).
#[cfg(feature = "db")]
fn connect_db(url: &str) -> Result<std::sync::Arc<dyn ferroreport::sql::Connection>, Box<dyn std::error::Error>> {
    Ok(db::connect(url)?)
}

#[cfg(not(feature = "db"))]
fn connect_db(_: &str) -> Result<std::sync::Arc<dyn ferroreport::sql::Connection>, Box<dyn std::error::Error>> {
    Err("this build has no database drivers (build with --features db)".into())
}

fn serve(args: &[String]) -> CliResult {
    let mut host = std::env::var("FERROREPORT_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let mut port = std::env::var("FERROREPORT_PORT").unwrap_or_else(|_| "8080".into());
    let mut templates = std::env::var("FERROREPORT_TEMPLATES").ok().map(PathBuf::from);
    let mut resources = Vec::new();
    let mut token = std::env::var("FERROREPORT_TOKEN").ok().filter(|t| !t.is_empty());
    let mut max_body_mb: usize = 50;
    let mut workers = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    // A request that runs for a minute is almost always a mistake or an attack.
    let mut limits = ferroreport::Limits { max_pages: None, timeout: Some(std::time::Duration::from_secs(60)) };
    let mut max_rows = None;
    let mut cache = 128usize;
    // `FERROREPORT_ACCESS_LOG=json|text` turns the log on; `--access-log` alone keeps its format.
    let mut access_log = match std::env::var("FERROREPORT_ACCESS_LOG").ok().filter(|v| !v.is_empty()) {
        Some(v) => Some(serve::AccessLog::parse(&v).ok_or_else(|| format!("FERROREPORT_ACCESS_LOG is `{v}`: say json or text"))?),
        None => None,
    };
    let mut allow_anonymous = std::env::var("FERROREPORT_ALLOW_ANONYMOUS").is_ok_and(|v| !v.is_empty() && v != "0");
    let mut rate_limit = None;
    let megabytes = |name: &str, default: u64| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(default);
    let mut spool_memory = megabytes("FERROREPORT_SPOOL_MEMORY", 32);
    let mut spool_disk = megabytes("FERROREPORT_SPOOL_DISK", 4096);
    let mut spool_dir = std::env::var("FERROREPORT_SPOOL_DIR").ok().filter(|d| !d.is_empty()).map(PathBuf::from);
    let mut db = std::env::var("FERROREPORT_DB").ok().filter(|u| !u.is_empty());
    let mut sign = SignOptions::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        if sign.option(a, &mut value)? {
            continue;
        }
        match a.as_str() {
            "--host" => host = value(a)?,
            "-P" | "--port" => port = value(a)?,
            "-t" | "--templates" => templates = Some(PathBuf::from(value(a)?)),
            "-r" | "--resources" => resources.push(PathBuf::from(value(a)?)),
            "--token" => token = Some(value(a)?),
            "--max-body" => max_body_mb = value(a)?.parse().map_err(|_| "--max-body expects megabytes")?,
            "--workers" => workers = value(a)?.parse().map_err(|_| "--workers expects a number")?,
            "--max-pages" => limits.max_pages = Some(value(a)?.parse().map_err(|_| "--max-pages expects a number")?),
            "--max-rows" => max_rows = Some(value(a)?.parse().map_err(|_| "--max-rows expects a number")?),
            "--timeout" => {
                let secs: f64 = value(a)?.parse().map_err(|_| "--timeout expects seconds (0 for none)")?;
                limits.timeout = (secs > 0.0).then(|| std::time::Duration::from_secs_f64(secs));
            }
            "--cache" => cache = value(a)?.parse().map_err(|_| "--cache expects a number of templates")?,
            "--access-log" => {
                let format = it.clone().next().and_then(|v| serve::AccessLog::parse(v));
                if format.is_some() {
                    it.next();
                }
                access_log = format.or(access_log).or(Some(serve::AccessLog::Text));
            }
            s if s.starts_with("--access-log=") => {
                let v = &s["--access-log=".len()..];
                access_log = Some(serve::AccessLog::parse(v).ok_or_else(|| format!("--access-log {v}: say json or text"))?);
            }
            "--allow-anonymous" => allow_anonymous = true,
            "--spool-dir" => spool_dir = Some(PathBuf::from(value(a)?)),
            "--spool-memory" => spool_memory = value(a)?.parse().map_err(|_| "--spool-memory expects megabytes")?,
            "--spool-disk" => spool_disk = value(a)?.parse().map_err(|_| "--spool-disk expects megabytes")?,
            "--rate-limit" => rate_limit = Some(value(a)?.parse().map_err(|_| "--rate-limit expects requests per minute")?),
            "--db" => db = Some(value(a)?),
            s => return Err(format!("unknown option {s}").into()),
        }
    }
    serve::run(serve::Config {
        addr: format!("{host}:{port}"),
        templates,
        resources,
        token,
        max_body: max_body_mb * 1024 * 1024,
        workers,
        limits,
        max_rows,
        cache,
        access_log,
        allow_anonymous,
        rate_limit,
        spool_memory: (spool_memory as usize).saturating_mul(1024 * 1024),
        spool_dir,
        spool_disk: spool_disk.saturating_mul(1024 * 1024),
        connection: match &db {
            Some(url) => Some(connect_db(url)?),
            None => None,
        },
        signature: sign.signature()?,
    })
}

fn count_elements(band: &Band, counts: &mut std::collections::BTreeMap<String, usize>) {
    fn walk(els: &[ferroreport::model::Element], counts: &mut std::collections::BTreeMap<String, usize>) {
        for e in els {
            let name = match &e.kind {
                ElementKind::Text(t) => match t.content {
                    ferroreport::model::TextContent::Static(_) => "staticText".to_string(),
                    _ => "textField".to_string(),
                },
                ElementKind::Line { .. } => "line".into(),
                ElementKind::Rectangle { .. } => "rectangle".into(),
                ElementKind::Ellipse { .. } => "ellipse".into(),
                ElementKind::Frame { children, .. } => {
                    walk(children, counts);
                    "frame".into()
                }
                ElementKind::Break | ElementKind::ColumnBreak => "break".into(),
                ElementKind::Image(_) => "image".into(),
                ElementKind::Subreport(_) | ElementKind::Content { .. } => "subreport".into(),
                ElementKind::Table(_) => "table".into(),
                ElementKind::List(_) => "list".into(),
                ElementKind::Crosstab(_) | ElementKind::CrosstabRest(_) => "crosstab".into(),
                ElementKind::Chart(c) => format!("{:?} chart", c.kind),
                ElementKind::Barcode(b) => b.symbology.component().into(),
                ElementKind::Unsupported(n) => format!("{n} (unsupported)"),
            };
            *counts.entry(name).or_default() += 1;
        }
    }
    walk(&band.elements, counts);
}

fn doctor(args: &[String]) -> CliResult {
    let path = args.first().ok_or("missing template path")?;
    let source = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let dir = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let report: Report = match Renderer::new().resource_dir(dir).parse(&source) {
        Ok(r) => r,
        Err(e) => {
            println!("✗ {path}: cannot be loaded\n  {e}");
            return Err("template not supported".into());
        }
    };
    println!("{path}: report `{}`", report.name);
    println!(
        "  page {}x{} pt, margins {}/{}/{}/{}, language {:?}",
        report.page_width,
        report.page_height,
        report.margins.top,
        report.margins.right,
        report.margins.bottom,
        report.margins.left,
        report.language
    );
    println!(
        "  {} parameters, {} fields, {} variables, {} groups",
        // The template's own parameters, not the hidden ones ferroreport keeps settings in.
        report
            .parameters
            .iter()
            .filter(|p| p.name != ferroreport::text::HYPHENATE && p.name != ferroreport::text::PARAGRAPHS)
            .count(),
        report.fields.len(),
        report.variables.len(),
        report.groups.len()
    );
    let mut counts = std::collections::BTreeMap::new();
    let bands = [
        &report.background,
        &report.title,
        &report.page_header,
        &report.column_header,
        &report.column_footer,
        &report.page_footer,
        &report.last_page_footer,
        &report.summary,
        &report.no_data,
    ];
    for b in bands.into_iter().flatten() {
        count_elements(b, &mut counts);
    }
    for b in &report.detail {
        count_elements(b, &mut counts);
    }
    for g in &report.groups {
        for b in g.header.iter().chain(&g.footer) {
            count_elements(b, &mut counts);
        }
    }
    let list: Vec<String> = counts.iter().map(|(k, v)| format!("{k} ×{v}")).collect();
    println!("  elements: {}", if list.is_empty() { "none".into() } else { list.join(", ") });
    if report.warnings.is_empty() {
        println!("✓ every feature used by this template is supported");
    } else {
        println!("! {} issue(s):", report.warnings.len());
        for w in &report.warnings {
            println!("  - {w}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compared(pictures: Option<pixels::Pictures>, note: Option<&str>) -> Compared {
        Compared {
            jasper_error: None,
            ours_error: None,
            jasper_pages: 1,
            ours_pages: 1,
            jasper_ms: 0.0,
            ours_ms: 0.0,
            csv_diff: (0, 1),
            rows: None,
            pictures,
            pictures_note: note.map(str::to_string),
        }
    }

    fn pictures(pages_differing: usize, worst_page: usize, worst_share: f64) -> pixels::Pictures {
        pixels::Pictures { worst_page: Some(worst_page), worst_share, pages_differing, diff_files: Vec::new() }
    }

    #[test]
    fn strict_pixels_passes_when_every_page_is_the_same_picture() {
        let same = compared(Some(pictures(0, 1, 0.001)), None);
        assert_eq!(strict_pixels_check(&[("a.jrxml", &same), ("b.jrxml", &same)]), Ok(()));
        assert_eq!(strict_pixels_check(&[]), Ok(()));
    }

    #[test]
    fn strict_pixels_lists_the_templates_whose_pages_differ() {
        let same = compared(Some(pictures(0, 1, 0.0)), None);
        let differs = compared(Some(pictures(2, 3, 0.0244)), None);
        let err = strict_pixels_check(&[("same.jrxml", &same), ("invoice.jrxml", &differs)]).unwrap_err();
        assert!(err.contains("1 template(s)"), "{err}");
        assert!(err.contains("invoice.jrxml: 2 page(s) differ; the worst, page 3, 2.44% of its ink"), "{err}");
        assert!(!err.contains("same.jrxml"), "{err}");
    }

    #[test]
    fn strict_pixels_fails_when_the_pictures_could_not_be_compared() {
        let unchecked = compared(None, Some("pdftoppm is not on the PATH"));
        let err = strict_pixels_check(&[("a.jrxml", &unchecked)]).unwrap_err();
        assert!(err.contains("could not be compared") && err.contains("pdftoppm is not on the PATH"), "{err}");
    }

    #[test]
    fn strict_pixels_leaves_templates_an_engine_could_not_render_to_the_other_checks() {
        let mut failed = compared(None, None);
        failed.jasper_error = Some("JRException".into());
        let mut ours = compared(None, None);
        ours.ours_error = Some("unsupported".into());
        assert_eq!(strict_pixels_check(&[("a.jrxml", &failed), ("b.jrxml", &ours)]), Ok(()));
    }
}
