//! `ferroreport serve`: an HTTP endpoint so any language can render reports.
//!
//! ```text
//! POST /render             {"template": "<jasperReport ...>" | "templateName": "invoice.jrxml",
//!                           "data": [...], "params": {...}, "locale": "es_DO",
//!                           "resources": {"logo.png": "<base64>"},
//!                           "format": "pdf" | "xlsx" | "docx" | "pptx" | "ods" | "odt" | "csv" | "html",
//!                           "csvSafe": true, "xlsxNumbers": true, "tagged": true} -> the file
//! POST /render/<name>      {"data": [...], "params": {...}}  (template from --templates)
//! POST /inspect            {"template": "..."}                          -> JSON
//! GET  /health                                                          -> JSON
//! GET  /metrics                                 -> counters in Prometheus text format
//! ```
//!
//! Large reports: send `Content-Type: application/x-ndjson` with the request above on the
//! first line (without `data`) and one row per line after it, and the rows are read as the
//! report needs them. The document is kept (in memory, then on disk) until it is complete,
//! so the answer is the whole document or an error; send `Prefer: respond-streaming` to get
//! it as it renders instead.
//!
//! Parsed templates are kept in a small cache, so a template sent again (or read again from
//! `--templates`) is not parsed twice. Every render runs under the limits given at start-up.

use crate::spool::{Spool, SpoolLimits};
use ferroreport::Renderer;
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub struct Config {
    pub addr: String,
    pub templates: Option<PathBuf>,
    pub resources: Vec<PathBuf>,
    pub token: Option<String>,
    pub max_body: usize,
    pub workers: usize,
    /// Limits every render runs under.
    pub limits: ferroreport::Limits,
    /// Most rows a render may read (`--max-rows`).
    pub max_rows: Option<usize>,
    /// Parsed templates kept (0 turns the cache off).
    pub cache: usize,
    /// Print a line per request to standard error.
    pub access_log: bool,
    /// Listen beyond localhost without a token.
    pub allow_anonymous: bool,
    /// Most requests one client address may make per minute.
    pub rate_limit: Option<u32>,
    /// Bytes of one document kept in memory before it goes to a file.
    pub spool_memory: usize,
    /// Directory for those files (the system's temporary directory when not given).
    pub spool_dir: Option<PathBuf>,
    /// Bytes all requests together may keep on disk.
    pub spool_disk: u64,
    /// The database the queries of templates from `templates` run on (`--db`). Templates
    /// sent in a request never get it: they could run any statement.
    pub connection: Option<Arc<dyn ferroreport::sql::Connection>>,
    /// Every PDF is signed with this key (`--sign-key`).
    pub signature: Option<ferroreport::sign::Signature>,
}

/// Counts each client's requests in the current minute.
#[derive(Default)]
pub struct RateLimiter {
    clients: Mutex<HashMap<std::net::IpAddr, (Instant, u32)>>,
}

impl RateLimiter {
    /// Whether `client` may make another request now; if not, the seconds until it may.
    fn allow(&self, client: std::net::IpAddr, per_minute: u32, now: Instant) -> Result<(), u64> {
        let window = std::time::Duration::from_secs(60);
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        // Forget clients whose minute has passed once the table grows.
        if clients.len() > 10_000 {
            clients.retain(|_, (start, _)| now.duration_since(*start) < window);
        }
        let entry = clients.entry(client).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        if entry.1 >= per_minute {
            return Err((window - now.duration_since(entry.0)).as_secs().max(1));
        }
        entry.1 += 1;
        Ok(())
    }
}

/// Parsed templates by their source text, the least recently used dropped first.
pub struct TemplateCache {
    capacity: usize,
    entries: Mutex<(u64, HashMap<u64, CacheEntry>)>,
}

struct CacheEntry {
    source: Arc<str>,
    report: Arc<ferroreport::Report>,
    used: u64,
}

impl TemplateCache {
    pub fn new(capacity: usize) -> TemplateCache {
        TemplateCache { capacity, entries: Mutex::new((0, HashMap::new())) }
    }

    /// The parsed form of `source`, and whether it came from the cache. `parse` reads the
    /// style templates it uses; a template that uses some is not kept, since they are files
    /// (or parts of the request) that can change without the template changing.
    fn get(
        &self,
        source: &str,
        parse: &dyn Fn(&str) -> ferroreport::Result<ferroreport::Report>,
    ) -> ferroreport::Result<(Arc<ferroreport::Report>, bool)> {
        use std::hash::{Hash, Hasher};
        if self.capacity == 0 || source.contains("<template") {
            return Ok((Arc::new(parse(source)?), false));
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        source.hash(&mut h);
        let key = h.finish();
        {
            let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            let (tick, map) = &mut *guard;
            *tick += 1;
            if let Some(e) = map.get_mut(&key) {
                // The text is compared too: two templates must never answer for each other.
                if &*e.source == source {
                    e.used = *tick;
                    return Ok((e.report.clone(), true));
                }
            }
        }
        // Parsed outside the lock, so a slow template does not hold the others up.
        let report = Arc::new(parse(source)?);
        let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let (tick, map) = &mut *guard;
        if map.len() >= self.capacity && !map.contains_key(&key) {
            if let Some(oldest) = map.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| *k) {
                map.remove(&oldest);
            }
        }
        map.insert(key, CacheEntry { source: source.into(), report: report.clone(), used: *tick });
        Ok((report, false))
    }
}

/// Counters for `GET /metrics`.
#[derive(Default)]
pub struct Metrics {
    renders_ok: AtomicU64,
    renders_failed: AtomicU64,
    limited: AtomicU64,
    unauthorized: AtomicU64,
    rate_limited: AtomicU64,
    streamed: AtomicU64,
    cancelled: AtomicU64,
    panics: AtomicU64,
    render_micros: AtomicU64,
    pages: AtomicU64,
    rows: AtomicU64,
    bytes_out: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    in_flight: AtomicU64,
}

