//! Streaming renders must give the same documents as rendering in memory, hand pages over
//! while rows are still arriving, and stop at the limits set for them.

use ferroreport::{Format, JsonRows, Limits, Renderer, RowIter};
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

fn now() -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap().and_hms_opt(10, 30, 0).unwrap()
}

struct Loaded {
    dir: PathBuf,
    report: ferroreport::Report,
    data: String,
    params: serde_json::Value,
}

fn load(name: &str) -> Loaded {
    let dir = fixture(name);
    let report = ferroreport::parse(&std::fs::read_to_string(dir.join("report.jrxml")).unwrap()).unwrap();
    let data = std::fs::read_to_string(dir.join("data.json")).unwrap_or_else(|_| "[]".into());
    let params = std::fs::read_to_string(dir.join("params.json")).map(|p| serde_json::from_str(&p).unwrap()).unwrap_or_default();
    Loaded { dir, report, data, params }
}

impl Loaded {
    fn renderer(&self) -> Renderer {
        Renderer::new().resource_dir(&self.dir).now(now())
    }

    fn in_memory(&self, format: Format) -> Vec<u8> {
        let rows = serde_json::from_str(&self.data).unwrap();
        let records = ferroreport::data::records_from_json(&self.report, &rows).unwrap();
        let params = ferroreport::params_from_json(&self.report, &self.params).unwrap();
        let mut r = self.renderer();
        let doc = r.fill(&self.report, &records, params).unwrap();
        r.export(&doc, format).unwrap()
    }

    fn streamed(&self, format: Format) -> (Vec<u8>, ferroreport::FillStats) {
        let rows: RowIter = Box::new(JsonRows::new(&self.report, self.data.as_bytes()));
        let params = ferroreport::params_from_json(&self.report, &self.params).unwrap();
        let mut out = Vec::new();
        let stats = self.renderer().render_to(&self.report, rows, params, format, &mut out).unwrap();
        (out, stats)
    }
}

fn pages_of(pdf: &[u8]) -> usize {
    let text = String::from_utf8_lossy(pdf);
    text.matches("/Type /Page\n").count() + text.matches("/Type /Page ").count() + text.matches("/Type /Page/").count()
}

