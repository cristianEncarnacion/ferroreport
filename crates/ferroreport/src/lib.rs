//! # ferroreport
//!
//! Renders JasperReports JRXML templates (and templates in its own format, see [`template`]) to
//! PDF, Excel (XLSX), Word (DOCX), PowerPoint (PPTX), OpenDocument (ODS, ODT), CSV and HTML
//! without a JVM.
//!
//! ```no_run
//! let template = std::fs::read_to_string("invoice.jrxml")?;
//! let data: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("rows.json")?)?;
//! let pdf = ferroreport::Renderer::new().render_json(&template, &data, &Default::default())?;
//! std::fs::write("invoice.pdf", pdf)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The expression language is interpreted from an allow-list of operations: templates cannot
//! load classes, deserialize objects or touch the file system, so rendering templates from
//! untrusted sources is safe.

pub mod barcode;
pub mod chart;
pub mod convert;
pub mod crosstab;
pub mod crypt;
pub mod csv;
pub mod data;
pub mod design;
pub mod docx;
pub mod error;
pub mod expr;
pub mod fill;
pub mod fonts;
pub mod format;
mod grid;
pub mod html;
mod iconlabel;
pub mod image;
pub mod jrxml;
pub mod model;
mod odf;
pub mod ods;
pub mod odt;
mod ooxml;
mod paragraph;
pub mod pdf;
pub mod pptx;
pub mod print;
mod raster;
pub mod rich;
pub mod rtf;
pub mod shape;
pub mod sign;
pub mod sql;
pub mod standard;
pub mod svg;
pub mod table;
pub mod template;
pub mod text;
pub mod value;
pub mod xlsx;
mod xml;
mod zip;

pub use data::{JsonRows, Record};
pub use error::{Error, Result};
pub use fill::{FillStats, Limits, PageSink, RowIter};
pub use format::Locale;
pub use model::Report;
pub use print::PrintDocument;
pub use value::Value;

use chrono::NaiveDateTime;
use fonts::FontSet;
use std::collections::HashMap;

/// Parses a JRXML template. Style templates it uses (`<template>`, `.jrtx` files) are not
/// read: use [`Renderer::parse`], which finds them where it finds images and subreports.
pub fn parse(template: &str) -> Result<Report> {
    jrxml::parse_report(template)
}

/// An output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Format {
    #[default]
    Pdf,
    /// Excel workbook, laid out like JasperReports' XLSX exporter.
    Xlsx,
    /// Comma-separated values, laid out like JasperReports' CSV exporter.
    Csv,
    /// A web page with every page of the report drawn as it is in the PDF, for previews.
    Html,
    /// Word document, laid out like JasperReports' DOCX exporter.
    Docx,
    /// OpenDocument spreadsheet (LibreOffice Calc), laid out like JasperReports' ODS exporter.
    Ods,
    /// OpenDocument text document (LibreOffice Writer), laid out like JasperReports' ODT
    /// exporter.
    Odt,
    /// PowerPoint presentation, a slide per page, laid out like JasperReports' PPTX exporter.
    Pptx,
}

impl Format {
    /// `pdf`, `xlsx`, `csv`, `html`, `docx`, `ods`, `odt` or `pptx` (any case).
    pub fn parse(name: &str) -> Option<Format> {
        match name.to_ascii_lowercase().as_str() {
            "pdf" => Some(Format::Pdf),
            "xlsx" | "excel" => Some(Format::Xlsx),
            "csv" => Some(Format::Csv),
            "html" | "htm" => Some(Format::Html),
            "docx" | "word" => Some(Format::Docx),
            "ods" => Some(Format::Ods),
            "odt" => Some(Format::Odt),
            "pptx" | "powerpoint" => Some(Format::Pptx),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Xlsx => "xlsx",
            Format::Csv => "csv",
            Format::Html => "html",
            Format::Docx => "docx",
            Format::Ods => "ods",
            Format::Odt => "odt",
            Format::Pptx => "pptx",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Format::Pdf => "application/pdf",
            Format::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Format::Csv => "text/csv; charset=utf-8",
            Format::Html => "text/html; charset=utf-8",
            Format::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Format::Ods => ods::MIMETYPE,
            Format::Odt => odt::MIMETYPE,
            Format::Pptx => pptx::MIMETYPE,
        }
    }
}