impl Metrics {
    fn text(&self, spool: &SpoolLimits) -> String {
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let mut out = String::new();
        let mut line = |name: &str, kind: &str, help: &str, value: String| {
            out.push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}\n"));
        };
        line("ferroreport_renders_total", "counter", "Reports rendered.", g(&self.renders_ok).to_string());
        line(
            "ferroreport_render_errors_total",
            "counter",
            "Renders that failed (bad template or data).",
            g(&self.renders_failed).to_string(),
        );
        line(
            "ferroreport_render_limited_total",
            "counter",
            "Renders stopped by a page, row or time limit.",
            g(&self.limited).to_string(),
        );
        line(
            "ferroreport_unauthorized_total",
            "counter",
            "Requests refused for a missing or wrong token.",
            g(&self.unauthorized).to_string(),
        );
        line(
            "ferroreport_rate_limited_total",
            "counter",
            "Requests refused because the client asked too often.",
            g(&self.rate_limited).to_string(),
        );
        line(
            "ferroreport_panics_total",
            "counter",
            "Requests answered 500 after an internal error.",
            g(&self.panics).to_string(),
        );
        line(
            "ferroreport_render_seconds_total",
            "counter",
            "Time spent rendering.",
            format!("{:.6}", g(&self.render_micros) as f64 / 1e6),
        );
        line("ferroreport_pages_total", "counter", "Pages rendered.", g(&self.pages).to_string());
        line("ferroreport_rows_total", "counter", "Data rows read.", g(&self.rows).to_string());
        line("ferroreport_response_bytes_total", "counter", "Bytes of rendered documents sent.", g(&self.bytes_out).to_string());
        line(
            "ferroreport_template_cache_hits_total",
            "counter",
            "Templates found already parsed.",
            g(&self.cache_hits).to_string(),
        );
        line(
            "ferroreport_template_cache_misses_total",
            "counter",
            "Templates that had to be parsed.",
            g(&self.cache_misses).to_string(),
        );
        line("ferroreport_renders_in_flight", "gauge", "Renders running now.", g(&self.in_flight).to_string());
        line(
            "ferroreport_streamed_responses_total",
            "counter",
            "Documents sent while they rendered (Prefer: respond-streaming).",
            g(&self.streamed).to_string(),
        );
        line(
            "ferroreport_cancelled_total",
            "counter",
            "Renders stopped because the client went away.",
            g(&self.cancelled).to_string(),
        );
        line(
            "ferroreport_spool_disk_bytes",
            "gauge",
            "Bytes of documents and requests kept on disk now.",
            spool.used().to_string(),
        );
        out
    }
}

/// What the workers share.
pub struct State {
    pub cfg: Config,
    pub cache: TemplateCache,
    pub metrics: Metrics,
    pub limiter: RateLimiter,
    pub spool: Arc<SpoolLimits>,
}

type Reply = tiny_http::ResponseBox;

fn json(status: u16, value: serde_json::Value) -> Reply {
    Response::from_data(value.to_string().into_bytes())
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", "application/json").expect("static header"))
        .boxed()
}

fn error(status: u16, message: impl std::fmt::Display) -> Reply {
    json(status, serde_json::json!({ "error": message.to_string() }))
}

/// Compares a secret in time that does not depend on where the inputs differ.
fn same_secret(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= (a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0)) as usize;
    }
    diff == 0
}

/// A template name from a URL: a plain relative path inside the templates directory.
fn safe_name(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if name.is_empty() || p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    Some(p.to_path_buf())
}

/// What one request asks for, ready to render on any thread.
struct Job {
    report: Arc<ferroreport::Report>,
    parse_ms: f64,
    locale: String,
    dirs: Vec<PathBuf>,
    resources: Vec<(String, Vec<u8>)>,
    now: Option<String>,
    csv_safe: bool,
    tagged: bool,
    pdfa: Option<ferroreport::pdf::PdfA>,
    encryption: Option<ferroreport::crypt::Encryption>,
    xlsx_numbers: bool,
    limits: ferroreport::Limits,
    max_rows: Option<usize>,
    params: serde_json::Value,
    format: ferroreport::Format,
    /// The database, for a template from the templates directory.
    connection: Option<Arc<dyn ferroreport::sql::Connection>>,
    /// Filled from its own query: the request sent no rows.
    from_db: bool,
    signature: Option<ferroreport::sign::Signature>,
}

/// What a write fails with once the client has closed the connection.
const GONE: &str = "the client went away";

/// The status a rendering error is answered with.
fn status_of(e: &ferroreport::Error) -> u16 {
    match e {
        ferroreport::Error::Limit(_) => 413,
        // No room to keep the document until it is complete.
        ferroreport::Error::Io(m) if m.contains("--spool") => 507,
        // Nobody to write to any more.
        ferroreport::Error::Io(m) if m.contains(GONE) => 499,
        // The rows stopped arriving.
        ferroreport::Error::Io(_) => 400,
        _ => 422,
    }
}

fn failed(e: ferroreport::Error) -> (u16, String) {
    (status_of(&e), e.to_string())
}