#[test]
fn every_fixture_streams_to_the_same_document() {
    let mut checked = 0;
    for entry in std::fs::read_dir(fixture("")).unwrap() {
        let dir = entry.unwrap().path();
        // A fixture filled from a database is checked by `fixtures.rs`.
        if !dir.join("expected.json").exists() || dir.join("data.sql").exists() {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        let f = load(&name);
        let want: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();
        let (pdf, stats) = f.streamed(Format::Pdf);
        assert_eq!(stats.pages as u64, want["pages"].as_u64().unwrap(), "{name}: pages");
        assert_eq!(pages_of(&pdf), stats.pages.max(1), "{name}: page objects");
        assert!(pdf.ends_with(b"%%EOF"), "{name}");
        assert_eq!(f.streamed(Format::Csv).0, f.in_memory(Format::Csv), "{name}: CSV");
        assert_eq!(f.streamed(Format::Xlsx).0, f.in_memory(Format::Xlsx), "{name}: XLSX");
        assert!(f.streamed(Format::Ods).0 == f.in_memory(Format::Ods), "{name}: ODS");
        assert!(f.streamed(Format::Odt).0 == f.in_memory(Format::Odt), "{name}: ODT");
        let pptx = f.streamed(Format::Pptx).0;
        assert!(pptx == f.in_memory(Format::Pptx), "{name}: PPTX");
        let slides = String::from_utf8_lossy(&pptx).matches("ppt/slides/slide").count();
        // Each slide is named in the archive twice: its entry and the central directory.
        assert_eq!(slides, stats.pages * 2, "{name}: PPTX slides");
        let html = String::from_utf8(f.streamed(Format::Html).0).unwrap();
        assert_eq!(html.matches("<svg class=\"page\"").count(), stats.pages, "{name}: HTML pages");
        assert_eq!(
            html.matches("</svg>\n").count() >= stats.pages,
            html.ends_with("</body>\n</html>\n"),
            "{name}: HTML is closed"
        );
        checked += 1;
    }
    assert!(checked >= 30, "only {checked} fixtures checked");
}

#[test]
fn a_page_whose_late_value_is_drawn_over_waits_for_it() {
    // The total in the title has a stamp drawn over it and the page count sits in a frame
    // with a line across: drawing them last would change the page, so nothing is written
    // early and the PDF is the one the in-memory path gives.
    let f = load("34-late-values-overlap");
    assert_eq!(f.streamed(Format::Pdf).0, f.in_memory(Format::Pdf));
}

#[test]
fn page_x_of_y_does_not_hold_pages_back() {
    let f = load("33-page-x-of-y");
    let (pdf, stats) = f.streamed(Format::Pdf);
    assert_eq!(stats.pages, 5);
    // Every page but the last was written before the page count was known, and got it (and
    // the first page its grand total) as a content stream of its own.
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(text.matches("/Contents [").count(), 4, "pages with values added later");

    // Pages leave while rows are still being read.
    struct Watch {
        read: Rc<Cell<usize>>,
        first_page_after: Option<usize>,
        pages: usize,
        late: usize,
    }
    impl ferroreport::PageSink for Watch {
        fn accepts_late(&self) -> bool {
            true
        }
        fn page(
            &mut self,
            _page: ferroreport::print::PrintPage,
            _fonts: &ferroreport::fonts::FontSet,
        ) -> ferroreport::Result<()> {
            self.first_page_after.get_or_insert(self.read.get());
            self.pages += 1;
            Ok(())
        }
        fn late(
            &mut self,
            _page: usize,
            _item: ferroreport::print::PrintItem,
            _fonts: &ferroreport::fonts::FontSet,
        ) -> ferroreport::Result<()> {
            self.late += 1;
            Ok(())
        }
    }
    let read = Rc::new(Cell::new(0));
    let counter = read.clone();
    let rows: RowIter = Box::new(JsonRows::new(&f.report, f.data.as_bytes()).inspect(move |_| counter.set(counter.get() + 1)));
    let mut watch = Watch { read, first_page_after: None, pages: 0, late: 0 };
    let mut r = f.renderer();
    let stats = r.fill_to(&f.report, rows, Default::default(), &mut watch).unwrap();
    assert_eq!((stats.pages, stats.rows, watch.pages), (5, 190, 5));
    assert!(watch.late >= 5, "four page counts, the grand total and group totals that end on a later page: {}", watch.late);
    assert!(watch.first_page_after.unwrap() < 100, "first page left after {:?} of 190 rows", watch.first_page_after);
}

#[test]
fn master_page_numbers_are_the_pages_of_the_report() {
    // evaluationTime="Master" (JasperReports' tableofcontents sample, TablePart): resolved when
    // the report ends, with the number of the page the element is on and the page count.
    // Anywhere else the two variables are null.
    let template = r#"<jasperReport name="master" pageWidth="300" pageHeight="120" columnWidth="260" leftMargin="20" rightMargin="20" topMargin="10" bottomMargin="10">
        <field name="n" class="java.lang.Integer"/>
        <detail><band height="20"><element kind="textField" x="0" y="0" width="200" height="20"><expression><![CDATA["row " + $F{n}]]></expression></element></band></detail>
        <pageFooter height="30">
            <element kind="textField" x="0" y="0" width="200" height="15" evaluationTime="Master"><expression><![CDATA["Page " + $V{MASTER_CURRENT_PAGE} + " of " + $V{MASTER_TOTAL_PAGES}]]></expression></element>
            <element kind="textField" x="0" y="15" width="200" height="15"><expression><![CDATA["now " + $V{MASTER_CURRENT_PAGE}]]></expression></element>
        </pageFooter>
    </jasperReport>"#;
    let report = ferroreport::parse(template).unwrap();
    let data = serde_json::to_string(&(1..=7).map(|n| serde_json::json!({ "n": n })).collect::<Vec<_>>()).unwrap();
    let f = Loaded { dir: std::env::temp_dir(), report, data, params: serde_json::Value::Null };
    let (csv, stats) = f.streamed(Format::Csv);
    assert_eq!(stats.pages, 3);
    assert_eq!(csv, f.in_memory(Format::Csv));
    let text = String::from_utf8(csv).unwrap();
    for want in ["Page 1 of 3", "Page 2 of 3", "Page 3 of 3", "now null"] {
        assert!(text.contains(want), "{want} in {text}");
    }
    assert!(!text.contains("Page null"), "{text}");
}

#[test]
fn limits_stop_a_render() {
    let f = load("33-page-x-of-y");
    let render = |limits: Limits| {
        let rows: RowIter = Box::new(JsonRows::new(&f.report, f.data.as_bytes()));
        f.renderer().limits(limits).render_to(&f.report, rows, Default::default(), Format::Pdf, Vec::new())
    };
    assert!(render(Limits { max_pages: Some(5), ..Default::default() }).is_ok());
    let err = render(Limits { max_pages: Some(4), ..Default::default() }).unwrap_err();
    assert!(matches!(err, ferroreport::Error::Limit(_)), "{err}");
    let err = render(Limits { timeout: Some(std::time::Duration::ZERO), ..Default::default() }).unwrap_err();
    assert!(matches!(err, ferroreport::Error::Limit(_)), "{err}");
}

#[test]
fn a_row_limit_stops_a_report_that_reads_more_than_it_prints() {
    // Only a summary: a million rows would make one page, which no page limit stops.
    let report = ferroreport::parse(
        r#"<jasperReport name="totals" pageWidth="300" pageHeight="100" columnWidth="300" leftMargin="0" rightMargin="0" topMargin="0" bottomMargin="0">
        <field name="n" class="java.lang.Integer"/>
        <variable name="total" class="java.lang.Integer" calculation="Sum"><variableExpression><![CDATA[$F{n}]]></variableExpression></variable>
        <summary><band height="20"><textField><reportElement x="0" y="0" width="300" height="20"/><textFieldExpression><![CDATA["rows " + $V{REPORT_COUNT} + " total " + $V{total}]]></textFieldExpression></textField></band></summary>
    </jasperReport>"#,
    )
    .unwrap();
    let row = ferroreport::data::records_from_json(&report, &serde_json::json!([{ "n": 1 }])).unwrap().remove(0);
    // Rows that never end, counting how many the render asked for.
    let read = Rc::new(Cell::new(0usize));
    let render = |max: usize, rows: Option<usize>| {
        let (row, counter) = (row.clone(), read.clone());
        counter.set(0);
        let endless = std::iter::repeat_with(move || {
            counter.set(counter.get() + 1);
            Ok(row.clone())
        });
        let rows: RowIter = match rows {
            Some(n) => Box::new(endless.take(n)),
            None => Box::new(endless),
        };
        let mut out = Vec::new();
        let limits = Limits { max_pages: Some(1), timeout: None };
        Renderer::new().limits(limits).max_rows(Some(max)).render_to(&report, rows, Default::default(), Format::Csv, &mut out)?;
        Ok::<_, ferroreport::Error>(String::from_utf8(out).unwrap())
    };
    let err = render(1000, None).unwrap_err();
    assert!(matches!(err, ferroreport::Error::Limit(_)), "{err}");
    assert_eq!(err.to_string(), "limit exceeded: the report reads more than 1000 rows");
    assert_eq!(read.get(), 1001, "stops at the first row past the limit");
    // Up to the limit, the report is whole.
    assert_eq!(render(1000, Some(1000)).unwrap().trim(), "rows 1000 total 1000");
    assert!(render(1000, Some(1001)).is_err());
    // Without a limit, every row is read.
    let rows: RowIter = Box::new(std::iter::repeat_n(row.clone(), 5000).map(Ok));
    let stats = Renderer::new().render_to(&report, rows, Default::default(), Format::Csv, Vec::new()).unwrap();
    assert_eq!(stats.rows, 5000);
}

#[test]
fn the_row_limit_counts_the_rows_of_subreports_and_datasets() {
    // Every row read in the render counts: the report's and those of a subreport, a table, a
    // list and a crosstab, each over a list field. A band laid out again (to split it over
    // pages, or a horizontal list measured first) counts its rows once.
    let fixtures = [
        ("17-subreports", "lines"),
        ("27-table", "lines"),
        ("29-list", "contacts"),
        ("41-horizontal-list", "contacts"),
        ("31-crosstab-pages", "sales"),
    ];
    for (name, nested) in fixtures {
        let f = load(name);
        let rows: Vec<serde_json::Value> = serde_json::from_str(&f.data).unwrap();
        let total = rows.len() + rows.iter().map(|r| r[nested].as_array().map_or(0, Vec::len)).sum::<usize>();
        let render = |max: usize| {
            let rows: RowIter = Box::new(JsonRows::new(&f.report, f.data.as_bytes()));
            let params = ferroreport::params_from_json(&f.report, &f.params).unwrap();
            f.renderer().max_rows(Some(max)).render_to(&f.report, rows, params, Format::Pdf, Vec::new())
        };
        render(total).unwrap_or_else(|e| panic!("{name}: {total} rows: {e}"));
        let err = render(total - 1).unwrap_err();
        assert!(err.to_string().contains(&format!("more than {} rows", total - 1)), "{name}: {err}");
    }
}

#[test]
fn bad_rows_stop_a_render_with_their_position() {
    let f = load("33-page-x-of-y");
    let data = r#"[{"id": 1, "name": "a", "salary": 1}, {"id": "x", "name": "b", "salary": 2}]"#;
    let rows: RowIter = Box::new(JsonRows::new(&f.report, data.as_bytes()));
    let err = f.renderer().render_to(&f.report, rows, Default::default(), Format::Pdf, Vec::new()).unwrap_err();
    assert!(err.to_string().contains("row 1"), "{err}");
}

#[test]
fn html_pages_carry_the_report_as_text_and_escape_it() {
    let f = load("02-invoice");
    let html = String::from_utf8(f.streamed(Format::Html).0).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>") && html.contains("<title>invoice</title>"));
    // Text is text (selectable, searchable), with its font and measured width.
    assert!(html.contains(">Ferro Industrial SRL</text>"), "{}", &html[..600]);
    assert!(html.contains("font-family=\"'DejaVu Sans',sans-serif\"") && html.contains("textLength="));
    // Data cannot inject markup.
    let data = r#"[{"sku": "<script>alert(1)</script>", "description": "a & b", "qty": 1, "price": "1.00", "discount": "0"}]"#;
    let rows: RowIter = Box::new(JsonRows::new(&f.report, data.as_bytes()));
    let params = ferroreport::params_from_json(&f.report, &f.params).unwrap();
    let mut out = Vec::new();
    f.renderer().render_to(&f.report, rows, params, Format::Html, &mut out).unwrap();
    let html = String::from_utf8(out).unwrap();
    // (the narrow SKU column breaks the text over lines, so only its start is on one of them)
    assert!(!html.contains("<script") && html.contains("&lt;scr") && html.contains("a &amp; b"), "{html}");
    // A fragment is just the pages.
    let rows: RowIter = Box::new(JsonRows::new(&f.report, f.data.as_bytes()));
    let params = ferroreport::params_from_json(&f.report, &f.params).unwrap();
    let mut out = Vec::new();
    let options = ferroreport::html::HtmlOptions { fragment: true, ..Default::default() };
    f.renderer().html_options(options).render_to(&f.report, rows, params, Format::Html, &mut out).unwrap();
    assert!(out.starts_with(b"<svg class=\"page\""));
}

/// The files of a ZIP archive, in order, with their content.
fn unzip(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    let u16_at = |i: usize| u16::from_le_bytes([zip[i], zip[i + 1]]) as usize;
    let u32_at = |i: usize| u32::from_le_bytes([zip[i], zip[i + 1], zip[i + 2], zip[i + 3]]) as usize;
    let end = zip.len() - 22;
    assert_eq!(u32_at(end), 0x0605_4b50);
    let (count, mut at) = (u16_at(end + 10), u32_at(end + 16));
    let mut files = Vec::new();
    for _ in 0..count {
        assert_eq!(u32_at(at), 0x0201_4b50);
        let (method, packed, size, name_len, local) =
            (u16_at(at + 10), u32_at(at + 20), u32_at(at + 24), u16_at(at + 28), u32_at(at + 42));
        let name = String::from_utf8(zip[at + 46..at + 46 + name_len].to_vec()).unwrap();
        let data = local + 30 + u16_at(local + 26) + u16_at(local + 28);
        let content = match method {
            0 => zip[data..data + packed].to_vec(),
            _ => miniz_oxide::inflate::decompress_to_vec(&zip[data..data + packed]).unwrap(),
        };
        assert_eq!(content.len(), size, "{name}");
        files.push((name, content));
        at += 46 + name_len;
    }
    files
}

/// A workbook too large to keep is made in two passes over its rows, and comes out the same
/// as when its pages are all in memory.
#[test]
fn a_large_workbook_is_written_in_two_passes_and_is_the_same() {
    let large = load("09-large");
    let few: Vec<serde_json::Value> = serde_json::from_str(&large.data).unwrap();
    let rows: Vec<serde_json::Value> = (0..40_000).map(|i| few[i % few.len()].clone()).collect();
    let lines: String = rows.iter().map(|r| format!("{r}\n")).collect();
    let records = ferroreport::data::records_from_json(&large.report, &serde_json::Value::Array(rows)).unwrap();

    let mut r = large.renderer();
    let doc = r.fill(&large.report, &records, Default::default()).unwrap();
    let kept = r.to_xlsx(&doc, &Default::default()).unwrap();
    drop(doc);

    let asked = Cell::new(0);
    let mut again = || -> ferroreport::Result<RowIter> {
        asked.set(asked.get() + 1);
        Ok(Box::new(JsonRows::new(&large.report, lines.as_bytes())))
    };
    let mut streamed = Vec::new();
    let stats =
        large.renderer().render_rows_to(&large.report, &mut again, Default::default(), Format::Xlsx, &mut streamed).unwrap();
    assert_eq!(asked.get(), 2, "a report this size reads its rows twice");
    assert_eq!(stats.rows, 40_000);
    assert!(streamed == kept, "the workbook written in two passes differs from the one written from memory");

    // An OpenDocument spreadsheet is made the same way, with a pass between the two that
    // finds the styles that come before its rows.
    let doc = r.fill(&large.report, &records, Default::default()).unwrap();
    let kept = r.to_ods(&doc, &Default::default()).unwrap();
    drop(doc);
    asked.set(0);
    let mut streamed = Vec::new();
    large.renderer().render_rows_to(&large.report, &mut again, Default::default(), Format::Ods, &mut streamed).unwrap();
    assert_eq!(asked.get(), 3, "a spreadsheet this size reads its rows three times");
    // `content.xml` is written in pieces, so its sizes follow it in the archive instead of
    // preceding it: the files in it are the same.
    assert!(unzip(&streamed) == unzip(&kept), "the spreadsheet written as it is made differs from the one written from memory");

    // An OpenDocument text: its styles first, then its pages as they are made.
    let doc = r.fill(&large.report, &records, Default::default()).unwrap();
    let kept = r.export(&doc, Format::Odt).unwrap();
    drop(doc);
    asked.set(0);
    let mut streamed = Vec::new();
    large.renderer().render_rows_to(&large.report, &mut again, Default::default(), Format::Odt, &mut streamed).unwrap();
    assert_eq!(asked.get(), 2, "a text document this size reads its rows twice");
    assert!(unzip(&streamed) == unzip(&kept), "the text document written as it is made differs from the one written from memory");

    // A small one is filled once, and other formats always are.
    for (format, rows) in [
        (Format::Xlsx, &lines[..2_000]),
        (Format::Ods, &lines[..2_000]),
        (Format::Odt, &lines[..2_000]),
        (Format::Csv, &lines[..]),
    ] {
        let rows = &rows[..rows.rfind('\n').unwrap() + 1];
        asked.set(0);
        let mut again = || -> ferroreport::Result<RowIter> {
            asked.set(asked.get() + 1);
            Ok(Box::new(JsonRows::new(&large.report, rows.as_bytes())))
        };
        large.renderer().render_rows_to(&large.report, &mut again, Default::default(), format, std::io::sink()).unwrap();
        assert_eq!(asked.get(), 1, "{format:?}");
    }
}

/// A tagged PDF carries a structure tree over the same pages: a paragraph per text and a
/// figure per picture, with the document's language and title.
#[test]
fn a_tagged_pdf_describes_its_content() {
    let count = |pdf: &[u8], what: &str| pdf.windows(what.len()).filter(|w| *w == what.as_bytes()).count();
    for (name, figures) in [("33-page-x-of-y", 0), ("15-images", 10)] {
        let fx = load(name);
        let plain = fx.streamed(Format::Pdf).0;
        let mut out = Vec::new();
        let rows: RowIter = Box::new(JsonRows::new(&fx.report, fx.data.as_bytes()));
        let params = ferroreport::params_from_json(&fx.report, &fx.params).unwrap();
        fx.renderer().tagged(true).render_to(&fx.report, rows, params, Format::Pdf, &mut out).unwrap();
        assert_eq!(pages_of(&out), pages_of(&plain), "{name}");
        for key in
            ["/Type /StructTreeRoot", "/MarkInfo", "/Lang (en-US)", "/DisplayDocTitle true", "/ParentTreeNextKey", "/S /Document"]
        {
            assert_eq!(count(&out, key), 1, "{name}: {key}");
        }
        assert!(count(&out, "/S/P/P ") > 0, "{name}: paragraphs");
        assert_eq!(count(&out, "/S /Figure"), figures, "{name}: figures");
        assert_eq!(count(&out, "/Alt ("), figures, "{name}: every figure says what it shows");
        assert_eq!(count(&out, "/StructParents"), count(&out, "/S /Sect"), "{name}: every page with content has its section");
        assert_eq!(count(&plain, "/StructTreeRoot"), 0);
    }
}

/// In a tagged PDF a table is a table: rows of header cells that say what they name and of
/// data cells, each holding its texts. A text can be marked as a heading.
#[test]
fn a_tagged_pdf_has_tables_and_headings() {
    let count = |pdf: &[u8], what: &str| pdf.windows(what.len()).filter(|w| *w == what.as_bytes()).count();
    let tagged = |name: &str| {
        let fx = load(name);
        let rows: RowIter = Box::new(JsonRows::new(&fx.report, fx.data.as_bytes()));
        let params = ferroreport::params_from_json(&fx.report, &fx.params).unwrap();
        let mut out = Vec::new();
        fx.renderer().tagged(true).render_to(&fx.report, rows, params, Format::Pdf, &mut out).unwrap();
        out
    };
    let table = tagged("27-table");
    assert!(count(&table, "/S /Table") >= 1);
    assert!(count(&table, "/S /TR") > count(&table, "/S /Table"), "rows");
    assert!(count(&table, "/S /TH") >= 1 && count(&table, "/S /TD") > count(&table, "/S /TH"), "cells");
    assert_eq!(count(&table, "/Scope /Column"), count(&table, "/S /TH"), "the header cells of a table name columns");
    let crosstab = tagged("30-crosstab");
    assert!(count(&crosstab, "/Scope /Row") >= 1 && count(&crosstab, "/Scope /Column") >= 1, "a crosstab has headers both ways");
    // A header over several columns or rows says how many.
    let groups = tagged("28-table-groups");
    assert!(count(&groups, "/ColSpan 2") >= 1 && count(&groups, "/RowSpan 2") >= 1, "column groups");
    assert!(count(&crosstab, "/ColSpan ") >= 1 && count(&crosstab, "/RowSpan ") >= 1, "crosstab groups");
    assert_eq!(count(&table, "/ColSpan 1") + count(&table, "/RowSpan 1"), 0, "a single cell says nothing");
    // Nothing sits in a table outside a row, or in a row outside a cell.
    for pdf in [&table, &crosstab, &groups] {
        assert_eq!(count(pdf, "/S /Div"), 0);
    }

    let template = r#"<jasperReport name="Headings" whenNoDataType="AllSectionsNoDetail" pageWidth="300" pageHeight="200" columnWidth="260" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
  <style name="Base" isDefault="true" fontName="DejaVu Sans" fontSize="10"/>
  <title><band height="60">
    <staticText><reportElement x="0" y="0" width="260" height="20"><property name="net.sf.jasperreports.export.accessibility.tag" value="h1"/></reportElement><text><![CDATA[Sales]]></text></staticText>
    <textField><reportElement x="0" y="20" width="260" height="20"><property name="net.sf.jasperreports.export.accessibility.tag" value="h2"/></reportElement><textFieldExpression><![CDATA["By " + "region"]]></textFieldExpression></textField>
    <staticText><reportElement x="0" y="40" width="260" height="20"/><text><![CDATA[Plain text]]></text></staticText>
  </band></title>
</jasperReport>"#;
    let report = ferroreport::parse(template).unwrap();
    let mut out = Vec::new();
    Renderer::new()
        .tagged(true)
        .render_to(&report, Box::new(std::iter::empty()), Default::default(), Format::Pdf, &mut out)
        .unwrap();
    assert_eq!((count(&out, "/S/H1/P "), count(&out, "/S/H2/P "), count(&out, "/S/P/P ")), (1, 1, 1));
}

/// The lists of an HTML text are lists in a tagged PDF: items with their bullet or number
/// and their body, a list inside an item's body, and the text around them as paragraphs.
#[test]
fn a_tagged_pdf_has_lists() {
    let count = |pdf: &[u8], what: &str| pdf.windows(what.len()).filter(|w| *w == what.as_bytes()).count();
    let template = r#"<jasperReport name="Lists" whenNoDataType="AllSectionsNoDetail" pageWidth="400" pageHeight="300" columnWidth="360" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
  <style name="Base" isDefault="true" fontName="DejaVu Sans" fontSize="10"/>
  <title><band height="200">
    <textField><reportElement x="0" y="0" width="360" height="200"/><textElement markup="html"/>
      <textFieldExpression><![CDATA["Before<ul><li>One</li><li>Two<ol><li>Two a</li><li>Two b</li></ol></li><li>Three</li></ul>After"]]></textFieldExpression></textField>
  </band></title>
</jasperReport>"#;
    let report = ferroreport::parse(template).unwrap();
    let render = |tagged: bool| {
        let mut out = Vec::new();
        Renderer::new()
            .tagged(tagged)
            .render_to(&report, Box::new(std::iter::empty()), Default::default(), Format::Pdf, &mut out)
            .unwrap();
        out
    };
    let out = render(true);
    assert_eq!(count(&out, "/S /L\n"), 2, "a list and the list inside it");
    assert_eq!((count(&out, "/ListNumbering /Disc"), count(&out, "/ListNumbering /Decimal")), (1, 1));
    assert_eq!((count(&out, "/S /LI"), count(&out, "/S/Lbl/P "), count(&out, "/S /LBody")), (5, 5, 5));
    // Before, after, and the body of each item.
    assert_eq!(count(&out, "/S/P/P "), 2 + 5);
    // The same page as without tags.
    assert_eq!(pages_of(&out), pages_of(&render(false)));
}