/// Renders one report: parse -> fill -> export.
pub struct Renderer {
    fonts: FontSet,
    images: image::ImageStore,
    locale: &'static Locale,
    now: Option<NaiveDateTime>,
    limits: Limits,
    xlsx: xlsx::XlsxOptions,
    csv: csv::CsvOptions,
    html: html::HtmlOptions,
    tagged: bool,
    pdfa: Option<pdf::PdfA>,
    attachments: Vec<pdf::Attachment>,
    encryption: Option<crypt::Encryption>,
    signature: Option<sign::Signature>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer {
            fonts: FontSet::new(),
            images: image::ImageStore::new(),
            locale: &format::locale::EN_US,
            now: None,
            limits: Limits::default(),
            xlsx: Default::default(),
            tagged: false,
            pdfa: None,
            attachments: Vec::new(),
            encryption: None,
            signature: None,
            csv: Default::default(),
            html: Default::default(),
        }
    }

    /// Sets the locale used for number and date formatting (`en_US`, `es_DO`, `es_ES`, ...).
    pub fn locale(mut self, tag: &str) -> Renderer {
        self.locale = Locale::from_tag(tag);
        self
    }

    /// Fixes the value of `new java.util.Date()` (useful for reproducible output).
    pub fn now(mut self, now: NaiveDateTime) -> Renderer {
        self.now = Some(now);
        self
    }

    /// Stops a render that goes over `limits` (pages, time) with [`Error::Limit`].
    pub fn limits(mut self, limits: Limits) -> Renderer {
        self.limits = limits;
        self
    }

    /// Stops a render that reads more than `rows` rows with [`Error::Limit`], as soon as it
    /// reads one more: a report that reads many rows but prints few pages (a summary of
    /// totals, a chart over a large data set) stops before it has read them all. Every row
    /// counts: the report's own and those of its subreports, tables, lists, charts and
    /// crosstabs. `None` (the default) sets no limit.
    pub fn max_rows(mut self, rows: Option<usize>) -> Renderer {
        self.images.max_rows = rows;
        self
    }

    /// Writes PDFs that say what their content is (tagged PDF), for screen readers and for
    /// software that extracts text: each text element is a paragraph and each image, chart
    /// and barcode a figure, in reading order; lines, borders and backgrounds are marked as
    /// decoration. An image is described by its `hyperlinkTooltipExpression`. The document
    /// carries its title and the language of the renderer's locale.
    pub fn tagged(mut self, tagged: bool) -> Renderer {
        self.tagged = tagged;
        self
    }

    /// Writes PDF/A documents, for archiving: every font inside the document (a text in a PDF
    /// standard font is an error, as in JasperReports), an sRGB output intent, the conformance
    /// in the metadata. Level A also tags the document. Overrides the template's
    /// `net.sf.jasperreports.export.pdfa.conformance`.
    pub fn pdfa(mut self, pdfa: Option<pdf::PdfA>) -> Renderer {
        self.pdfa = pdfa;
        self
    }

    /// Carries a file inside the PDFs written (an e-invoice's XML, say). With PDF/A, only
    /// PDF/A-3 allows it.
    pub fn attach(mut self, file: pdf::Attachment) -> Renderer {
        self.attachments.push(file);
        self
    }

    /// Encrypts the PDFs written (AES-256): a password to open them, and permissions (printing,
    /// copying…) that an owner password lifts. Overrides the template's
    /// `net.sf.jasperreports.export.pdf.encrypted` and its siblings.
    pub fn encrypt(mut self, encryption: Option<crypt::Encryption>) -> Renderer {
        self.encryption = encryption;
        self
    }

    /// Signs PDF output (PAdES B-B) with `signature`'s key and certificates.
    pub fn sign(mut self, signature: Option<sign::Signature>) -> Renderer {
        self.signature = signature;
        self
    }

    fn pdf_options(&self, title: &str, settings: &model::PdfSettings) -> pdf::PdfOptions {
        pdf::PdfOptions {
            signature: self.signature.clone(),
            signed_at: self.now,
            encryption: self.encryption.clone().or_else(|| settings.encryption.clone()),
            pdfa: self.pdfa.or(settings.pdfa),
            attachments: self.attachments.clone(),
            title: Some(title.to_string()),
            compress: true,
            tagged: self.tagged || settings.tagged,
            lang: Some(settings.language.clone().unwrap_or_else(|| self.locale.tag.replace('_', "-"))),
            page_offsets: settings.page_offsets,
        }
    }

    /// Options for Excel output from [`Renderer::export`] and the `render` methods.
    pub fn xlsx_options(mut self, options: xlsx::XlsxOptions) -> Renderer {
        self.xlsx = options;
        self
    }

    /// Options for CSV output from [`Renderer::export`] and the `render` methods.
    pub fn csv_options(mut self, options: csv::CsvOptions) -> Renderer {
        self.csv = options;
        self
    }

    /// Options for HTML output from [`Renderer::export`] and the `render` methods.
    pub fn html_options(mut self, options: html::HtmlOptions) -> Renderer {
        self.html = options;
        self
    }

    /// Adds a directory where relative resource paths (images, subreports) are looked up. Paths in a
    /// template can never leave these directories.
    pub fn resource_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Renderer {
        self.images.add_root(dir);
        self
    }

    /// Adds a resource (image or subreport template) held in memory, under the relative path
    /// templates use for it. Useful where there is no file system (WebAssembly) or when
    /// templates come from a database.
    pub fn resource(mut self, name: &str, data: impl Into<std::sync::Arc<[u8]>>) -> Renderer {
        self.images.add_file(name, data);
        self
    }

    /// Like [`Renderer::resource`], for a renderer already built.
    pub fn add_resource(&mut self, name: &str, data: impl Into<std::sync::Arc<[u8]>>) {
        self.images.add_file(name, data);
    }

    /// Registers an additional TrueType/OpenType font family.
    pub fn register_font(&mut self, family: &str, bold: bool, italic: bool, data: Vec<u8>) -> Result<()> {
        self.fonts.register(family, bold, italic, data)
    }

    /// Registers a font set, like a `<fontSet>` in a JasperReports font extension: text whose
    /// font name is `name` is drawn in `families` (registered ones or the bundled DejaVu
    /// families), each run of characters in the first family that has them all; characters
    /// none of them has stay in the first family, which is the primary one.
    pub fn register_font_set(&mut self, name: &str, families: &[&str]) -> Result<()> {
        self.fonts.register_set(name, families)
    }

    /// Registers the fonts of the JasperReports font extensions in the renderer's resources,
    /// as JasperReports finds them on its class path: each resource folder (and the files
    /// added with [`Renderer::add_resource`]) with a `jasperreports_extension.properties` at
    /// its root is read for the `fonts.xml` files it names
    /// (`net.sf.jasperreports.extension.simple.font.families.*`), whose `<fontFamily>` faces
    /// and `<fontSet>`s are registered as [`Renderer::register_font`] and
    /// [`Renderer::register_font_set`] would. Font files are looked up in the resources by
    /// the path the extension gives them. Returns what could not be registered (a missing
    /// file, a font that does not parse); the rest is registered.
    pub fn load_font_extensions(&mut self) -> Vec<String> {
        let properties = self.images.read_every(fonts::EXTENSION_PROPERTIES);
        if properties.is_empty() {
            return Vec::new();
        }
        let images = &self.images;
        fonts::load_extensions(&mut self.fonts, &properties, &|location| images.read_resource(location))
    }

    /// Draws characters a text's font lacks in another font that has them (off by default:
    /// JasperReports leaves them out unless the font is a font set). The fonts are looked at
    /// in this order: the registered families, in the order they were registered, then the
    /// bundled DejaVu Sans, DejaVu Serif and DejaVu Sans Mono. As with a font set, the longest
    /// run of characters one font has is drawn in that font, so a font that has the missing
    /// character and the ones around it draws them all. Texts whose characters are all in
    /// their font are not affected.
    pub fn font_fallback(mut self, on: bool) -> Renderer {
        self.fonts.set_fallback(on);
        self
    }

    /// Like [`Renderer::font_fallback`], for a renderer already built.
    pub fn set_font_fallback(&mut self, on: bool) {
        self.fonts.set_fallback(on);
    }

    /// Fills a parsed report into a print document.
    pub fn fill(&mut self, report: &Report, records: &[Record], params: HashMap<String, Value>) -> Result<PrintDocument> {
        let env = self.env(report);
        fill::fill_slice(report, records, params, &mut self.fonts, &mut self.images, &env, self.limits)
    }

    fn env(&self, report: &Report) -> expr::EvalEnv<'static> {
        expr::EvalEnv {
            locale: self.locale,
            groovy: report.language == model::Language::Groovy,
            now: self.now.unwrap_or_else(|| chrono::Utc::now().naive_utc()),
        }
    }

    /// Fills a report whose rows arrive one at a time, handing each page to `sink` as soon as
    /// it is finished. Use it to write a format of your own, or to send pages somewhere
    /// as they are made.
    pub fn fill_to(
        &mut self,
        report: &Report,
        rows: RowIter,
        params: HashMap<String, Value>,
        sink: &mut dyn PageSink,
    ) -> Result<FillStats> {
        let env = self.env(report);
        fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, sink)
    }

    /// Renders a report whose rows arrive one at a time, writing the output as it goes:
    /// PDF and CSV pages leave memory as soon as they are finished, so a report of a
    /// million rows needs about as much memory as one of a thousand. (An Excel workbook
    /// needs its rows twice to be written that way: see [`Renderer::render_rows_to`]. Here
    /// they can only be read once, so the workbook is built in memory.)
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let report = ferroreport::parse(&std::fs::read_to_string("sales.jrxml")?)?;
    /// let rows = ferroreport::JsonRows::new(&report, std::fs::File::open("sales.json")?);
    /// let out = std::io::BufWriter::new(std::fs::File::create("sales.pdf")?);
    /// ferroreport::Renderer::new().render_to(&report, Box::new(rows), Default::default(), ferroreport::Format::Pdf, out)?;
    /// # Ok(()) }
    /// ```
    pub fn render_to<W: std::io::Write>(
        &mut self,
        report: &Report,
        rows: RowIter,
        params: HashMap<String, Value>,
        format: Format,
        mut out: W,
    ) -> Result<FillStats> {
        let env = self.env(report);
        match format {
            Format::Pdf => {
                let options = self.pdf_options(&report.name, &report.pdf);
                // A signature covers the whole file: it is written in memory, signed, then sent.
                if let Some(signature) = options.signature.clone() {
                    let mut sink = pdf::PdfStream::new(Vec::new(), report.page_width, report.page_height, options)?;
                    let stats =
                        fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
                    let mut pdf = sink.finish(&self.fonts)?;
                    sign::apply(&mut pdf, &signature)?;
                    out.write_all(&pdf).and_then(|_| out.flush()).map_err(|e| Error::Io(e.to_string()))?;
                    return Ok(stats);
                }
                let mut sink = pdf::PdfStream::new(out, report.page_width, report.page_height, options)?;
                let stats = fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
                sink.finish(&self.fonts)?;
                Ok(stats)
            }
            Format::Csv => {
                let mut sink = csv::CsvStream::new(out, report.page_width, report.page_height, self.csv.clone());
                let stats = fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
                sink.finish()?;
                Ok(stats)
            }
            Format::Html => {
                let mut sink =
                    html::HtmlStream::new(out, &report.name, report.page_width, report.page_height, self.html.clone())?;
                let stats = fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
                sink.finish()?;
                Ok(stats)
            }
            Format::Docx => {
                let mut sink = docx::DocxStream::new(out, &report.name, report.page_width, report.page_height, report.landscape)?;
                let stats = fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
                sink.finish()?;
                Ok(stats)
            }
            Format::Odt => {
                let mut sink = odt::OdtStream::new(out, &report.name, report.page_width, report.page_height, report.landscape)?;
                let stats = fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
                sink.finish()?;
                Ok(stats)
            }
            Format::Pptx => {
                let mut sink = pptx::PptxStream::new(out, &report.name, report.page_width, report.page_height)?;
                let stats = fill::fill_to(report, rows, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
                sink.finish()?;
                Ok(stats)
            }
            Format::Xlsx | Format::Ods => {
                let mut count = 0;
                let rows = rows.inspect(|_| count += 1);
                let doc = fill::fill_rows(report, Box::new(rows), params, &mut self.fonts, &mut self.images, &env, self.limits)?;
                let bytes = self.export(&doc, format)?;
                out.write_all(&bytes).and_then(|_| out.flush()).map_err(|e| Error::Io(e.to_string()))?;
                Ok(FillStats { pages: doc.pages.len(), rows: count })
            }
        }
    }

    /// Exports a filled document to PDF.
    pub fn to_pdf(&self, doc: &PrintDocument) -> Result<Vec<u8>> {
        pdf::write_pdf(doc, &self.fonts, &self.pdf_options(&doc.name, &doc.pdf))
    }

    /// Exports a filled document to an Excel workbook (XLSX), laid out like JasperReports'
    /// XLSX exporter.
    pub fn to_xlsx(&self, doc: &PrintDocument, options: &xlsx::XlsxOptions) -> Result<Vec<u8>> {
        xlsx::write_xlsx(doc, &self.fonts, options)
    }

    /// Exports a filled document to an OpenDocument spreadsheet (ODS), laid out like
    /// JasperReports' ODS exporter. Numbers are written as numbers when `options` asks for it
    /// ([`xlsx::XlsxOptions::detect_cell_type`]), as in a workbook.
    pub fn to_ods(&self, doc: &PrintDocument, options: &xlsx::XlsxOptions) -> Result<Vec<u8>> {
        ods::write_ods(doc, &self.fonts, options)
    }

    /// Renders a report whose rows can be read again from the start: `rows` gives a new
    /// iterator over them each time it is called (open the file again, run the query again).
    ///
    /// PDF, CSV, HTML, Word and PowerPoint read them once, like [`Renderer::render_to`]. An Excel
    /// workbook needs them twice when it is large: the columns of its sheet come from every
    /// page, so the report is filled once to find them and once more to write the rows, which
    /// are compressed and sent on as they are made. A workbook of a million rows then takes
    /// about the memory of one of a thousand. Small workbooks are still filled once.
    ///
    /// OpenDocument puts the styles of a document before its body, and they are only all
    /// known at its end. A large OpenDocument text is filled once to find them and once more
    /// to write its pages as they are made; a large OpenDocument spreadsheet is filled three
    /// times (its columns, its styles, its rows). Both then take about the memory of a small
    /// one. Small ones are filled once.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let report = ferroreport::parse(&std::fs::read_to_string("sales.jrxml")?)?;
    /// let mut rows = || -> ferroreport::Result<ferroreport::RowIter> {
    ///     let file = std::fs::File::open("sales.json").map_err(|e| ferroreport::Error::Io(e.to_string()))?;
    ///     Ok(Box::new(ferroreport::JsonRows::new(&report, file)))
    /// };
    /// let out = std::io::BufWriter::new(std::fs::File::create("sales.xlsx")?);
    /// ferroreport::Renderer::new().render_rows_to(&report, &mut rows, Default::default(), ferroreport::Format::Xlsx, out)?;
    /// # Ok(()) }
    /// ```
    pub fn render_rows_to<'a, W: std::io::Write>(
        &mut self,
        report: &Report,
        rows: &mut dyn FnMut() -> Result<RowIter<'a>>,
        params: HashMap<String, Value>,
        format: Format,
        out: W,
    ) -> Result<FillStats> {
        // One environment for every pass: `new Date()` must say the same in each.
        let env = self.env(report);
        let page = (report.page_width, report.page_height, report.landscape);
        if format == Format::Odt {
            let stream = odt::OdtStream::new(out, &report.name, page.0, page.1, page.2)?;
            let mut first = odt::FirstPass::new(stream);
            let stats =
                fill::fill_to(report, rows()?, params.clone(), &mut self.fonts, &mut self.images, &env, self.limits, &mut first)?;
            if !first.stream.measuring() {
                // Small enough to have been kept: written from this pass.
                first.stream.finish()?;
                return Ok(stats);
            }
            let (styles, out) = first.stream.into_styles()?;
            let mut sink = odt::OdtStream::streamed(out, &report.name, page, styles)?;
            let stats = fill::fill_to(report, rows()?, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sink)?;
            sink.finish()?;
            return Ok(stats);
        }
        if format != Format::Xlsx && format != Format::Ods {
            return self.render_to(report, rows()?, params, format, out);
        }
        let mut first = xlsx::FirstPass::new();
        let stats =
            fill::fill_to(report, rows()?, params.clone(), &mut self.fonts, &mut self.images, &env, self.limits, &mut first)?;
        let (columns, kept) = first.done();
        let (w, h, landscape) = page;
        let mut sheet: Sheet<W> = match (format, &kept) {
            (Format::Ods, Some(_)) => {
                Sheet::Ods(Box::new(ods::OdsStream::new(out, columns, &report.name, w, h, landscape, self.xlsx.clone())?))
            }
            (Format::Ods, None) => {
                // The styles of the sheet, from a pass that keeps nothing, so that its rows can
                // be written as they are made in the next.
                let mut measure = ods::OdsStream::measuring(out, columns.clone(), &report.name, page, self.xlsx.clone())?;
                fill::fill_to(
                    report,
                    rows()?,
                    params.clone(),
                    &mut self.fonts,
                    &mut self.images,
                    &env,
                    self.limits,
                    &mut measure,
                )?;
                let (styles, out) = measure.into_styles()?;
                Sheet::Ods(Box::new(ods::OdsStream::streamed(out, columns, &report.name, page, self.xlsx.clone(), styles)?))
            }
            _ => Sheet::Xlsx(Box::new(xlsx::XlsxStream::new(out, columns, &report.name, w, h, landscape, self.xlsx.clone())?)),
        };
        let stats = match kept {
            // A report that fits in memory is written from the pages of the first pass.
            Some(pages) => {
                for page in &pages {
                    sheet.add(&page.items, &self.fonts)?;
                }
                stats
            }
            None => fill::fill_to(report, rows()?, params, &mut self.fonts, &mut self.images, &env, self.limits, &mut sheet)?,
        };
        sheet.finish()?;
        Ok(stats)
    }

    /// The database that reports, subreports, tables, lists, charts and crosstabs with a
    /// query get their rows from (a subreport or dataset run uses it when it has a
    /// `connectionExpression`). The engine has no driver of its own: see [`sql::Connection`].
    pub fn connection(mut self, connection: Option<std::sync::Arc<dyn sql::Connection>>) -> Renderer {
        self.images.connection = connection.map(sql::Shared);
        self
    }

    /// Renders `report` with the rows of its own query, run on the renderer's
    /// [`Renderer::connection`] with `params` (and the defaults of the other parameters), as
    /// JasperReports fills a report from a JDBC connection. Rows are read from the database
    /// as the report needs them.
    pub fn render_query_to<W: std::io::Write>(
        &mut self,
        report: &Report,
        params: HashMap<String, Value>,
        format: Format,
        out: W,
    ) -> Result<FillStats> {
        let connection = self
            .images
            .connection
            .clone()
            .ok_or_else(|| Error::Data("render_query_to needs a connection (Renderer::connection)".into()))?;
        if report.query.is_none() {
            return Err(Error::Data(format!("`{}` has no query to fill it from a database", report.name)));
        }
        let resolved = fill::resolve_params(report, params.clone(), &self.env(report))?;
        let conn: &dyn sql::Connection = connection.0.as_ref();
        let mut rows = || sql::query_rows(report, &resolved, conn);
        self.render_rows_to(report, &mut rows, params, format, out)
    }

    /// Exports a filled document to CSV, laid out like JasperReports' CSV exporter.
    pub fn to_csv(&self, doc: &PrintDocument, options: &csv::CsvOptions) -> String {
        csv::write_csv(doc, options)
    }

    /// Parses a JRXML template, reading the style templates it uses (`<template>`, `.jrtx`
    /// files) from the renderer's resources: its resource folders and the files added with
    /// [`Renderer::add_resource`].
    pub fn parse(&self, template: &str) -> Result<Report> {
        jrxml::parse_report_with(template, &|location| self.images.read_resource(location))
    }

    /// Exports a filled document in the given format.
    pub fn export(&self, doc: &PrintDocument, format: Format) -> Result<Vec<u8>> {
        Ok(match format {
            Format::Pdf => self.to_pdf(doc)?,
            Format::Xlsx => self.to_xlsx(doc, &self.xlsx)?,
            Format::Csv => self.to_csv(doc, &self.csv).into_bytes(),
            Format::Html => html::write_html(doc, &self.fonts, &self.html)?.into_bytes(),
            Format::Docx => docx::write_docx(doc, &self.fonts)?,
            Format::Ods => self.to_ods(doc, &self.xlsx)?,
            Format::Odt => odt::write_odt(doc, &self.fonts)?,
            Format::Pptx => pptx::write_pptx(doc, &self.fonts)?,
        })
    }

    /// Renders a template with rows from a JSON array and parameters from a JSON object.
    pub fn render_json(&mut self, template: &str, rows: &serde_json::Value, params: &serde_json::Value) -> Result<Vec<u8>> {
        self.render_json_as(template, rows, params, Format::Pdf)
    }

    /// Like [`Renderer::render_json`], to another format.
    pub fn render_json_as(
        &mut self,
        template: &str,
        rows: &serde_json::Value,
        params: &serde_json::Value,
        format: Format,
    ) -> Result<Vec<u8>> {
        let report = self.parse(template)?;
        self.render_report_json(&report, rows, params, format)
    }

    /// Like [`Renderer::render_json_as`], with a template already parsed: parse a template
    /// once with [`parse`] and render it as many times as needed.
    pub fn render_report_json(
        &mut self,
        report: &Report,
        rows: &serde_json::Value,
        params: &serde_json::Value,
        format: Format,
    ) -> Result<Vec<u8>> {
        let records = data::records_from_json(report, rows)?;
        let params = params_from_json(report, params)?;
        let mut out = Vec::new();
        if matches!(format, Format::Xlsx | Format::Ods | Format::Odt) {
            // A workbook or an OpenDocument file may read the rows more than once.
            let mut rows = || -> Result<RowIter> { Ok(Box::new(records.iter().cloned().map(Ok))) };
            self.render_rows_to(report, &mut rows, params, format, &mut out)?;
        } else {
            self.render_to(report, Box::new(records.into_iter().map(Ok)), params, format, &mut out)?;
        }
        Ok(out)
    }

    /// Font families that were substituted during the last fills.
    pub fn font_substitutions(&self) -> &[(String, String)] {
        &self.fonts.substitutions
    }
}