/// Reads the request (everything but its rows): which template, with what options.
fn prepare(
    state: &State,
    body: &serde_json::Value,
    name: Option<&str>,
    format: ferroreport::Format,
) -> Result<Job, (u16, String)> {
    let cfg = &state.cfg;
    let bad = |m: String| (400, m);
    let (source, dir) = match (name, body.get("templateName").and_then(|v| v.as_str()), body.get("template")) {
        (Some(n), _, _) | (None, Some(n), _) => {
            let root = cfg.templates.as_ref().ok_or((404, "no --templates directory configured".to_string()))?;
            let mut rel = safe_name(n).ok_or((400, format!("invalid template name `{n}`")))?;
            if rel.extension().is_none() {
                // `invoice` is invoice.jrxml, or invoice.ferro when there is no JRXML.
                rel.set_extension("jrxml");
                if !root.join(&rel).exists() && root.join(rel.with_extension("ferro")).exists() {
                    rel.set_extension("ferro");
                }
            }
            let path = root.join(&rel);
            // A symbolic link in the templates directory may not point outside it.
            let inside = match (path.canonicalize(), root.canonicalize()) {
                (Ok(real), Ok(real_root)) => real.starts_with(real_root),
                _ => false,
            };
            if !inside {
                return Err((404, format!("template `{n}` not found")));
            }
            let text = std::fs::read_to_string(&path).map_err(|_| (404, format!("template `{n}` not found")))?;
            (text, path.parent().map(Path::to_path_buf))
        }
        (None, None, Some(serde_json::Value::String(t))) => (t.clone(), None),
        _ => return Err(bad("send `template` (JRXML source) or `templateName`".into())),
    };
    let mut dirs: Vec<PathBuf> = dir.into_iter().collect();
    dirs.extend(cfg.resources.iter().cloned());
    let now = body.get("now").and_then(|v| v.as_str()).map(str::to_string);
    if let Some(now) = &now {
        ferroreport::data::parse_datetime(now).ok_or_else(|| bad(format!("cannot parse now `{now}`")))?;
    }
    let mut resources = Vec::new();
    if let Some(res) = body.get("resources").and_then(|v| v.as_object()) {
        for (k, v) in res {
            let text = v.as_str().ok_or_else(|| bad(format!("resource `{k}` must be a base64 string")))?;
            let bytes = ferroreport::image::decode_base64(text).map_err(|e| bad(format!("resource `{k}`: {e}")))?;
            resources.push((k.clone(), bytes));
        }
    }
    let flag = |name: &str| body.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
    let started = Instant::now();
    // Style templates are read where images and subreports are: the template's folder, the
    // resource folders and the files sent with the request.
    let parse = |text: &str| {
        let mut r = Renderer::new();
        for d in &dirs {
            r = r.resource_dir(d);
        }
        for (name, bytes) in &resources {
            r.add_resource(name, bytes.clone());
        }
        r.parse(text)
    };
    let (report, cached) = state.cache.get(&source, &parse).map_err(failed)?;
    let named = name.is_some() || body.get("templateName").and_then(|v| v.as_str()).is_some();
    let connection = if named { cfg.connection.clone() } else { None };
    let params = body.get("params").cloned().unwrap_or_else(|| serde_json::Value::Object(Default::default()));
    if connection.is_some() {
        // A value pasted into the SQL text ($P!{}) is the template's to choose, not a client's.
        if let (Some(query), Some(given)) = (&report.query, params.as_object()) {
            for pasted in pasted_params(&query.text) {
                if given.contains_key(&pasted) {
                    return Err(bad(format!(
                        "parameter `{pasted}` is pasted into the query ($P!{{{pasted}}}): the server does not take it from requests"
                    )));
                }
            }
        }
    }
    let from_db = connection.is_some() && body.get("data").is_none() && report.query.is_some();
    let counter = if cached { &state.metrics.cache_hits } else { &state.metrics.cache_misses };
    counter.fetch_add(1, Ordering::Relaxed);
    Ok(Job {
        report,
        parse_ms: started.elapsed().as_secs_f64() * 1e3,
        locale: body.get("locale").and_then(|v| v.as_str()).unwrap_or("en_US").to_string(),
        dirs,
        resources,
        now,
        csv_safe: flag("csvSafe"),
        tagged: flag("tagged"),
        pdfa: match body.get("pdfa").and_then(|v| v.as_str()) {
            Some(v) => {
                Some(ferroreport::pdf::PdfA::parse(v).ok_or_else(|| bad(format!("pdfa \"{v}\": say 2b, 2u, 2a, 3b, 3u or 3a")))?)
            }
            None => None,
        },
        encryption: {
            let text = |k: &str| body.get(k).and_then(|v| v.as_str()).map(str::to_string);
            let permissions = match body.get("permissions") {
                Some(serde_json::Value::Array(list)) => {
                    Some(list.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(","))
                }
                other => other.and_then(|v| v.as_str()).map(str::to_string),
            };
            let (password, owner) = (text("password"), text("ownerPassword"));
            if password.is_some() || owner.is_some() || permissions.is_some() {
                Some(ferroreport::crypt::Encryption {
                    user_password: password.unwrap_or_default(),
                    owner_password: owner,
                    permissions: ferroreport::crypt::Permissions::parse(permissions.as_deref().unwrap_or(""))
                        .map_err(|e| bad(e.to_string()))?,
                })
            } else {
                None
            }
        },
        xlsx_numbers: flag("xlsxNumbers"),
        limits: cfg.limits,
        max_rows: cfg.max_rows,
        params,
        format,
        connection,
        from_db,
        signature: cfg.signature.clone(),
    })
}

/// The parameters a query pastes in as text: `$P!{NAME}`.
fn pasted_params(sql: &str) -> Vec<String> {
    sql.match_indices("$P!{").filter_map(|(i, _)| sql[i + 4..].split_once('}').map(|(name, _)| name.trim().to_string())).collect()
}

impl Job {
    /// Renders the report into `out`. `rows` gives the rows from the start each time it is
    /// called: once, or more for a large workbook or OpenDocument file.
    fn run<'a, W: Write>(
        &self,
        rows: &mut dyn FnMut() -> ferroreport::Result<ferroreport::RowIter<'a>>,
        out: W,
    ) -> Result<ferroreport::FillStats, (u16, String)> {
        let mut r = Renderer::new()
            .locale(&self.locale)
            .limits(self.limits)
            .max_rows(self.max_rows)
            .tagged(self.tagged)
            .pdfa(self.pdfa)
            .encrypt(self.encryption.clone())
            .csv_options(ferroreport::csv::CsvOptions { guard_formulas: self.csv_safe, ..Default::default() })
            .xlsx_options(ferroreport::xlsx::XlsxOptions { detect_cell_type: self.xlsx_numbers });
        for d in &self.dirs {
            r = r.resource_dir(d);
        }
        if let Some(now) = self.now.as_deref().and_then(ferroreport::data::parse_datetime) {
            r = r.now(now);
        }
        for (name, bytes) in &self.resources {
            r.add_resource(name, bytes.clone());
        }
        let params = ferroreport::params_from_json(&self.report, &self.params).map_err(failed)?;
        r = r.connection(self.connection.clone()).sign(self.signature.clone());
        if self.from_db {
            r.render_query_to(&self.report, params, self.format, out).map_err(failed)
        } else {
            r.render_rows_to(&self.report, rows, params, self.format, out).map_err(failed)
        }
    }

    /// Whether the rows may be asked for a second time.
    fn reads_rows_twice(&self) -> bool {
        matches!(self.format, ferroreport::Format::Xlsx | ferroreport::Format::Ods | ferroreport::Format::Odt)
    }
}

/// Reads a spool that several readers take turns with.
struct Shared(std::rc::Rc<std::cell::RefCell<Spool>>);

impl Read for Shared {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().read(out)
    }
}

impl OwnedRows {
    /// The rows from the start, each time the result is called. `again` says whether it
    /// may be called more than once; when not, records are handed over instead of copied.
    fn source<'a>(
        self,
        report: &'a ferroreport::Report,
        again: bool,
    ) -> Box<dyn FnMut() -> ferroreport::Result<ferroreport::RowIter<'a>> + 'a> {
        match self {
            OwnedRows::Records(records) if again => {
                let records = std::rc::Rc::new(records);
                Box::new(move || {
                    let records = records.clone();
                    Ok(Box::new((0..records.len()).map(move |i| Ok(records[i].clone()))))
                })
            }
            OwnedRows::Records(records) => {
                let mut records = Some(records);
                Box::new(move || Ok(Box::new(records.take().unwrap_or_default().into_iter().map(Ok))))
            }
            OwnedRows::Lines(spool) => {
                let spool = std::rc::Rc::new(std::cell::RefCell::new(spool));
                Box::new(move || {
                    spool.borrow_mut().rewind().map_err(|e| ferroreport::Error::Io(e.to_string()))?;
                    Ok(Box::new(ferroreport::JsonRows::new(report, Shared(spool.clone())).allow_empty()))
                })
            }
        }
    }
}

