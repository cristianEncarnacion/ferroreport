//! WebAssembly bindings: `render(template, data, params, options)` returning the PDF (or XLSX,
//! DOCX, PPTX, ODS, ODT, CSV or HTML) bytes.

use js_sys::{Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn to_json(v: &JsValue, default: serde_json::Value) -> Result<serde_json::Value, JsError> {
    if v.is_undefined() || v.is_null() {
        return Ok(default);
    }
    if let Some(s) = v.as_string() {
        return serde_json::from_str(&s).map_err(js_err);
    }
    serde_wasm_bindgen::from_value(v.clone()).map_err(|e| JsError::new(&e.to_string()))
}

fn get(obj: &JsValue, key: &str) -> JsValue {
    if obj.is_object() {
        Reflect::get(obj, &JsValue::from_str(key)).unwrap_or(JsValue::UNDEFINED)
    } else {
        JsValue::UNDEFINED
    }
}

/// `options.resources` as (path, bytes) pairs.
fn resources(options: &JsValue) -> Vec<(String, Vec<u8>)> {
    let resources = get(options, "resources");
    if !resources.is_object() {
        return Vec::new();
    }
    Object::entries(&Object::from(resources))
        .iter()
        .map(|entry| {
            let pair = js_sys::Array::from(&entry);
            let name = pair.get(0).as_string().unwrap_or_default();
            let value = pair.get(1);
            let bytes = match value.as_string() {
                Some(s) => s.into_bytes(),
                None => Uint8Array::new(&value).to_vec(),
            };
            (name, bytes)
        })
        .collect()
}

/// Renders a JRXML template to PDF (or to Excel or CSV with `options.format`).
///
/// - `template`: the JRXML source.
/// - `data`: rows (an array of objects) or JSON text.
/// - `params`: report parameters (an object) or JSON text.
/// - `options`: `{ locale?: string, now?: string | Date, resources?: { [path]: Uint8Array | string },
///   format?: "pdf" | "xlsx" | "docx" | "pptx" | "ods" | "odt" | "csv" | "html", maxPages?: number, maxRows?: number, csvSafe?: boolean,
///   xlsxNumbers?: boolean,
///   tagged?: boolean, pdfa?: "2b" | "2u" | "2a" | "3b" | "3u" | "3a",
///   attachments?: { name: string, data: Uint8Array | string, mime?: string }[],
///   password?: string, ownerPassword?: string, permissions?: string | string[] }`
///   (`tagged` writes a tagged PDF, for screen readers; `pdfa` a PDF/A document, for archiving;
///   `attachments` are files carried inside the PDF, which PDF/A-3 allows; `password`,
///   `ownerPassword` and `permissions` — "print", "copy", "modify", "annotate", "fill",
///   "accessibility", "assemble", "all" — encrypt the PDF with AES-256).
///   `maxPages` stops a report that grows past that many pages, and `maxRows` one that reads
///   more than that many rows (those of its subreports and datasets too); `csvSafe` puts an
///   apostrophe before CSV values a spreadsheet would run as a formula; `xlsxNumbers` writes
///   numbers as numbers in Excel and ODS instead of formatted text.
#[wasm_bindgen]
pub fn render(template: &str, data: JsValue, params: JsValue, options: JsValue) -> Result<Vec<u8>, JsError> {
    let rows = to_json(&data, serde_json::Value::Array(Vec::new()))?;
    let params = to_json(&params, serde_json::Value::Object(Default::default()))?;
    let locale = get(&options, "locale").as_string().unwrap_or_else(|| "en_US".into());
    let limits =
        ferroreport::Limits { max_pages: get(&options, "maxPages").as_f64().map(|n| n.max(0.0) as usize), timeout: None };
    let mut r = ferroreport::Renderer::new()
        .locale(&locale)
        .limits(limits)
        .max_rows(get(&options, "maxRows").as_f64().map(|n| n.max(0.0) as usize))
        .xlsx_options(ferroreport::xlsx::XlsxOptions { detect_cell_type: get(&options, "xlsxNumbers").is_truthy() })
        .tagged(get(&options, "tagged").is_truthy())
        .csv_options(ferroreport::csv::CsvOptions { guard_formulas: get(&options, "csvSafe").is_truthy(), ..Default::default() });
    if let Some(p) = get(&options, "pdfa").as_string() {
        let pdfa = ferroreport::pdf::PdfA::parse(&p)
            .ok_or_else(|| JsError::new(&format!("pdfa \"{p}\": say 2b, 2u, 2a, 3b, 3u or 3a")))?;
        r = r.pdfa(Some(pdfa));
    }
    let (password, owner) = (get(&options, "password").as_string(), get(&options, "ownerPassword").as_string());
    let permissions = get(&options, "permissions");
    let permissions = if js_sys::Array::is_array(&permissions) {
        Some(js_sys::Array::from(&permissions).iter().filter_map(|p| p.as_string()).collect::<Vec<_>>().join(","))
    } else {
        permissions.as_string()
    };
    if password.is_some() || owner.is_some() || permissions.is_some() {
        let permissions = ferroreport::crypt::Permissions::parse(permissions.as_deref().unwrap_or(""))
            .map_err(|e| JsError::new(&e.to_string()))?;
        r = r.encrypt(Some(ferroreport::crypt::Encryption {
            user_password: password.unwrap_or_default(),
            owner_password: owner,
            permissions,
        }));
    }
    let files = get(&options, "attachments");
    if js_sys::Array::is_array(&files) {
        for f in js_sys::Array::from(&files).iter() {
            let name = get(&f, "name").as_string().ok_or_else(|| JsError::new("an attachment needs a name"))?;
            let data = get(&f, "data");
            let bytes = match data.as_string() {
                Some(s) => s.into_bytes(),
                None => js_sys::Uint8Array::new(&data).to_vec(),
            };
            let mime = get(&f, "mime").as_string().unwrap_or_else(|| "application/octet-stream".into());
            r = r.attach(ferroreport::pdf::Attachment {
                name,
                data: std::sync::Arc::new(bytes),
                mime,
                description: None,
                relationship: "Data".into(),
            });
        }
    }

    // `new Date()` in expressions: the given value, or the current time from JavaScript.
    let now = get(&options, "now");
    let now = if let Some(s) = now.as_string() {
        s
    } else if now.is_instance_of::<js_sys::Date>() {
        String::from(js_sys::Date::from(now).to_iso_string())
    } else {
        String::from(js_sys::Date::new_0().to_iso_string())
    };
    let now = ferroreport::data::parse_datetime(now.trim_end_matches('Z'))
        .ok_or_else(|| JsError::new(&format!("options.now: cannot parse `{now}`")))?;
    r = r.now(now);

    for (name, bytes) in resources(&options) {
        r.add_resource(&name, bytes);
    }
    let format = match get(&options, "format").as_string() {
        Some(f) => ferroreport::Format::parse(&f).ok_or_else(|| {
            JsError::new(&format!("options.format: unknown format `{f}` (pdf, xlsx, docx, pptx, ods, odt, csv or html)"))
        })?,
        None => ferroreport::Format::Pdf,
    };
    r.render_json_as(template, &rows, &params, format).map_err(js_err)
}

/// Parses a template: `{ name, parameters, fields, warnings }`, where `warnings` lists what
/// this engine does not support in it.
#[wasm_bindgen]
pub fn inspect(template: &str) -> Result<JsValue, JsError> {
    let report = ferroreport::parse(template).map_err(js_err)?;
    let out = serde_json::json!({
        "name": report.name,
        "parameters": report.parameters.iter().filter(|p| p.name != ferroreport::text::HYPHENATE && p.name != ferroreport::text::PARAGRAPHS).map(|p| p.name.clone()).collect::<Vec<_>>(),
        "fields": report.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        "warnings": report.warnings,
    });
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    serde::Serialize::serialize(&out, &ser).map_err(|e| JsError::new(&e.to_string()))
}

/// Rewrites a template: `to` is "ferro" (ferroreport's own format) or "jrxml" (JRXML as
/// JasperReports 7 writes it). Both are rendered the same.
#[wasm_bindgen]
pub fn convert(template: &str, to: &str) -> Result<String, JsError> {
    use ferroreport::{convert, template as own};
    let r = match (own::is_template(template), to) {
        (true, "jrxml") => own::to_jrxml(template),
        (true, "ferro") => own::to_jrxml(template).and_then(|x| own::from_jrxml(&x)),
        (false, "jrxml") => convert::jrxml_to_v7(template),
        (false, "ferro") => own::from_jrxml(template),
        (_, other) => return Err(JsError::new(&format!("to={other:?}: use \"ferro\" or \"jrxml\""))),
    };
    r.map_err(js_err)
}

/// Reads a template (JRXML 6 or 7, or `.ferro`) as a tree an editor can change: its JRXML 7
/// form, one `{ tag, attrs, children, text? }` object per element, with `attrs` as
/// `[name, value]` pairs in order. `options.resources` holds the files a `.ferro` template
/// imports (`{ "parts.ferro": text }`).
#[wasm_bindgen(js_name = toTree)]
pub fn to_tree(template: &str, options: JsValue) -> Result<JsValue, JsError> {
    let files = resources(&options);
    let load = |name: &str| files.iter().find(|(n, _)| n == name).map(|(_, b)| b.clone());
    let tree = ferroreport::design::to_tree(template, Some(&load)).map_err(js_err)?;
    let ser = serde_wasm_bindgen::Serializer::json_compatible();
    serde::Serialize::serialize(&tree, &ser).map_err(|e| JsError::new(&e.to_string()))
}

/// Writes a tree from `toTree` (changed or not) as a template: `to` is "jrxml" (the default,
/// as JasperReports 7 writes it) or "ferro".
#[wasm_bindgen(js_name = fromTree)]
pub fn from_tree(tree: JsValue, to: Option<String>) -> Result<String, JsError> {
    let tree = to_json(&tree, serde_json::Value::Null)?;
    let jrxml = ferroreport::design::from_tree(&tree).map_err(js_err)?;
    match to.as_deref().unwrap_or("jrxml") {
        "jrxml" => Ok(jrxml),
        "ferro" => ferroreport::template::from_jrxml(&jrxml).map_err(js_err),
        other => Err(JsError::new(&format!("to={other:?}: use \"ferro\" or \"jrxml\""))),
    }
}

/// Version of the engine.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
