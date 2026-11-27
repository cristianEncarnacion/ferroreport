//! Python bindings: the `ferroreport._ferroreport` extension module. The public API lives in
//! `python/ferroreport/__init__.py`.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::collections::HashMap;

fn err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Renders a JRXML template (source text) with parameters given as JSON text. Rows come as
/// JSON text, or from `data_path` (a JSON array or one object per line), read as they are
/// needed. The document is returned, or written to `output_path` as it is made. Returns
/// (bytes, pages, rows).
#[pyfunction]
#[pyo3(signature = (template, data_json, params_json, locale, resource_dirs, resources, now=None, format=None,
                    max_pages=None, timeout=None, csv_safe=false, xlsx_numbers=false, data_path=None, output_path=None,
                    tagged=false, pdfa=None, attachments=Vec::new(), password=None, owner_password=None,
                    permissions=None, max_rows=None))]
#[allow(clippy::too_many_arguments)]
fn render(
    py: Python<'_>,
    template: String,
    data_json: String,
    params_json: String,
    locale: String,
    resource_dirs: Vec<String>,
    resources: HashMap<String, Vec<u8>>,
    now: Option<String>,
    format: Option<String>,
    max_pages: Option<usize>,
    timeout: Option<f64>,
    csv_safe: bool,
    xlsx_numbers: bool,
    data_path: Option<String>,
    output_path: Option<String>,
    tagged: bool,
    pdfa: Option<String>,
    attachments: Vec<(String, Vec<u8>, String)>,
    password: Option<String>,
    owner_password: Option<String>,
    permissions: Option<String>,
    max_rows: Option<usize>,
) -> PyResult<(Py<PyBytes>, usize, usize)> {
    let pdfa = match pdfa {
        Some(p) => {
            Some(ferroreport::pdf::PdfA::parse(&p).ok_or_else(|| err(format!("pdfa=\"{p}\": say 2b, 2u, 2a, 3b, 3u or 3a")))?)
        }
        None => None,
    };
    let encryption = if password.is_some() || owner_password.is_some() || permissions.is_some() {
        Some(ferroreport::crypt::Encryption {
            user_password: password.unwrap_or_default(),
            owner_password,
            permissions: ferroreport::crypt::Permissions::parse(permissions.as_deref().unwrap_or("")).map_err(err)?,
        })
    } else {
        None
    };
    let format = match format {
        Some(f) => ferroreport::Format::parse(&f)
            .ok_or_else(|| err(format!("unknown format `{f}` (pdf, xlsx, docx, pptx, ods, odt, csv or html)")))?,
        None => ferroreport::Format::Pdf,
    };
    let (bytes, stats) = py
        .detach(move || -> Result<(Vec<u8>, ferroreport::FillStats), String> {
            let params: serde_json::Value = serde_json::from_str(&params_json).map_err(|e| format!("params: {e}"))?;
            let limits =
                ferroreport::Limits { max_pages, timeout: timeout.filter(|t| *t > 0.0).map(std::time::Duration::from_secs_f64) };
            let mut r = ferroreport::Renderer::new()
                .locale(&locale)
                .limits(limits)
                .max_rows(max_rows)
                .xlsx_options(ferroreport::xlsx::XlsxOptions { detect_cell_type: xlsx_numbers })
                .tagged(tagged)
                .pdfa(pdfa)
                .encrypt(encryption)
                .csv_options(ferroreport::csv::CsvOptions { guard_formulas: csv_safe, ..Default::default() });
            for (name, data, mime) in attachments {
                r = r.attach(ferroreport::pdf::Attachment {
                    name,
                    data: std::sync::Arc::new(data),
                    mime,
                    description: None,
                    relationship: "Data".into(),
                });
            }
            if let Some(now) = now {
                let t = ferroreport::data::parse_datetime(&now).ok_or_else(|| format!("now: cannot parse `{now}`"))?;
                r = r.now(t);
            }
            for dir in resource_dirs {
                r = r.resource_dir(dir);
            }
            for (name, data) in resources {
                r.add_resource(&name, data);
            }
            let report = r.parse(&template).map_err(|e| e.to_string())?;
            let params = ferroreport::params_from_json(&report, &params).map_err(|e| e.to_string())?;
            // The rows from the start, each time they are asked for: a large workbook reads
            // them twice (`Renderer::render_rows_to`).
            let mut rows = || -> ferroreport::Result<ferroreport::RowIter> {
                Ok(match &data_path {
                    Some(path) => {
                        let file = std::fs::File::open(path).map_err(|e| ferroreport::Error::Io(format!("{path}: {e}")))?;
                        Box::new(ferroreport::JsonRows::new(&report, file))
                    }
                    None => Box::new(ferroreport::JsonRows::new(&report, data_json.as_bytes())),
                })
            };
            match output_path {
                Some(path) => {
                    // Written beside the output and moved into place, so a failed render
                    // leaves no half-written file.
                    let part = format!("{path}.part");
                    let file = std::fs::File::create(&part).map_err(|e| format!("{path}: {e}"))?;
                    let out = std::io::BufWriter::with_capacity(1 << 16, file);
                    match r.render_rows_to(&report, &mut rows, params, format, out) {
                        Ok(stats) => {
                            std::fs::rename(&part, &path).map_err(|e| format!("{path}: {e}"))?;
                            Ok((Vec::new(), stats))
                        }
                        Err(e) => {
                            let _ = std::fs::remove_file(&part);
                            Err(e.to_string())
                        }
                    }
                }
                None => {
                    let mut out = Vec::new();
                    let stats = r.render_rows_to(&report, &mut rows, params, format, &mut out).map_err(|e| e.to_string())?;
                    Ok((out, stats))
                }
            }
        })
        .map_err(err)?;
    Ok((PyBytes::new(py, &bytes).unbind(), stats.pages, stats.rows))
}

/// Parses a template and reports what this engine does not support in it.
#[pyfunction]
fn inspect(py: Python<'_>, template: String) -> PyResult<Py<PyAny>> {
    let report = ferroreport::parse(&template).map_err(err)?;
    let out = pyo3::types::PyDict::new(py);
    out.set_item("name", &report.name)?;
    out.set_item(
        "parameters",
        report
            .parameters
            .iter()
            .filter(|p| p.name != ferroreport::text::HYPHENATE && p.name != ferroreport::text::PARAGRAPHS)
            .map(|p| p.name.clone())
            .collect::<Vec<_>>(),
    )?;
    out.set_item("fields", report.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>())?;
    out.set_item("warnings", &report.warnings)?;
    Ok(out.into_any().unbind())
}

/// Rewrites a template: `to` is "ferro" (ferroreport's own format) or "jrxml" (JRXML 7).
#[pyfunction]
fn convert(template: String, to: String) -> PyResult<String> {
    convert_template(&template, &to).map_err(err)
}

fn convert_template(template: &str, to: &str) -> Result<String, String> {
    use ferroreport::{convert, template as own};
    let r = match (own::is_template(template), to) {
        (true, "jrxml") => own::to_jrxml(template),
        (true, "ferro") => own::to_jrxml(template).and_then(|x| own::from_jrxml(&x)),
        (false, "jrxml") => convert::jrxml_to_v7(template),
        (false, "ferro") => own::from_jrxml(template),
        (_, other) => return Err(format!("to={other:?}: use \"ferro\" or \"jrxml\"")),
    };
    r.map_err(|e| e.to_string())
}

#[pymodule]
fn _ferroreport(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(render, m)?)?;
    m.add_function(wrap_pyfunction!(inspect, m)?)?;
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