/// The rows of a request, owned so they can cross to another thread.
enum OwnedRows {
    Records(Vec<ferroreport::Record>),
    /// One JSON object per line, kept in a spool.
    Lines(Spool),
}

/// A piece of a document on its way to the client, or why there will be no more.
enum Piece {
    Data(Vec<u8>),
    Failed(u16, String),
}

/// Sends what is written to it to the thread that answers the client, 64 KB at a time. A
/// full channel makes the renderer wait (a slow client slows the render down instead of
/// filling memory); a closed one means the client is gone, and stops it.
///
/// What has not filled a piece yet stays in `tail`, for the renderer's thread to send once
/// the render has succeeded: a report that fails before its first piece is then answered
/// with an ordinary error, not with the start of a document.
struct ToClient {
    tx: std::sync::mpsc::SyncSender<Piece>,
    state: Arc<State>,
    tail: Arc<std::sync::Mutex<Vec<u8>>>,
}

const PIECE: usize = 1 << 16;

impl ToClient {
    fn send(tx: &std::sync::mpsc::SyncSender<Piece>, state: &State, bytes: Vec<u8>) -> std::io::Result<()> {
        let n = bytes.len() as u64;
        tx.send(Piece::Data(bytes)).map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, GONE))?;
        state.metrics.bytes_out.fetch_add(n, Ordering::Relaxed);
        Ok(())
    }
}

impl Write for ToClient {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut tail = self.tail.lock().unwrap_or_else(|e| e.into_inner());
        tail.extend_from_slice(buf);
        if tail.len() >= PIECE {
            let full = std::mem::replace(&mut *tail, Vec::with_capacity(PIECE));
            Self::send(&self.tx, &self.state, full)?;
        }
        Ok(buf.len())
    }

    /// Nothing: pieces go out when they are full, and the last one when the render is done.
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A document on its way out while it renders: its first bytes, and where the rest arrives.
struct Streamed {
    format: ferroreport::Format,
    first: Vec<u8>,
    rx: std::sync::mpsc::Receiver<Piece>,
}

/// What a request is answered with.
enum Outcome {
    Reply(Reply),
    Stream(Streamed),
}

impl Outcome {
    fn status(&self) -> u16 {
        match self {
            Outcome::Reply(r) => r.status_code().0,
            Outcome::Stream(_) => 200,
        }
    }
}

impl Streamed {
    /// Writes the response by hand, in chunks, as the renderer produces them.
    ///
    /// By hand because of how it must end when the render fails half way: a chunked body is
    /// only known to be complete by its last, empty chunk, and the HTTP library writes that
    /// chunk whatever happened. Here a failure writes a line no client accepts as a chunk
    /// instead, so that every client reports an error rather than a document that looks whole.
    /// Returns whether the document went out complete.
    fn send(self, mut out: Box<dyn Write + Send>) -> bool {
        let head = format!(
            "HTTP/1.1 200 OK\r\nServer: ferroreport/{}\r\nDate: {}\r\nContent-Type: {}\r\nPreference-Applied: respond-streaming\r\n\
             Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            env!("CARGO_PKG_VERSION"),
            httpdate::fmt_http_date(std::time::SystemTime::now()),
            self.format.mime()
        );
        fn chunk(out: &mut dyn Write, bytes: &[u8]) -> std::io::Result<()> {
            if bytes.is_empty() {
                return Ok(());
            }
            write!(out, "{:x}\r\n", bytes.len())?;
            out.write_all(bytes)?;
            out.write_all(b"\r\n")?;
            out.flush()
        }
        // An error here is the client closing the connection: dropping `rx` on the way out
        // is what tells the renderer to stop.
        if out.write_all(head.as_bytes()).is_err() || chunk(&mut out, &self.first).is_err() {
            return false;
        }
        loop {
            match self.rx.recv() {
                Ok(Piece::Data(bytes)) => {
                    if chunk(&mut out, &bytes).is_err() {
                        return false;
                    }
                }
                Ok(Piece::Failed(_, message)) => {
                    let said: String = message.chars().filter(|c| !c.is_control()).take(300).collect();
                    let _ = write!(out, "incomplete: {said}\r\n");
                    let _ = out.flush();
                    return false;
                }
                Err(_) => return out.write_all(b"0\r\n\r\n").and_then(|_| out.flush()).is_ok(),
            }
        }
    }
}

fn typed(format: ferroreport::Format) -> Header {
    Header::from_bytes("Content-Type", format.mime()).expect("static header")
}

/// Renders into a spool and answers with the whole document, or with an error and nothing
/// else. `lines` are the rows still to be read from the request, when they come one per line.
fn respond_whole<'a>(
    state: &Arc<State>,
    job: &'a Job,
    body: &serde_json::Value,
    lines: Option<&'a mut dyn Read>,
) -> Result<Reply, (u16, String)> {
    let again = job.reads_rows_twice();
    let mut rows: Box<dyn FnMut() -> ferroreport::Result<ferroreport::RowIter<'a>> + 'a> = match lines {
        // Rows that are read twice are put aside first: the request can only be read once.
        Some(reader) if again => {
            let mut request = Spool::new(state.spool.clone(), "the request");
            std::io::copy(reader, &mut request)
                .map_err(|e| (if e.to_string().contains("--spool") { 507 } else { 400 }, e.to_string()))?;
            OwnedRows::Lines(request).source(&job.report, true)
        }
        Some(reader) => {
            let mut reader = Some(reader);
            Box::new(move || match reader.take() {
                Some(r) => Ok(Box::new(ferroreport::JsonRows::new(&job.report, r).allow_empty()) as ferroreport::RowIter),
                None => Err(ferroreport::Error::Io("the rows of the request were already read".into())),
            })
        }
        None => {
            let empty = serde_json::Value::Array(Vec::new());
            let records =
                ferroreport::data::records_from_json(&job.report, body.get("data").unwrap_or(&empty)).map_err(failed)?;
            OwnedRows::Records(records).source(&job.report, again)
        }
    };
    let started = Instant::now();
    let mut spool = Spool::new(state.spool.clone(), "the document");
    let stats = job.run(&mut *rows, &mut spool)?;
    spool.rewind().map_err(|e| (500, e.to_string()))?;
    let len = spool.len();
    let m = &state.metrics;
    m.renders_ok.fetch_add(1, Ordering::Relaxed);
    m.pages.fetch_add(stats.pages as u64, Ordering::Relaxed);
    m.rows.fetch_add(stats.rows as u64, Ordering::Relaxed);
    m.bytes_out.fetch_add(len, Ordering::Relaxed);
    let timing = format!("parse;dur={:.2}, render;dur={:.2}", job.parse_ms, started.elapsed().as_secs_f64() * 1e3);
    let headers = vec![
        typed(job.format),
        Header::from_bytes("Server-Timing", timing).expect("ascii header"),
        Header::from_bytes("X-Report-Pages", stats.pages.to_string()).expect("ascii header"),
    ];
    // With its length, whatever its size: a client can then tell a whole document from one cut short.
    Ok(Response::new(StatusCode(200), headers, Box::new(spool) as Box<dyn Read + Send>, Some(len as usize), None)
        .with_chunked_threshold(usize::MAX))
}