/// The second pass of a workbook: an Excel or an OpenDocument sheet.
enum Sheet<W: std::io::Write> {
    Xlsx(Box<xlsx::XlsxStream<W>>),
    Ods(Box<ods::OdsStream<W>>),
}

impl<W: std::io::Write> Sheet<W> {
    fn add(&mut self, items: &[print::PrintItem], fonts: &FontSet) -> Result<()> {
        match self {
            Sheet::Xlsx(s) => s.add(items, fonts),
            Sheet::Ods(s) => s.add(items, fonts),
        }
    }

    fn finish(self) -> Result<W> {
        match self {
            Sheet::Xlsx(s) => s.finish(),
            Sheet::Ods(s) => s.finish(),
        }
    }
}

impl<W: std::io::Write> PageSink for Sheet<W> {
    fn page(&mut self, page: print::PrintPage, fonts: &FontSet) -> Result<()> {
        self.add(&page.items, fonts)
    }
}

/// Converts a JSON object into report parameters typed by their declarations.
pub fn params_from_json(report: &Report, json: &serde_json::Value) -> Result<HashMap<String, Value>> {
    let mut out = HashMap::new();
    let Some(obj) = json.as_object() else { return Ok(out) };
    for (k, v) in obj {
        let class = report.parameters.iter().find(|p| &p.name == k).map(|p| p.class).unwrap_or(value::JavaType::Object);
        out.insert(k.clone(), data::json_to_value(v, class)?);
    }
    Ok(out)
}