/// Renders on a thread of its own and answers with the document as it is produced. An error
/// before the first bytes is answered as an error; one after them cuts the response short.
fn respond_streaming(state: &Arc<State>, job: Job, rows: OwnedRows) -> Result<Streamed, Reply> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Piece>(8);
    let format = job.format;
    let shared = state.clone();
    state.metrics.in_flight.fetch_add(1, Ordering::Relaxed);
    let spawned = std::thread::Builder::new().name("render".into()).stack_size(32 * 1024 * 1024).spawn(move || {
        let started = Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut rows = rows.source(&job.report, job.reads_rows_twice());
            let tail = Arc::new(std::sync::Mutex::new(Vec::with_capacity(PIECE)));
            let stats = job.run(&mut *rows, ToClient { tx: tx.clone(), state: shared.clone(), tail: tail.clone() })?;
            let last = std::mem::take(&mut *tail.lock().unwrap_or_else(|e| e.into_inner()));
            ToClient::send(&tx, &shared, last).map_err(|e| failed(ferroreport::Error::Io(e.to_string())))?;
            Ok(stats)
        }));
        let m = &shared.metrics;
        m.in_flight.fetch_sub(1, Ordering::Relaxed);
        m.render_micros.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        match result {
            Ok(Ok(stats)) => {
                m.renders_ok.fetch_add(1, Ordering::Relaxed);
                m.pages.fetch_add(stats.pages as u64, Ordering::Relaxed);
                m.rows.fetch_add(stats.rows as u64, Ordering::Relaxed);
            }
            Ok(Err((499, _))) => {
                m.cancelled.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Err((status, message))) => {
                let counter = if status == 413 { &m.limited } else { &m.renders_failed };
                counter.fetch_add(1, Ordering::Relaxed);
                let _ = tx.send(Piece::Failed(status, message));
            }
            Err(_) => {
                m.panics.fetch_add(1, Ordering::Relaxed);
                let _ = tx.send(Piece::Failed(500, "internal error while rendering".into()));
            }
        }
    });
    if spawned.is_err() {
        state.metrics.in_flight.fetch_sub(1, Ordering::Relaxed);
        return Err(error(503, "cannot start another render now"));
    }
    let first = match rx.recv() {
        Ok(Piece::Failed(status, message)) => return Err(error(status, message)),
        Ok(Piece::Data(bytes)) => bytes,
        Err(_) => Vec::new(),
    };
    state.metrics.streamed.fetch_add(1, Ordering::Relaxed);
    Ok(Streamed { format, first, rx })
}

fn handle(state: &Arc<State>, req: &mut Request) -> Outcome {
    let mut stream = None;
    let reply = answer(state, req, &mut stream);
    match stream {
        Some(streamed) => Outcome::Stream(streamed),
        None => Outcome::Reply(reply),
    }
}

/// Answers a request; a document to be sent while it renders is left in `stream` instead.
fn answer(state: &Arc<State>, req: &mut Request, stream: &mut Option<Streamed>) -> Reply {
    let cfg = &state.cfg;
    let url = req.url().split('?').next().unwrap_or("").to_string();
    // Load balancers ask for /health without credentials; it says nothing but "ok".
    if (req.method(), url.as_str()) == (&Method::Get, "/health") {
        return json(200, serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }));
    }
    if let (Some(per_minute), Some(client)) = (cfg.rate_limit, req.remote_addr().map(|a| a.ip())) {
        if let Err(wait) = state.limiter.allow(client, per_minute, Instant::now()) {
            state.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
            return error(429, format!("too many requests: try again in {wait} s"))
                .with_header(Header::from_bytes("Retry-After", wait.to_string()).expect("ascii header"));
        }
    }
    if let Some(token) = &cfg.token {
        let ok = req.headers().iter().any(|h| {
            h.field.equiv("Authorization")
                && h.value.as_str().strip_prefix("Bearer ").is_some_and(|t| same_secret(t.as_bytes(), token.as_bytes()))
        });
        if !ok {
            state.metrics.unauthorized.fetch_add(1, Ordering::Relaxed);
            return error(401, "missing or wrong bearer token");
        }
    }
    match (req.method(), url.as_str()) {
        (Method::Get, "/metrics") => Response::from_data(state.metrics.text(&state.spool).into_bytes())
            .with_header(Header::from_bytes("Content-Type", "text/plain; version=0.0.4").expect("static header"))
            .boxed(),
        (Method::Post, path) if path == "/render" || path.starts_with("/render/") || path == "/inspect" => {
            let header = |name: &'static str| {
                req.headers()
                    .iter()
                    .find(|h| h.field.equiv(name))
                    .map(|h| h.value.as_str().to_ascii_lowercase())
                    .unwrap_or_default()
            };
            // Rows one per line after a first line with everything else: read as they are needed.
            let content_type = header("Content-Type");
            let lines = ["application/x-ndjson", "application/jsonl", "application/x-jsonlines", "application/json-lines"]
                .iter()
                .any(|t| content_type.starts_with(t));
            // A document can only be sent as it renders to a client that reads chunks (HTTP/1.1).
            let streaming = header("Prefer").contains("respond-streaming") && *req.http_version() >= tiny_http::HTTPVersion(1, 1);
            let length = req.body_length();
            let query = req.url().split_once('?').map(|(_, q)| q.to_string()).unwrap_or_default();
            if !lines && length.is_some_and(|n| n > cfg.max_body) {
                return error(413, "request body too large");
            }
            // Read what the request says it carries, not "until the connection ends": a client
            // that offers to switch protocols (Java's HttpClient offers HTTP/2 on every first
            // request) keeps the connection open, and waiting for its end would never return.
            let limit = match length {
                Some(n) => n as u64,
                None if header("Connection").contains("upgrade") => return error(411, "send a Content-Length with this request"),
                None if lines => u64::MAX,
                None => cfg.max_body as u64 + 1,
            };
            let mut reader = std::io::BufReader::with_capacity(1 << 16, req.as_reader().take(limit));
            let mut head = Vec::new();
            let read = if lines {
                (&mut reader).take(cfg.max_body as u64 + 1).read_until(b'\n', &mut head)
            } else {
                reader.read_to_end(&mut head)
            };
            if read.is_err() {
                return error(400, "cannot read request body");
            }
            if head.len() > cfg.max_body {
                return error(413, if lines { "the first line of the request is too large" } else { "request body too large" });
            }
            let body: serde_json::Value = match serde_json::from_slice(&head) {
                Ok(v) => v,
                Err(e) => return error(400, format!("invalid JSON: {e}")),
            };
            drop(head);
            if path == "/inspect" {
                let Some(t) = body.get("template").and_then(|v| v.as_str()) else {
                    return error(400, "send `template`");
                };
                return match ferroreport::parse(t) {
                    Ok(r) => json(
                        200,
                        serde_json::json!({
                            "name": r.name,
                            "parameters": r.parameters.iter().map(|p| &p.name).filter(|n| *n != ferroreport::text::HYPHENATE && *n != ferroreport::text::PARAGRAPHS).collect::<Vec<_>>(),
                            "fields": r.fields.iter().map(|f| &f.name).collect::<Vec<_>>(),
                            "warnings": r.warnings,
                        }),
                    ),
                    Err(e) => error(422, e),
                };
            }
            let name = path.strip_prefix("/render/").map(|n| n.trim_matches('/'));
            // `format` in the body, or `?format=` in the URL; PDF by default.
            let requested = body
                .get("format")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| query.split('&').find_map(|kv| kv.strip_prefix("format=").map(str::to_string)));
            let format = match requested {
                None => ferroreport::Format::Pdf,
                Some(f) => match ferroreport::Format::parse(&f) {
                    Some(f) => f,
                    None => return error(400, format!("unknown format `{f}` (pdf, xlsx, docx, pptx, ods, odt, csv or html)")),
                },
            };
            let m = &state.metrics;
            let refuse = |(status, message): (u16, String)| {
                let counter = if status == 413 { &m.limited } else { &m.renders_failed };
                counter.fetch_add(1, Ordering::Relaxed);
                error(status, message)
            };
            let mut job = match prepare(state, &body, name, format) {
                Ok(job) => job,
                Err(e) => return refuse(e),
            };
            // Rows sent one per line are the report's rows, whatever its query.
            job.from_db &= !lines;
            if streaming {
                // The response starts while the report renders, and a request cannot be read
                // and answered at once: its rows are set aside first (on disk when large).
                let rows = if lines {
                    let mut spool = Spool::new(state.spool.clone(), "the request");
                    if let Err(e) = std::io::copy(&mut reader, &mut spool).and_then(|_| spool.rewind()) {
                        let status = if e.to_string().contains("--spool") { 507 } else { 400 };
                        return refuse((status, e.to_string()));
                    }
                    OwnedRows::Lines(spool)
                } else {
                    let empty = serde_json::Value::Array(Vec::new());
                    match ferroreport::data::records_from_json(&job.report, body.get("data").unwrap_or(&empty)) {
                        Ok(records) => OwnedRows::Records(records),
                        Err(e) => return refuse(failed(e)),
                    }
                };
                return match respond_streaming(state, job, rows) {
                    Ok(streamed) => {
                        *stream = Some(streamed);
                        json(200, serde_json::Value::Null)
                    }
                    Err(reply) => reply,
                };
            }
            m.in_flight.fetch_add(1, Ordering::Relaxed);
            let started = Instant::now();
            let result = respond_whole(state, &job, &body, if lines { Some(&mut reader) } else { None });
            m.in_flight.fetch_sub(1, Ordering::Relaxed);
            m.render_micros.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
            match result {
                Ok(reply) => reply,
                Err(e) => refuse(e),
            }
        }
        _ => error(404, "not found: use POST /render, POST /render/<name>, POST /inspect, GET /health or GET /metrics"),
    }
}

/// Whether an address to listen on can only be reached from this machine.
fn is_local(addr: &str) -> bool {
    ["127.", "localhost", "[::1]"].iter().any(|p| addr.starts_with(p))
}

/// Refuses a configuration that would let anyone on the network render reports.
pub fn check(cfg: &Config) -> Result<(), String> {
    if cfg.token.is_none() && !cfg.allow_anonymous && !is_local(&cfg.addr) {
        return Err(format!(
            "refusing to listen on {} without a token: anyone who can reach this port could render reports.\n\
             Set --token <secret> (or FERROREPORT_TOKEN), or pass --allow-anonymous (or FERROREPORT_ALLOW_ANONYMOUS=1) \
             if the port is only reachable by your own services.",
            cfg.addr
        ));
    }
    Ok(())
}

pub fn run(cfg: Config) -> Result<(), Box<dyn std::error::Error>> {
    check(&cfg)?;
    let listener = std::net::TcpListener::bind(&cfg.addr).map_err(|e| format!("{}: {e}", cfg.addr))?;
    // Answers leave as soon as they are written. Without this the system holds the end of a
    // response back until the client acknowledges its start, which a client puts off for
    // 40 ms: every report asked for over a connection that is kept open took that much longer.
    // Connections take the setting from the socket they are accepted on.
    let _ = socket2::SockRef::from(&listener).set_tcp_nodelay(true);
    let server = Arc::new(Server::from_listener(listener, None).map_err(|e| format!("{}: {e}", cfg.addr))?);
    eprintln!("ferroreport {} listening on http://{}", env!("CARGO_PKG_VERSION"), cfg.addr);
    // Where documents wait until they are complete once they outgrow memory.
    let spool_dir = match &cfg.spool_dir {
        Some(dir) if crate::spool::writable(dir) => Some(dir.clone()),
        Some(dir) => return Err(format!("--spool-dir {}: cannot write there", dir.display()).into()),
        None => Some(std::env::temp_dir()).filter(|d| crate::spool::writable(d)),
    };
    if spool_dir.is_none() {
        eprintln!(
            "note: no writable temporary directory: documents over {} MB will be refused. Set --spool-dir to allow them.",
            cfg.spool_memory / (1024 * 1024)
        );
    }
    let state = Arc::new(State {
        cache: TemplateCache::new(cfg.cache),
        metrics: Metrics::default(),
        limiter: RateLimiter::default(),
        spool: Arc::new(SpoolLimits::new(cfg.spool_memory, spool_dir, cfg.spool_disk)),
        cfg,
    });
    let workers: Vec<_> = (0..state.cfg.workers.max(1))
        .map(|_| {
            let (server, state) = (server.clone(), state.clone());
            // Large stacks: templates are processed recursively (nesting is capped well below
            // what this holds).
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
                    while let Ok(mut req) = server.recv() {
                        // A bug must not take the worker down with it: answer 500 and go on.
                        let started = Instant::now();
                        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(&state, &mut req)))
                            .unwrap_or_else(|_| {
                                state.metrics.panics.fetch_add(1, Ordering::Relaxed);
                                Outcome::Reply(error(500, "internal error while rendering"))
                            });
                        if state.cfg.access_log {
                            eprintln!(
                                "{} {} {} {:.1}ms",
                                req.method(),
                                req.url().split('?').next().unwrap_or(""),
                                outcome.status(),
                                started.elapsed().as_secs_f64() * 1e3
                            );
                        }
                        match outcome {
                            Outcome::Reply(reply) => {
                                drain(&mut req);
                                let _ = req.respond(reply);
                            }
                            Outcome::Stream(streamed) => {
                                streamed.send(req.into_writer());
                            }
                        }
                    }
                })
                .expect("spawn worker")
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

/// Reads what the client is still sending of a request answered before its end (a wrong
/// template, a row that fails): a connection closed with data left unread is reset, and the
/// client can lose the answer with it. Up to a limit, past which the answer goes as it is.
/// Not for a client that offers to switch protocols: its body is read by its length, and what
/// follows on its connection only ends when the client closes it.
fn drain(req: &mut Request) {
    const LIMIT: u64 = 256 * 1024 * 1024;
    let upgrade =
        req.headers().iter().any(|h| h.field.equiv("Connection") && h.value.as_str().to_ascii_lowercase().contains("upgrade"));
    if upgrade {
        return;
    }
    let _ = std::io::copy(&mut req.as_reader().take(LIMIT), &mut std::io::sink());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `Streamed::send` wrote, for a render that produced `pieces`.
    fn sent(pieces: Vec<Piece>) -> (bool, String) {
        #[derive(Clone, Default)]
        struct Wire(Arc<Mutex<Vec<u8>>>);
        impl Write for Wire {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(pieces.len() + 1);
        for p in pieces {
            tx.send(p).unwrap();
        }
        drop(tx);
        let wire = Wire::default();
        let whole = Streamed { format: ferroreport::Format::Csv, first: b"a,b\n".to_vec(), rx }.send(Box::new(wire.clone()));
        let text = String::from_utf8(wire.0.lock().unwrap().clone()).unwrap();
        (whole, text)
    }

    #[test]
    fn a_streamed_document_ends_with_the_last_chunk_only_when_it_is_whole() {
        let (whole, text) = sent(vec![Piece::Data(b"1,2\n".to_vec())]);
        assert!(whole);
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("Transfer-Encoding: chunked\r\n") && text.contains("Preference-Applied: respond-streaming\r\n"));
        assert!(text.ends_with("\r\n\r\n4\r\na,b\n\r\n4\r\n1,2\n\r\n0\r\n\r\n"), "{text:?}");

        // A render that fails half way: no last chunk, and a line that is not a chunk size,
        // so that a client reports an error instead of a short document.
        let (whole, text) = sent(vec![Piece::Data(b"1,2\n".to_vec()), Piece::Failed(422, "row 7\r\nis wrong".into())]);
        assert!(!whole);
        assert!(text.ends_with("4\r\n1,2\n\r\nincomplete: row 7is wrong\r\n"), "{text:?}");
        assert!(!text.contains("0\r\n\r\n"));
    }

    #[test]
    fn errors_are_answered_with_the_status_that_says_whose_they_are() {
        use ferroreport::Error;
        assert_eq!(status_of(&Error::Limit("too many pages".into())), 413);
        assert_eq!(status_of(&Error::Io(format!("the document {}", crate::spool::NO_DIR))), 507);
        assert_eq!(status_of(&Error::Io(format!("the document {}", crate::spool::DISK_FULL))), 507);
        assert_eq!(status_of(&Error::Io(GONE.into())), 499);
        assert_eq!(status_of(&Error::Data("row 3 is not an object".into())), 422);
    }

    #[test]
    fn secrets_compare_exactly() {
        assert!(same_secret(b"abc", b"abc"));
        assert!(!same_secret(b"abc", b"abd"));
        assert!(!same_secret(b"abc", b"abcd"));
        assert!(!same_secret(b"", b"a"));
    }

    #[test]
    fn the_template_cache_reuses_and_forgets() {
        let t = |name: &str| {
            format!(
                r#"<jasperReport name="{name}" pageWidth="100" pageHeight="100" columnWidth="100" leftMargin="0" rightMargin="0" topMargin="0" bottomMargin="0"/>"#
            )
        };
        let cache = TemplateCache::new(2);
        assert!(!cache.get(&t("a"), &|s| ferroreport::parse(s)).unwrap().1);
        assert!(cache.get(&t("a"), &|s| ferroreport::parse(s)).unwrap().1);
        assert!(!cache.get(&t("b"), &|s| ferroreport::parse(s)).unwrap().1);
        assert!(cache.get(&t("a"), &|s| ferroreport::parse(s)).unwrap().1);
        // A third template pushes out the one used longest ago ("b").
        assert!(!cache.get(&t("c"), &|s| ferroreport::parse(s)).unwrap().1);
        assert!(cache.get(&t("a"), &|s| ferroreport::parse(s)).unwrap().1);
        assert!(!cache.get(&t("b"), &|s| ferroreport::parse(s)).unwrap().1);
        assert_eq!(cache.get(&t("b"), &|s| ferroreport::parse(s)).unwrap().0.name, "b");
        assert!(cache.get("<nonsense", &|s| ferroreport::parse(s)).is_err());
        let parse = |s: &str| ferroreport::parse(s);
        assert!(!TemplateCache::new(0).get(&t("a"), &parse).unwrap().1 && !TemplateCache::new(0).get(&t("a"), &parse).unwrap().1);
        // A template that uses style templates is never kept: they can change on their own.
        let styled = t("s").replace("/>", "><template><![CDATA[\"s.jrtx\"]]></template></jasperReport>");
        assert!(!cache.get(&styled, &parse).unwrap().1 && !cache.get(&styled, &parse).unwrap().1);
    }

    fn config(addr: &str, token: Option<&str>, allow_anonymous: bool) -> Config {
        Config {
            addr: addr.into(),
            templates: None,
            resources: Vec::new(),
            token: token.map(str::to_string),
            max_body: 1 << 20,
            workers: 1,
            limits: Default::default(),
            max_rows: None,
            cache: 0,
            access_log: false,
            allow_anonymous,
            rate_limit: None,
            spool_memory: 1 << 20,
            spool_dir: None,
            spool_disk: 1 << 30,
            connection: None,
            signature: None,
        }
    }

    #[test]
    fn a_report_that_reads_too_many_rows_is_answered_413_and_counted() {
        let mut cfg = config("127.0.0.1:0", None, false);
        cfg.max_rows = Some(10);
        let state = Arc::new(State {
            cache: TemplateCache::new(0),
            metrics: Metrics::default(),
            limiter: RateLimiter::default(),
            spool: Arc::new(SpoolLimits::new(cfg.spool_memory, None, cfg.spool_disk)),
            cfg,
        });
        // Only a total: one page however many rows, sent one per line.
        let template = r#"<jasperReport name="total" pageWidth="200" pageHeight="100" columnWidth="200" leftMargin="0" rightMargin="0" topMargin="0" bottomMargin="0"><field name="n" class="java.lang.Integer"/><summary><band height="20"><textField><reportElement x="0" y="0" width="200" height="20"/><textFieldExpression><![CDATA["rows " + $V{REPORT_COUNT}]]></textFieldExpression></textField></band></summary></jasperReport>"#;
        let request = |rows: usize, more: &[(&str, &str)]| {
            let head = serde_json::json!({ "template": template, "format": "csv" }).to_string();
            let body = std::iter::once(head).chain((0..rows).map(|n| format!("{{\"n\": {n}}}"))).collect::<Vec<_>>().join("\n");
            let mut req = tiny_http::TestRequest::new()
                .with_method(Method::Post)
                .with_path("/render")
                .with_header(Header::from_bytes("Content-Type", "application/x-ndjson").unwrap())
                .with_body(Box::leak(body.into_boxed_str()));
            for (name, value) in more {
                req = req.with_header(Header::from_bytes(*name, *value).unwrap());
            }
            let mut req: Request = req.into();
            let mut stream = None;
            let reply = answer(&state, &mut req, &mut stream);
            match stream {
                Some(streamed) => (200, String::from_utf8_lossy(&streamed.first).into_owned()),
                None => {
                    let status = reply.status_code().0;
                    let mut text = Vec::new();
                    reply.raw_print(&mut text, tiny_http::HTTPVersion(1, 1), &[], false, None).unwrap();
                    (status, String::from_utf8_lossy(&text).into_owned())
                }
            }
        };
        let (status, text) = request(10, &[]);
        assert_eq!(status, 200, "{text}");
        assert!(text.ends_with("rows 10\n"), "{text}");
        let (status, text) = request(50, &[]);
        assert_eq!(status, 413, "{text}");
        assert!(text.contains("the report reads more than 10 rows"), "{text}");
        // The same when the document is sent as it renders: the limit is reached before any of it.
        let (status, text) = request(50, &[("Prefer", "respond-streaming")]);
        assert_eq!(status, 413, "{text}");
        let metrics = state.metrics.text(&state.spool);
        assert!(metrics.contains("\nferroreport_render_limited_total 2\n"), "{metrics}");
        assert!(metrics.contains("\nferroreport_render_errors_total 0\n"), "{metrics}");
        assert!(metrics.contains("\nferroreport_renders_total 1\n"), "{metrics}");
    }

    #[test]
    fn finds_the_parameters_pasted_into_a_query() {
        assert_eq!(pasted_params("SELECT * FROM t WHERE a = $P{A} ORDER BY $P!{ORDER}, $P!{ DIR }"), ["ORDER", "DIR"]);
        assert!(pasted_params("SELECT 1").is_empty());
    }

    #[test]
    fn a_public_address_needs_a_token() {
        assert!(check(&config("127.0.0.1:8080", None, false)).is_ok());
        assert!(check(&config("[::1]:8080", None, false)).is_ok());
        assert!(check(&config("0.0.0.0:8080", None, false)).is_err());
        assert!(check(&config("10.0.0.5:8080", None, false)).is_err());
        assert!(check(&config("0.0.0.0:8080", Some("s3cret"), false)).is_ok());
        assert!(check(&config("0.0.0.0:8080", None, true)).is_ok());
    }

    #[test]
    fn clients_are_limited_per_minute() {
        let limiter = RateLimiter::default();
        let (a, b): (std::net::IpAddr, std::net::IpAddr) = ("10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap());
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(limiter.allow(a, 3, t0).is_ok());
        }
        let wait = limiter.allow(a, 3, t0 + std::time::Duration::from_secs(20)).unwrap_err();
        assert!((39..=40).contains(&wait), "{wait}");
        // Another client is not affected, and the minute starts over.
        assert!(limiter.allow(b, 3, t0).is_ok());
        assert!(limiter.allow(a, 3, t0 + std::time::Duration::from_secs(61)).is_ok());
    }

    #[test]
    fn metrics_are_prometheus_text() {
        let m = Metrics::default();
        m.renders_ok.fetch_add(3, Ordering::Relaxed);
        m.render_micros.fetch_add(1_500_000, Ordering::Relaxed);
        let text = m.text(&SpoolLimits::new(0, None, 0));
        assert!(text.contains("# TYPE ferroreport_renders_total counter\nferroreport_renders_total 3\n"), "{text}");
        assert!(text.contains("ferroreport_render_seconds_total 1.500000\n"), "{text}");
    }

    #[test]
    fn template_names_stay_relative() {
        assert!(safe_name("sales/invoice").is_some());
        for bad in ["", "/etc/passwd", "../x", "a/../../b"] {
            assert!(safe_name(bad).is_none(), "{bad}");
        }
    }
}
