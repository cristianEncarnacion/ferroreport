//! The filler: walks the data source, computes variables and groups, paginates bands and
//! produces a [`PrintDocument`].
//!
//! Evaluation follows JasperReports' three modes:
//! * group footers see the previous record and the variables before the current record;
//! * page footers printed by a page break see variables without the record being moved to
//!   the next page;
//! * detail bands see variables that include the current record.

use crate::data::Record;
use crate::error::{Error, Result};
use crate::expr::{truthy, EvalEnv, Expr, Scope};
use crate::fonts::FontSet;
use crate::format::format_value;
use crate::image::{ImageData, ImageStore};
use crate::model::*;
use crate::print::*;
use crate::text::{break_lines_hyphenated, text_after_lines, text_before_line, vertical_metrics, Measure};
use crate::value::{JavaType, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// ---------------------------------------------------------------------------------------------
// Variables

#[derive(Debug, Clone)]
struct VarDef {
    name: String,
    class: JavaType,
    calculation: Calculation,
    reset: ResetType,
    /// When the variable takes a new value in: with every row (`None`), or once for each
    /// group, page or column (`incrementType`).
    increment: ResetType,
    expression: Option<Expr>,
    initial: Option<Expr>,
}

#[derive(Debug, Clone, Default)]
struct VarState {
    value: Value,
    initialized: bool,
    count: i64,
    sum: Option<Value>,
    distinct: HashSet<String>,
    mean: f64,
    m2: f64,
}

fn add_values(a: &Value, b: &Value, class: JavaType) -> Value {
    match class {
        JavaType::Integer | JavaType::Short | JavaType::Byte => {
            Value::Int((a.as_i64().unwrap_or(0) as i32).wrapping_add(b.as_i64().unwrap_or(0) as i32))
        }
        JavaType::Long | JavaType::BigInteger => Value::Long(a.as_i64().unwrap_or(0).wrapping_add(b.as_i64().unwrap_or(0))),
        JavaType::Float => Value::Float(a.as_f64().unwrap_or(0.0) as f32 + b.as_f64().unwrap_or(0.0) as f32),
        JavaType::BigDecimal => {
            Value::Decimal(a.as_decimal().unwrap_or_default().saturating_add(b.as_decimal().unwrap_or_default()))
        }
        _ => Value::Double(a.as_f64().unwrap_or(0.0) + b.as_f64().unwrap_or(0.0)),
    }
}

fn update_var(st: &mut VarState, def: &VarDef, ev: Value) {
    let first = !st.initialized;
    st.initialized = true;
    let class = def.class;
    match def.calculation {
        Calculation::Nothing => st.value = class.coerce(ev),
        // Set from outside (subreport return values); the variable keeps its value.
        Calculation::System => {}
        Calculation::First => {
            if first {
                st.value = class.coerce(ev);
            }
        }
        Calculation::Count => {
            if first {
                st.count = 0;
            }
            if !ev.is_null() {
                st.count += 1;
            }
            st.value = class.coerce(Value::Long(st.count));
            if matches!(class, JavaType::Object | JavaType::Number) {
                st.value = Value::Int(st.count as i32);
            }
        }
        Calculation::DistinctCount => {
            if !ev.is_null() {
                st.distinct.insert(ev.to_java_string());
            }
            st.value = class.coerce(Value::Long(st.distinct.len() as i64));
        }
        Calculation::Sum => {
            if !ev.is_null() {
                st.sum = Some(match (&st.sum, first) {
                    (Some(s), false) => add_values(s, &ev, class),
                    _ => class.coerce(ev),
                });
            } else if first {
                st.sum = None;
            }
            if let Some(s) = &st.sum {
                st.value = s.clone();
            } else if first {
                st.value = Value::Null;
            }
        }
        Calculation::Average => {
            if first {
                st.count = 0;
                st.sum = None;
            }
            if !ev.is_null() {
                st.count += 1;
                st.sum = Some(match &st.sum {
                    Some(s) => add_values(s, &ev, class),
                    None => class.coerce(ev),
                });
            }
            if let Some(s) = &st.sum {
                st.value = match class {
                    // Jasper divides with MathContext.DECIMAL64.
                    JavaType::BigDecimal => crate::value::java_decimal_divide(
                        s.as_decimal().unwrap_or_default(),
                        rust_decimal::Decimal::from(st.count),
                        16,
                    )
                    .map(Value::Decimal)
                    .unwrap_or(Value::Null),
                    _ => class.coerce(Value::Double(s.as_f64().unwrap_or(0.0) / st.count as f64)),
                };
            }
        }
        Calculation::Highest | Calculation::Lowest => {
            if !ev.is_null() {
                let want =
                    if def.calculation == Calculation::Highest { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Less };
                if st.value.is_null() || first || ev.compare(&st.value) == Some(want) {
                    st.value = class.coerce(ev);
                }
            }
        }
        Calculation::Variance | Calculation::StandardDeviation => {
            if first {
                st.count = 0;
                st.mean = 0.0;
                st.m2 = 0.0;
            }
            if let Some(x) = ev.as_f64() {
                st.count += 1;
                let delta = x - st.mean;
                st.mean += delta / st.count as f64;
                st.m2 += delta * (x - st.mean);
            }
            if st.count > 0 {
                // Jasper uses the population variance (divides by n).
                let var = st.m2 / st.count as f64;
                let v = if def.calculation == Calculation::Variance { var } else { var.sqrt() };
                st.value = class.coerce(Value::Double(v));
            }
        }
    }
}

/// An expression's value, as JasperReports evaluates it: what Java would throw as a
/// `NullPointerException` (a method called on null, null in arithmetic) makes the whole
/// expression null instead of failing the report (`net.sf.jasperreports.evaluator.ignore.npe`,
/// true by default).
fn eval(expr: &Expr, scope: &dyn Scope, env: &EvalEnv) -> Result<Value> {
    match crate::expr::eval(expr, scope, env) {
        Err(e) if crate::expr::eval::is_null_pointer(&e) => Ok(Value::Null),
        other => other,
    }
}

/// The value of a print-when expression, a conditional style's condition or an
/// increment-when expression: null is false, as in JasperReports.
fn condition(v: &Value, env: &EvalEnv) -> Result<bool> {
    if v.is_null() {
        Ok(false)
    } else {
        truthy(v, env)
    }
}

// ---------------------------------------------------------------------------------------------
// Scope

struct Ctx<'a> {
    record: Option<&'a Record>,
    report: &'a Report,
    params: &'a HashMap<String, Value>,
    vars: &'a [VarState],
    var_index: &'a HashMap<String, usize>,
    page_number: i32,
    column_number: i32,
    /// `MASTER_CURRENT_PAGE` and `MASTER_TOTAL_PAGES`: set only while an element with
    /// `evaluationTime="Master"` is resolved, null everywhere else (as in JasperReports).
    master: Option<(i32, i32)>,
}

impl Scope for Ctx<'_> {
    fn field(&self, name: &str) -> Result<Value> {
        if self.report.field(name).is_none() {
            return Err(Error::eval(format!("field `{name}` is not declared")));
        }
        Ok(self.record.and_then(|r| r.get(name).cloned()).unwrap_or(Value::Null))
    }
    fn param(&self, name: &str) -> Result<Value> {
        // The JSON data source: its current row, for `subDataSource("path")`.
        if name == "REPORT_DATA_SOURCE" {
            if let Some(row) = self.record.and_then(|r| r.get(crate::data::ROW_KEY)) {
                return Ok(row.clone());
            }
        }
        self.params.get(name).cloned().ok_or_else(|| Error::eval(format!("parameter `{name}` is not declared")))
    }
    fn variable(&self, name: &str) -> Result<Value> {
        match name {
            "PAGE_NUMBER" => Ok(Value::Int(self.page_number)),
            "COLUMN_NUMBER" => Ok(Value::Int(self.column_number)),
            "MASTER_CURRENT_PAGE" => Ok(self.master.map_or(Value::Null, |m| Value::Int(m.0))),
            "MASTER_TOTAL_PAGES" => Ok(self.master.map_or(Value::Null, |m| Value::Int(m.1))),
            _ => self
                .var_index
                .get(name)
                .map(|&i| self.vars[i].value.clone())
                .ok_or_else(|| Error::eval(format!("variable `{name}` is not declared"))),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rows, limits and where pages go

/// A row of the data source: borrowed, or owned when it arrives from a stream.
pub type Row<'a> = std::borrow::Cow<'a, Record>;

/// Rows arriving one at a time.
pub type RowIter<'a> = Box<dyn Iterator<Item = Result<Record>> + 'a>;

enum RowSource<'a> {
    Slice(std::slice::Iter<'a, Record>),
    Stream(RowIter<'a>),
}

/// The rows of the report being filled. Only the current row and the one before it are
/// kept, unless the report needs them all (a crosstab over the report's own rows).
struct Rows<'a> {
    source: RowSource<'a>,
    seen: Vec<Row<'a>>,
    keep_all: bool,
    count: usize,
}

impl<'a> Rows<'a> {
    fn new(source: RowIter<'a>, report: &Report) -> Rows<'a> {
        Rows { source: RowSource::Stream(source), seen: Vec::new(), keep_all: needs_all_rows(report), count: 0 }
    }

    fn from_slice(records: &'a [Record], report: &Report) -> Rows<'a> {
        Rows { source: RowSource::Slice(records.iter()), seen: Vec::new(), keep_all: needs_all_rows(report), count: 0 }
    }

    /// Moves to the next row; false at the end (the last row stays current). Every row read
    /// counts toward the render's row limit, kept with `images`.
    fn advance(&mut self, images: &mut ImageStore) -> Result<bool> {
        let next = match &mut self.source {
            RowSource::Slice(it) => it.next().map(|r| Ok(Row::Borrowed(r))),
            RowSource::Stream(it) => it.next().map(|r| r.map(Row::Owned)),
        };
        match next {
            Some(row) => {
                let row = row?;
                count_rows(images, 1)?;
                if !self.keep_all && self.seen.len() >= 2 {
                    self.seen.remove(0);
                }
                self.seen.push(row);
                self.count += 1;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn current(&self) -> Option<&Record> {
        self.seen.last().map(|r| r.as_ref())
    }

    fn record(&self, mode: Values) -> Option<&Record> {
        match mode {
            Values::Old if self.seen.len() >= 2 => Some(self.seen[self.seen.len() - 2].as_ref()),
            _ => self.current(),
        }
    }

    fn seen(&self) -> &[Row<'a>] {
        &self.seen
    }
}

/// Whether filling `report` reads rows again after printing them.
fn needs_all_rows(report: &Report) -> bool {
    fn any(elements: &[Element]) -> bool {
        elements.iter().any(|e| match &e.kind {
            ElementKind::Crosstab(ct) => ct.run.is_none(),
            ElementKind::Frame { children, .. } => any(children),
            _ => false,
        })
    }
    let r = report;
    [
        &r.background,
        &r.title,
        &r.page_header,
        &r.column_header,
        &r.column_footer,
        &r.page_footer,
        &r.last_page_footer,
        &r.summary,
        &r.no_data,
    ]
    .into_iter()
    .flatten()
    .chain(r.detail.iter())
    .chain(r.groups.iter().flat_map(|g| g.header.iter().chain(g.footer.iter())))
    .any(|b| any(&b.elements))
}

/// What one render may use. Rendering stops with [`Error::Limit`] when it goes over.
#[derive(Debug, Clone, Copy, Default)]
pub struct Limits {
    /// Most pages a report may have.
    pub max_pages: Option<usize>,
    /// Longest a render may take (checked while filling).
    pub timeout: Option<std::time::Duration>,
}

impl Limits {
    fn check_pages(&self, pages: usize) -> Result<()> {
        match self.max_pages {
            Some(max) if pages > max => Err(Error::Limit(format!("the report has more than {max} pages"))),
            _ => Ok(()),
        }
    }

    /// When a render started now must be done, with the time it was given.
    fn deadline(&self) -> Option<(std::time::Instant, std::time::Duration)> {
        self.timeout.map(|t| (std::time::Instant::now() + t, t))
    }
}

/// Stops a render that has run past its deadline. The deadline travels with the image
/// store, which every nested fill (subreports, tables, lists) shares.
fn check_time(images: &ImageStore) -> Result<()> {
    match images.deadline {
        Some((deadline, given)) if std::time::Instant::now() > deadline => {
            Err(Error::Limit(format!("rendering took longer than {} s", given.as_secs_f32())))
        }
        _ => Ok(()),
    }
}

/// Counts `n` rows read toward the render's row limit ([`crate::Renderer::max_rows`]): the
/// report's own rows and those of its subreports, tables, lists, charts and crosstabs. The
/// count travels with the image store, as the deadline does.
fn count_rows(images: &mut ImageStore, n: usize) -> Result<()> {
    images.rows_read = images.rows_read.saturating_add(n);
    match images.max_rows {
        Some(max) if images.rows_read > max => Err(Error::Limit(format!("the report reads more than {max} rows"))),
        _ => Ok(()),
    }
}

/// How many rows a query needs to give to reach the row limit and go one past it: reading
/// more would only be thrown away.
fn rows_left(images: &ImageStore) -> Option<usize> {
    images.max_rows.map(|max| max.saturating_sub(images.rows_read).saturating_add(1))
}

/// Receives the pages of a report as they are finished, so a long report never has to be
/// held in memory whole.
pub trait PageSink {
    /// Whether [`PageSink::late`] can add to a page after it was delivered.
    fn accepts_late(&self) -> bool {
        false
    }

    /// Whether texts should come with what spreadsheet and text exporters need (their
    /// [`CellInfo`]). A sink that only draws can say no and save that work.
    fn wants_cells(&self) -> bool {
        true
    }

    /// The next page of the document.
    fn page(&mut self, page: PrintPage, fonts: &FontSet) -> Result<()>;

    /// A value computed after its page was delivered ("page 3 of 240"), drawn on top of
    /// page number `page` (from 0). Only called when [`PageSink::accepts_late`] is true.
    fn late(&mut self, _page: usize, _item: PrintItem, _fonts: &FontSet) -> Result<()> {
        Ok(())
    }
}

/// How much was filled.
#[derive(Debug, Clone, Copy, Default)]
pub struct FillStats {
    pub pages: usize,
    pub rows: usize,
}

/// Whether `item` may draw inside the box `b` (`x0, y0, x1, y1`). True when unsure.
fn may_touch(item: &PrintItem, b: [f32; 4]) -> bool {
    let hit = |x: f32, y: f32, w: f32, h: f32| x < b[2] && x + w > b[0] && y < b[3] && y + h > b[1];
    match item {
        PrintItem::Element(_) | PrintItem::Link(_) => false,
        PrintItem::Cell(info, _) => hit(info.x, info.y, info.w, info.h),
        PrintItem::Group(items) | PrintItem::Paragraphs(_, items) => items.iter().any(|i| may_touch(i, b)),
        PrintItem::Rect { x, y, w, h, fill, stroke, .. } => match (fill, stroke) {
            (Some(_), _) => hit(*x, *y, *w, *h),
            (None, Some(pen)) => {
                // Only the outline is drawn.
                let t = pen.width / 2.0 + 0.5;
                hit(x - t, y - t, w + 2.0 * t, 2.0 * t)
                    || hit(x - t, y + h - t, w + 2.0 * t, 2.0 * t)
                    || hit(x - t, y - t, 2.0 * t, h + 2.0 * t)
                    || hit(x + w - t, y - t, 2.0 * t, h + 2.0 * t)
            }
            (None, None) => false,
        },
        PrintItem::Ellipse { x, y, w, h, .. } | PrintItem::Image { x, y, w, h, .. } => hit(*x, *y, *w, *h),
        PrintItem::Line { x1, y1, x2, y2, pen } => {
            let t = pen.width / 2.0 + 0.5;
            hit(x1.min(*x2) - t, y1.min(*y2) - t, (x1 - x2).abs() + 2.0 * t, (y1 - y2).abs() + 2.0 * t)
        }
        PrintItem::Text(t) => t.lines.iter().any(|l| hit(l.x, l.baseline - t.size, l.width, t.size * 1.5)),
        PrintItem::Path { .. } | PrintItem::Transform(..) | PrintItem::Shapes { .. } => true,
    }
}

/// Whether nothing drawn after a waiting element touches its box, so that drawing it last
/// gives the same page.
fn stands_alone(items: &[PrintItem], p: &Pending) -> bool {
    let d = &p.text;
    let b = [d.x, d.y, d.x + d.w, d.y + d.h];
    !items[p.item + 1..].iter().any(|i| may_touch(i, b))
}

// ---------------------------------------------------------------------------
// Band layout

/// What a deferred element draws.
#[derive(Debug, Clone)]
enum Deferred {
    Text(Box<TextElement>),
    Chart(Box<crate::chart::ChartElement>),
}

/// A text field or chart whose value is computed later (evaluationTime other than Now).
#[derive(Debug, Clone)]
struct DeferredText {
    what: Deferred,
    forecolor: Color,
    /// Background of an opaque text element (for exporters).
    back: Option<Color>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    time: EvaluationTime,
    group: Option<String>,
    /// For `evaluationTime="Auto"`: what the element still waits for.
    auto: Option<Box<Auto>>,
    /// What the text is in a tagged PDF (a heading).
    tag: Option<Tag>,
}

/// A text field with `evaluationTime="Auto"`: each variable of its expression is read at the
/// moment its own reset type names (a group's total when the group ends, the report's when
/// the report ends), and everything else as it was when the element was laid out. That is
/// what makes "this group's share of the grand total" one expression.
#[derive(Debug, Clone)]
struct Auto {
    record: Option<Record>,
    vars: Vec<VarState>,
    /// The moments still to come, each with the variables it brings.
    waits: Vec<(EvaluationTime, Option<String>, Vec<usize>)>,
}

#[derive(Debug)]
struct Laid {
    item: PrintItem,
    deferred: Option<DeferredText>,
}

#[derive(Debug, Default)]
struct BandLayout {
    height: f32,
    items: Vec<Laid>,
    has_break: bool,
    has_column_break: bool,
    /// When the layout was cut at the available height: what still has to be printed, in the
    /// container's coordinates (not yet moved up), with expressions already evaluated.
    rest: Option<Vec<Element>>,
    /// The rest has elements printed again (`isPrintWhenDetailOverflows`): it keeps the
    /// container's top instead of moving up to its first element (JasperReports' `firstY` is 0
    /// then, JRFillElementContainer.prepareElements).
    rest_reprints: bool,
    /// Values printed by elements with isPrintRepeatedValues="false" (committed on emit).
    printed_values: Vec<(usize, String)>,
    /// Subreport return values: (variable, calculation, value), applied on emit.
    returns: Vec<Returned>,
}

/// A copy of `el` that will not be evaluated again: print conditions are dropped and a text
/// field becomes the given (already formatted) text.
fn freeze(el: &Element, text: Option<String>) -> Element {
    let mut e = el.clone();
    e.print_when = None;
    e.print_repeated_values = true;
    e.conditional = None;
    if let (ElementKind::Text(te), Some(t)) = (&mut e.kind, text) {
        te.content = TextContent::Static(t);
    }
    e
}

/// [`freeze`], keeping the paragraph settings of a text (see [`crate::paragraph`]).
fn frozen(fonts: &mut FontSet, el: &Element, text: Option<String>) -> Element {
    let e = freeze(el, text);
    carry_paragraph(fonts, el, &e);
    e
}

/// `to`, the rest of the text `from` on the next page, keeps its paragraph settings.
fn carry_paragraph(fonts: &mut FontSet, from: &Element, to: &Element) {
    if let (ElementKind::Text(a), ElementKind::Text(b)) = (&from.kind, &to.kind) {
        fonts.carry_paragraph(a, b);
    }
}

/// Moves elements up so the first one starts at the top; returns the offset removed.
fn move_to_top(elements: &mut [Element]) -> f32 {
    let first = elements.iter().map(|e| e.y).fold(f32::INFINITY, f32::min);
    if !first.is_finite() {
        return 0.0;
    }
    for e in elements.iter_mut() {
        e.y -= first;
    }
    first
}

fn translate(item: &mut PrintItem, dx: f32, dy: f32) {
    match item {
        PrintItem::Cell(info, inner) => {
            let info = Arc::make_mut(info);
            info.x += dx;
            info.y += dy;
            translate(inner, dx, dy);
        }
        PrintItem::Group(items) | PrintItem::Paragraphs(_, items) => {
            for i in items {
                translate(i, dx, dy);
            }
        }
        PrintItem::Element(info) => {
            let info = Arc::make_mut(info);
            info.x += dx;
            info.y += dy;
        }
        PrintItem::Link(info) => {
            let info = Arc::make_mut(info);
            info.x += dx;
            info.y += dy;
        }
        PrintItem::Path { ops, .. } => {
            let (dx, dy) = (dx as f64, dy as f64);
            *ops = Arc::new(
                ops.iter()
                    .map(|op| match *op {
                        PathOp::Move(a, b) => PathOp::Move(a + dx, b + dy),
                        PathOp::Line(a, b) => PathOp::Line(a + dx, b + dy),
                        PathOp::Quad(a, b, c, d) => PathOp::Quad(a + dx, b + dy, c + dx, d + dy),
                        PathOp::Cubic(a, b, c, d, e, f) => PathOp::Cubic(a + dx, b + dy, c + dx, d + dy, e + dx, f + dy),
                        PathOp::Close => PathOp::Close,
                    })
                    .collect(),
            );
        }
        PrintItem::Transform(m, items) => {
            // Move the local drawing and its anchor together: M' = T(d) · M · T(-d).
            let [a, b, c, d, _, _] = *m;
            m[4] += dx - (a * dx + c * dy);
            m[5] += dy - (b * dx + d * dy);
            for i in items {
                translate(i, dx, dy);
            }
        }
        PrintItem::Rect { x, y, .. } | PrintItem::Ellipse { x, y, .. } | PrintItem::Shapes { x, y, .. } => {
            *x += dx;
            *y += dy;
        }
        PrintItem::Image { x, y, clip, .. } => {
            *x += dx;
            *y += dy;
            if let Some(c) = clip {
                c[0] += dx;
                c[1] += dy;
            }
        }
        PrintItem::Line { x1, y1, x2, y2, .. } => {
            *x1 += dx;
            *x2 += dx;
            *y1 += dy;
            *y2 += dy;
        }
        PrintItem::Text(t) => {
            for l in &mut t.lines {
                l.x += dx;
                l.baseline += dy;
            }
        }
    }
}

struct LayoutEnv<'a> {
    fonts: &'a mut FontSet,
    images: &'a mut ImageStore,
    /// Last value printed by each element with isPrintRepeatedValues="false".
    repeated: &'a HashMap<usize, String>,
    /// Nothing but page/column headers has been printed on this page yet.
    first_on_page: bool,
    /// Groups that changed with the current record.
    changed_groups: &'a [String],
    env: &'a EvalEnv<'a>,
    scope: &'a dyn Scope,
    /// Chart datasets collected so far.
    charts: &'a HashMap<usize, crate::chart::DataState>,
    /// The report being filled and its rows read so far (crosstabs over the report's data).
    report: &'a Report,
    records: &'a [Row<'a>],
    /// The least height the band being laid out may end with (the row of a horizontal list
    /// it shares with taller ones). Only for that band, not for what is inside it.
    min_height: f32,
}

/// Evaluates a chart's title, axis labels and range limits.
fn chart_texts(ch: &crate::chart::ChartElement, scope: &dyn Scope, env: &EvalEnv) -> Result<crate::chart::ChartTexts> {
    let s = |e: &Option<Expr>| -> Result<Option<String>> {
        Ok(match e {
            Some(e) => match eval(e, scope, env)? {
                Value::Null => None,
                v => Some(v.to_java_string()),
            },
            None => None,
        })
    };
    let n = |e: &Option<Expr>| -> Result<Option<f64>> {
        Ok(match e {
            Some(e) => eval(e, scope, env)?.as_f64(),
            None => None,
        })
    };
    let pair = |a: &Option<Expr>, b: &Option<Expr>| -> Result<Option<(f64, f64)>> {
        Ok(match (n(a)?, n(b)?) {
            (Some(lo), Some(hi)) => Some((lo, hi)),
            _ => None,
        })
    };
    let (dial, intervals) = match &ch.dial {
        Some(d) => (pair(&d.low, &d.high)?, d.intervals.iter().map(|i| pair(&i.low, &i.high)).collect::<Result<Vec<_>>>()?),
        None => (None, Vec::new()),
    };
    Ok(crate::chart::ChartTexts {
        title: s(&ch.title)?,
        subtitle: s(&ch.subtitle)?,
        category_label: s(&ch.category_axis_label)?,
        value_label: s(&ch.value_axis_label)?,
        value_min: n(&ch.value_min)?,
        value_max: n(&ch.value_max)?,
        dial,
        intervals,
        axes: ch.axes.iter().map(|(_, a)| chart_texts(a, scope, env)).collect::<Result<Vec<_>>>()?,
    })
}

/// A chart whose dataset runs on a subdataset: the rows of the run are filled into the
/// chart's dataset, as JasperReports does, by filling a report made over the subdataset whose
/// summary is the chart.
#[allow(clippy::too_many_arguments)]
fn chart_over_run(
    ch: &crate::chart::ChartElement,
    run: &DatasetRun,
    scope: &dyn Scope,
    env: &EvalEnv,
    fonts: &mut FontSet,
    images: &mut ImageStore,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> Result<Vec<PrintItem>> {
    let mut inner = ch.clone();
    inner.dataset.run = None;
    inner.evaluation_time = EvaluationTime::Now;
    inner.evaluation_group = None;
    crate::chart::copy_settings(ch, &inner);
    let report = crate::table::chart_report(inner, run, w, h);
    let (params, records) = run_inputs(run, &report, scope, env, "chart", images)?;
    if images.depth >= MAX_SUBREPORT_DEPTH {
        return Err(Error::Unsupported(format!("chart: nested more than {MAX_SUBREPORT_DEPTH} levels")));
    }
    check_time(images)?;
    images.runs += 1;
    let inner_env = EvalEnv { locale: env.locale, groovy: report.language == Language::Groovy, now: env.now };
    images.depth += 1;
    let filled = fill_flow(&report, &records, params, fonts, images, &inner_env);
    images.depth -= 1;
    let mut out = Vec::new();
    let mut top = y;
    for seg in filled?.0 {
        for item in seg.items {
            let mut item = item;
            translate(&mut item, x, top);
            out.push(item);
        }
        top += seg.height;
    }
    Ok(out)
}

/// Charts in a list of elements (frames included).
fn collect_charts<'a>(elements: &'a [Element], out: &mut Vec<&'a crate::chart::ChartElement>) {
    for el in elements {
        match &el.kind {
            ElementKind::Chart(c) => {
                out.push(c);
                // The charts of a multiple axis chart collect their own data.
                out.extend(c.axes.iter().map(|(_, a)| a));
            }
            ElementKind::Frame { children, .. } => collect_charts(children, out),
            _ => {}
        }
    }
}

/// Evaluates the text displayed by a text element.
/// The text of a text element, with the number behind it and its pattern (for exporters).
#[derive(Debug, Clone, Default)]
struct TextValue {
    text: String,
    number: Option<f64>,
    pattern: Option<String>,
}

fn text_value_full(content: &TextContent, scope: &dyn Scope, env: &EvalEnv) -> Result<TextValue> {
    match content {
        TextContent::Static(s) => Ok(TextValue { text: s.clone(), ..Default::default() }),
        TextContent::Expression { expr, pattern, pattern_expr, blank_when_null, .. } => {
            let value = match expr {
                Some(e) => eval(e, scope, env)?,
                None => Value::Null,
            };
            if value.is_null() {
                // Jasper prints the literal text "null" unless isBlankWhenNull is set.
                let text = if *blank_when_null { String::new() } else { "null".to_string() };
                return Ok(TextValue { text, ..Default::default() });
            }
            let pattern = match pattern_expr {
                Some(pe) => Some(eval(pe, scope, env)?.to_java_string()),
                None => pattern.clone(),
            };
            let number = value.as_f64();
            Ok(TextValue { text: format_value(&value, pattern.as_deref(), env.locale), number, pattern })
        }
    }
}

/// A text item with what it is in a tagged PDF (a heading), when its element says so.
fn tagged(mut item: PrintItem, tag: Option<Tag>) -> PrintItem {
    if let (Some(_), PrintItem::Cell(info, _)) = (tag, &mut item) {
        Arc::make_mut(info).tag = tag;
    }
    item
}

/// Wraps the item drawing a text element with what non-PDF exporters need. `printed` is the
/// text the cell keeps when [`text_item_with_cell`] already has it.
#[allow(clippy::too_many_arguments)]
fn cell_item(
    fonts: &mut FontSet,
    te: &TextElement,
    color: Color,
    back: Option<Color>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    value: &TextValue,
    drawn: PrintItem,
    printed: Option<String>,
) -> PrintItem {
    let (text, runs) = match printed {
        Some(text) => (text, Vec::new()),
        None => printed_text(fonts, te, color, w, h, &value.text),
    };
    let info = CellInfo {
        tag: None,
        x,
        y,
        w,
        h,
        text,
        runs,
        number: value.number,
        pattern: value.pattern.clone(),
        font: te.style.font.clone(),
        color,
        back,
        halign: te.style.halign,
        valign: te.style.valign,
        rotation: te.style.rotation,
        boxed: te.boxed,
        field: te.field.as_deref().map(|f| Arc::new(f.clone())),
    };
    PrintItem::Cell(Arc::new(info), Box::new(drawn))
}

/// The part of a text that fits its box, as JasperReports keeps it in the printed element (a
/// text that does not fit is cut after its last whole line). Styled and HTML text keep their
/// words; the markup becomes plain text.
fn printed_text(
    fonts: &mut FontSet,
    te: &TextElement,
    color: Color,
    w: f32,
    h: f32,
    text: &str,
) -> (String, Vec<crate::rich::Run>) {
    let (w, h) = match te.style.rotation {
        Rotation::Left | Rotation::Right => (h, w),
        _ => (w, h),
    };
    let p = te.boxed.padding;
    let (cw, ch) = ((w - p.left - p.right).max(0.0), h - p.top - p.bottom);
    let te = match text_form(fonts, te, color, text) {
        TextForm::Plain(te) => te,
        TextForm::Styled(st) => {
            let (k, total, rest) = crate::rich::lines_that_fit_in(&st, fonts, te, w, ch);
            let mut out = st.text;
            if k < total {
                out.truncate(rest);
                if out.ends_with('\n') {
                    out.pop();
                }
            }
            if te.markup == crate::rich::Markup::None {
                // Plain text in more than one font is still plain text to other exporters.
                return (out, Vec::new());
            }
            // Other exporters name the fonts the markup names (a font set, not its families).
            let runs =
                if fonts.resolves_fonts() { crate::rich::parse(text, &run_style(te, color), te.markup).runs } else { st.runs };
            let runs = runs
                .into_iter()
                .filter(|r| r.start < out.len() && r.end > r.start)
                .map(|mut r| {
                    r.end = r.end.min(out.len());
                    r
                })
                .collect();
            return (out, runs);
        }
    };
    let te = &*te;
    let hyphenator = fonts.hyphenator();
    let face_id = fonts.resolve(&te.style.font);
    let face = fonts.face(face_id);
    let lines = break_lines_hyphenated(text, face, te.style.font.size, cw, Measure::Fill, hyphenator);
    (printed_plain_text(te, face, text, &lines, cw, ch), Vec::new())
}

/// [`printed_text`] of a text in one font, from its fill-time `lines` in a box `cw` by `ch`
/// inside the padding.
fn printed_plain_text(
    te: &TextElement,
    face: &crate::fonts::FaceData,
    text: &str,
    lines: &[crate::text::Line],
    cw: f32,
    ch: f32,
) -> String {
    if te.truncate.keep_full {
        return text.to_string();
    }
    let size = te.style.font.size;
    let vm = vertical_metrics(face, size, te.style.line_spacing);
    let fit = vm.lines_that_fit(ch).min(lines.len());
    let cut = if fit < lines.len() { cut_text(text, lines, fit, face, size, cw, &te.truncate) } else { None };
    match lines.get(fit) {
        _ if cut.is_some() => cut.unwrap_or_default(),
        // Jasper keeps the spaces the last line ends with, and the line break too when the
        // rest of the text goes on to the next page.
        Some(next) => {
            let normalized = crate::text::normalize(text);
            let head = &normalized[..next.start];
            if te.stretch { head } else { head.strip_suffix('\n').unwrap_or(head) }.to_string()
        }
        None => text.to_string(),
    }
}

/// What JasperReports keeps of a text cut after `fit` of its fill `lines` when the element
/// asks for more than the default cut after the last whole line: the last line filled up to
/// the last character that fits (`truncate.at.char`), and a suffix that takes the place of
/// the last words or characters it needs room for (`truncate.suffix`). `None`: the default.
fn cut_text(
    text: &str,
    lines: &[crate::text::Line],
    fit: usize,
    face: &crate::fonts::FaceData,
    size: f32,
    cw: f32,
    t: &crate::model::Truncate,
) -> Option<String> {
    if !t.at_char && t.suffix.is_none() {
        return None;
    }
    let normalized = crate::text::normalize(text);
    if fit == 0 {
        return Some(t.suffix.clone().unwrap_or_default());
    }
    let start = lines[fit - 1].start;
    let mut end = lines.get(fit).map(|l| l.start).unwrap_or(normalized.len());
    if normalized[..end].ends_with('\n') {
        end -= 1;
    }
    let paragraph_end = normalized[start..].find('\n').map(|i| start + i).unwrap_or(normalized.len());
    let fits = |s: &str| face.fill_width(s, size) <= cw + 1e-3;
    if t.at_char {
        let from = end;
        for (i, c) in normalized[from..paragraph_end].char_indices() {
            let next = from + i + c.len_utf8();
            if !fits(&normalized[start..next]) {
                break;
            }
            end = next;
        }
        end = end.max(lines[fit - 1].start);
    }
    let suffix = t.suffix.as_deref().unwrap_or("");
    if !suffix.is_empty() {
        while end > start && !fits(&format!("{}{suffix}", &normalized[start..end])) {
            if t.at_char {
                end = normalized[..end].char_indices().next_back().map(|(i, _)| i).unwrap_or(start).max(start);
            } else {
                // Drop the last word, keeping the space before it.
                let units = crate::text::java_units(&normalized[start..end]);
                let last = units.last().map(|u| u.len()).unwrap_or(end - start);
                end -= last;
            }
        }
    }
    Some(format!("{}{suffix}", &normalized[..end]))
}

/// The base style of a text element's runs.
fn run_style(te: &TextElement, color: Color) -> crate::rich::RunStyle {
    crate::rich::RunStyle { font: te.style.font.clone(), color, back: None, script: crate::rich::Script::Normal }
}

/// Lays out text inside a box, returning what to draw and the height the text needs.
#[allow(clippy::too_many_arguments)]
fn text_item(
    fonts: &mut FontSet,
    te: &TextElement,
    color: Color,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: &str,
    clip: bool,
) -> (PrintItem, f32) {
    if te.style.rotation != Rotation::None {
        // Lay the text out in the rotated box at the origin, then turn it into place, like
        // JasperReports' PDF exporter (padding turns with the text).
        let mut r = te.clone();
        r.style.rotation = Rotation::None;
        let p = te.boxed.padding;
        // The rotated box is laid out with its top-left corner on the anchor, a corner of the
        // element, and turned around that point (the same transform Jasper writes).
        let (lw, lh, (ax, ay), m) = match te.style.rotation {
            Rotation::Left => {
                r.boxed.padding = Padding { top: p.left, left: p.bottom, bottom: p.right, right: p.top };
                let (ax, ay) = (x, y + h);
                (h, w, (ax, ay), [0.0, -1.0, 1.0, 0.0, ax - ay, ay + ax])
            }
            Rotation::Right => {
                r.boxed.padding = Padding { top: p.right, right: p.bottom, bottom: p.left, left: p.top };
                let (ax, ay) = (x + w, y);
                (h, w, (ax, ay), [0.0, 1.0, -1.0, 0.0, ax + ay, ay - ax])
            }
            _ => {
                r.boxed.padding = Padding { top: p.bottom, bottom: p.top, left: p.right, right: p.left };
                let (ax, ay) = (x + w, y + h);
                (w, h, (ax, ay), [-1.0, 0.0, 0.0, -1.0, 2.0 * ax, 2.0 * ay])
            }
        };
        let (item, _) = text_item(fonts, &r, color, ax, ay, lw, lh, text, clip);
        // Rotated text does not stretch the element.
        return (PrintItem::Transform(m, vec![item]), h);
    }
    match text_form(fonts, te, color, text) {
        TextForm::Styled(st) => crate::rich::layout(fonts, te, &st, x, y, w, h, clip),
        TextForm::Plain(te) => {
            let (t, needed, _) = plain_text_item(fonts, &te, color, x, y, w, h, text, clip);
            (PrintItem::Text(t), needed)
        }
    }
}

/// [`text_item`] (clipped to its box) for an element whose cell is wanted too, with the text
/// the cell keeps ([`printed_text`]) when that comes from the same layout: for plain text that
/// is not rotated, [`printed_text`] breaks the text with the same font, size, box and
/// hyphenation as the fill-time lines laid out for drawing, so those lines are used instead of
/// being broken again (and `text_form` is not run twice). `None`: [`cell_item`] works it out.
#[allow(clippy::too_many_arguments)]
fn text_item_with_cell(
    fonts: &mut FontSet,
    te: &TextElement,
    color: Color,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: &str,
) -> (PrintItem, Option<String>) {
    if te.style.rotation != Rotation::None {
        return (text_item(fonts, te, color, x, y, w, h, text, true).0, None);
    }
    match text_form(fonts, te, color, text) {
        TextForm::Styled(st) => (crate::rich::layout(fonts, te, &st, x, y, w, h, true).0, None),
        TextForm::Plain(te) => {
            let (t, _, fill_lines) = plain_text_item(fonts, &te, color, x, y, w, h, text, true);
            let p = te.boxed.padding;
            let (cw, ch) = ((w - p.left - p.right).max(0.0), h - p.top - p.bottom);
            // The face the text was laid out in (`plain_text_item` resolves the element's font).
            let printed = printed_plain_text(&te, fonts.face(t.face), text, &fill_lines, cw, ch);
            (PrintItem::Text(t), Some(printed))
        }
    }
}

/// How a text element's text is laid out: in its font alone, or as styled runs. Short-lived,
/// on the stack: the element is copied only when its font changes, which is rare.
#[allow(clippy::large_enum_variant)]
enum TextForm<'a> {
    /// In one font: the element's, or the one family of its font set (or fallback) that has
    /// every character.
    Plain(std::borrow::Cow<'a, TextElement>),
    /// Styled or HTML markup, or a text drawn in more than one font (font sets, fallback).
    Styled(crate::rich::StyledText),
}

fn text_form<'a>(fonts: &mut FontSet, te: &'a TextElement, color: Color, text: &str) -> TextForm<'a> {
    if te.markup != crate::rich::Markup::None {
        let mut st = crate::rich::parse(text, &run_style(te, color), te.markup);
        st.resolve_fonts(fonts);
        return TextForm::Styled(st);
    }
    if text.contains('\t') || fonts.paragraph(te).is_some() {
        // Tab stops and indents are laid out by the styled text engine, a run of one style.
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut st = crate::rich::StyledText::plain(&text, &run_style(te, color));
        st.resolve_fonts(fonts);
        return TextForm::Styled(st);
    }
    let normalized = crate::text::normalize(text);
    match fonts.fallback_runs(&te.style.font, &normalized) {
        None => TextForm::Plain(std::borrow::Cow::Borrowed(te)),
        Some(runs) if runs.len() == 1 => {
            let mut te = te.clone();
            te.style.font.name = runs[0].2.clone();
            TextForm::Plain(std::borrow::Cow::Owned(te))
        }
        Some(runs) => {
            let base = run_style(te, color);
            let runs = runs
                .into_iter()
                .map(|(start, end, family)| {
                    let mut style = base.clone();
                    style.font.name = family;
                    crate::rich::Run { start, end, style }
                })
                .collect();
            TextForm::Styled(crate::rich::StyledText { text: normalized.into_owned(), runs, paras: Vec::new() })
        }
    }
}

/// The width of the widest line of `text` laid out in `te` `w` wide, and the height `te` needs
/// for it, padding included: what JasperReports measures while filling
/// (`JRFillTextElement.getTextWidth`, and the text height of `chopTextElement`). An empty text
/// is not measured: no width, no height.
fn text_extent(fonts: &mut FontSet, te: &TextElement, color: Color, w: f32, text: &str) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    let p = te.boxed.padding;
    match text_form(fonts, te, color, text) {
        TextForm::Plain(te) => {
            let hyphenator = fonts.hyphenator();
            let id = fonts.resolve(&te.style.font);
            let face = fonts.face(id);
            let size = te.style.font.size;
            let lines = break_lines_hyphenated(text, face, size, (w - p.left - p.right).max(0.0), Measure::Fill, hyphenator);
            let vm = vertical_metrics(face, size, te.style.line_spacing);
            // A line's own width is the drawn one (iText's); filling measures with AWT's exact
            // advances (shaped text has them already).
            let width = |l: &crate::text::Line| {
                if crate::shape::needs_shaping(&l.text) {
                    l.width
                } else {
                    face.fill_width(&l.text, size)
                }
            };
            (lines.iter().map(width).fold(0.0, f32::max), p.top + p.bottom + vm.block_height(lines.len()))
        }
        TextForm::Styled(_) => {
            let (item, needed) = text_item(fonts, te, color, 0.0, 0.0, w, f32::MAX, text, false);
            // The runs of a line share its baseline.
            fn lines(item: &PrintItem, out: &mut Vec<(f32, f32, f32)>) {
                match item {
                    PrintItem::Text(t) => out.extend(t.lines.iter().map(|l| (l.baseline, l.x, l.x + l.width))),
                    PrintItem::Group(items) | PrintItem::Paragraphs(_, items) | PrintItem::Transform(_, items) => {
                        items.iter().for_each(|i| lines(i, out))
                    }
                    PrintItem::Cell(_, item) => lines(item, out),
                    _ => {}
                }
            }
            let mut all = Vec::new();
            lines(&item, &mut all);
            let mut widest = 0.0f32;
            for &(baseline, ..) in &all {
                let on = all.iter().filter(|l| (l.0 - baseline).abs() < 1e-3);
                let (start, end) = on.fold((f32::INFINITY, f32::NEG_INFINITY), |(s, e), l| (s.min(l.1), e.max(l.2)));
                widest = widest.max(end - start);
            }
            (widest, needed)
        }
    }
}

/// Measures an icon label (a frame with [`crate::iconlabel::of`]) as JasperReports prepares it.
fn prepare_icon_label(
    le: &mut LayoutEnv,
    el: &Element,
    boxed: &BoxStyle,
    label: &Element,
    icon: &Element,
) -> Result<crate::iconlabel::Prepared> {
    // The text of each field; a field not printed (or evaluated later) measures nothing.
    fn field<'e>(le: &LayoutEnv, child: &'e Element) -> Result<(&'e TextElement, String)> {
        let ElementKind::Text(te) = &child.kind else { unreachable!("an icon label's fields are texts") };
        let visible = match &child.print_when {
            Some(e) => condition(&eval(e, le.scope, le.env)?, le.env)?,
            None => true,
        };
        let deferred = matches!(&te.content, TextContent::Expression { evaluation_time, .. }
            if !matches!(evaluation_time, EvaluationTime::Now | EvaluationTime::Band));
        let text = if visible && !deferred { text_value_full(&te.content, le.scope, le.env)?.text } else { String::new() };
        Ok((te, text))
    }
    let (label_te, label_text) = field(le, label)?;
    let (icon_te, icon_text) = field(le, icon)?;
    let fonts = &mut *le.fonts;
    let mut measure = |is_icon: bool, w: f32| {
        let (child, te, text) = if is_icon { (icon, icon_te, &icon_text) } else { (label, label_te, &label_text) };
        let (text_width, needed) = text_extent(fonts, te, child.forecolor, w, text);
        // A field that stretches is as tall as its text, in whole points, or as designed.
        let height = if te.stretch { child.height.max((needed - 1e-3).ceil()) } else { child.height };
        crate::iconlabel::Measured { text_width, height }
    };
    let pad = |te: &TextElement| te.boxed.padding.left + te.boxed.padding.right;
    let fields = crate::iconlabel::Fields {
        label_padding: pad(label_te),
        icon_padding: pad(icon_te),
        icon_font_size: icon_te.style.font.size,
        label_stretches: label_te.stretch,
    };
    Ok(crate::iconlabel::prepare(el.width, el.height, boxed.padding, f32::INFINITY, &fields, &mut measure))
}

/// The label and the icon of an icon label placed in its frame, `width` wide and finally
/// `height` tall.
fn icon_label_children(
    settings: crate::iconlabel::Settings,
    prepared: &crate::iconlabel::Prepared,
    boxed: &BoxStyle,
    label: &Element,
    icon: &Element,
    width: f32,
    height: f32,
) -> Vec<Element> {
    let placed = crate::iconlabel::place(settings, prepared, width, boxed.padding, height);
    let put = |e: &Element, [x, y, w, h]: [f32; 4]| {
        let mut e = e.clone();
        // Inside the frame's padding (exporters offset a frame's elements by it); the fields
        // do not stretch with the frame.
        (e.x, e.y, e.width, e.height) = (prepared.left + x, boxed.padding.top + y, w, h);
        e.stretch_type = StretchType::NoStretch;
        e.position_type = PositionType::Fix;
        e
    };
    let mut out = vec![put(label, placed.label)];
    out.extend(placed.icon.map(|b| put(icon, b)));
    out
}

#[allow(clippy::too_many_arguments)]
fn plain_text_item(
    fonts: &mut FontSet,
    te: &TextElement,
    color: Color,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: &str,
    clip: bool,
) -> (PrintText, f32, Vec<crate::text::Line>) {
    let face_id = fonts.resolve(&te.style.font);
    let face_arc = fonts.faces()[face_id].clone();
    let face = &*face_arc;
    let size = te.style.font.size;
    let p = te.boxed.padding;
    let (cx, cy, cw, ch) = (x + p.left, y + p.top, (w - p.left - p.right).max(0.0), h - p.top - p.bottom);
    // Like JasperReports: the fill-time measurement decides the height and where the text is
    // cut; the export-time measurement decides the drawn line breaks.
    let vm = vertical_metrics(face, size, te.style.line_spacing);
    let hyphenator = fonts.hyphenator();
    let fill_lines = break_lines_hyphenated(text, face, size, cw, Measure::Fill, hyphenator);
    let needed = p.top + p.bottom + vm.block_height(fill_lines.len());
    let fit = if clip { vm.lines_that_fit(ch).min(fill_lines.len()) } else { fill_lines.len() };
    let rendered_text = if fit < fill_lines.len() {
        cut_text(text, &fill_lines, fit, face, size, cw, &te.truncate).unwrap_or_else(|| text_before_line(text, &fill_lines, fit))
    } else {
        text.to_string()
    };
    // Text JasperReports writes in a PDF standard font: iText breaks it again with that
    // font's widths.
    let standard = fonts.standard(&te.style.font, &rendered_text);
    let render = match standard {
        Some(st) => Measure::Standard(st.font),
        None => Measure::Render,
    };
    let mut lines = break_lines_hyphenated(&rendered_text, face, size, cw, render, hyphenator);
    if clip {
        lines.truncate(vm.lines_that_fit(ch));
    }
    let block = vm.block_height(lines.len());
    let advance = vm.advance(lines.len());
    let voff = match te.style.valign {
        VAlign::Top => 0.0,
        VAlign::Middle => (ch - block) / 2.0,
        VAlign::Bottom => ch - block,
    };
    // Justified lines spread the leftover width over their spaces. The last line of a
    // paragraph keeps the spacing of the line before it (JasperReports 6 behaviour).
    let face_ref = fonts.face(face_id);
    // JasperReports puts the first baseline the font's line gap (leading) and its ascent below
    // the top.
    let leading = face_ref.line_gap * size / face_ref.units_per_em;
    let normalized = crate::text::normalize(&rendered_text);
    let starts: Vec<usize> = lines.iter().map(|l| l.start).collect();
    let print_lines_next_start = |_: &str, k: usize| starts.get(k + 1).copied();
    let starts_paragraph = |n: &str, start: usize| start == 0 || n.as_bytes().get(start - 1) == Some(&b'\n');
    let mut prev_spacing = 0.0f32;
    let mut print_lines = Vec::with_capacity(lines.len());
    // Text that starts a paragraph right to left runs right to left as a whole, as in
    // JasperReports; alignment stays where the element puts it (left is left).
    let complex = standard.is_none() && crate::shape::needs_shaping(&rendered_text);
    let rtl = complex && crate::shape::is_rtl(&rendered_text);
    for (k, mut l) in lines.into_iter().enumerate() {
        let shaped = if complex { crate::shape::line(face_ref, &l.text, rtl, size) } else { None };
        let shaped = shaped.map(|(glyphs, width)| {
            l.width = width;
            glyphs
        });
        let mut word_spacing = 0.0;
        if te.style.halign == HAlign::Justified {
            let ends_paragraph = match print_lines_next_start(&normalized, k) {
                Some(next) => normalized.as_bytes().get(next.wrapping_sub(1)) == Some(&b'\n'),
                None => true,
            };
            let spaces = l.text.matches(' ').count();
            // iText lets 40% of a trailing , . ; or : hang into the margin.
            let hanging = match l.text.chars().last().filter(|_| !rtl) {
                Some(c @ (',' | '.' | ';' | ':')) => match render {
                    Measure::Standard(font) => 0.4 * crate::standard::width(font, c.encode_utf8(&mut [0; 4]), size),
                    _ => 0.4 * face_ref.char_width(c, size),
                },
                _ => 0.0,
            };
            word_spacing = if !ends_paragraph && spaces > 0 {
                (cw - (l.width - hanging)) / spaces as f32
            } else if ends_paragraph && k > 0 && !starts_paragraph(&normalized, l.start) {
                prev_spacing
            } else {
                0.0
            };
            prev_spacing = word_spacing;
        }
        let spaces = l.text.matches(' ').count() as f32;
        let width = l.width + word_spacing * spaces;
        let lx = match te.style.halign {
            HAlign::Center => cx + (cw - l.width) / 2.0,
            HAlign::Right => cx + cw - l.width,
            // The last line of a justified right-to-left paragraph sits on the right.
            HAlign::Justified if rtl => cx + cw - width,
            _ => cx,
        };
        print_lines.push(PrintLine {
            x: lx,
            baseline: cy + voff + leading + vm.ascent + advance * k as f32,
            width,
            word_spacing,
            text: l.text,
            shaped,
        });
    }
    let f = &te.style.font;
    let halign = te.style.halign;
    (
        PrintText { face: face_id, size, color, underline: f.underline, strike: f.strike, lines: print_lines, standard, halign },
        needed,
        fill_lines,
    )
}

/// Whether JasperReports prints the element's content in a frame (tables and lists do,
/// subreports do not).
fn is_framed(kind: &ElementKind) -> bool {
    match kind {
        ElementKind::Table(_) | ElementKind::List(_) => true,
        ElementKind::Content { framed, .. } => *framed,
        _ => false,
    }
}

/// Starts the items of a non-text element with its marker for spreadsheet exporters; close it
/// with [`close_mark`] once its items are pushed.
#[allow(clippy::too_many_arguments)]
fn open_mark(out: &mut Vec<Laid>, el: &Element, shape: Shape, boxed: Option<BoxStyle>, x: f32, y: f32, w: f32, h: f32) -> usize {
    let info = ElementInfo {
        x,
        y,
        w,
        h,
        shape,
        back: (el.mode == Mode::Opaque).then_some(el.backcolor),
        fore: el.forecolor,
        boxed,
        len: 0,
        alt: None,
        // A table is what its element says it is, whatever its content.
        tag: el.tag.or(matches!(el.kind, ElementKind::Table(_)).then_some(Tag::Table)),
    };
    out.push(Laid { item: PrintItem::Element(Arc::new(info)), deferred: None });
    out.len() - 1
}

/// Where an element's link leads, the anchor it is and its bookmark, evaluated now.
fn link_info(le: &mut LayoutEnv, link: &crate::model::Hyperlink, x: f32, y: f32, w: f32, h: f32) -> Result<Option<LinkInfo>> {
    use crate::model::LinkKind;
    let text = |e: &Option<Expr>| -> Result<Option<String>> {
        Ok(match e {
            Some(e) => match eval(e, le.scope, le.env)? {
                Value::Null => None,
                v => Some(v.to_java_string()).filter(|s| !s.is_empty()),
            },
            None => None,
        })
    };
    let reference = text(&link.reference)?;
    let anchor = text(&link.anchor)?;
    let page = text(&link.page)?.and_then(|p| p.trim().parse::<f64>().ok()).filter(|p| *p >= 1.0).map(|p| p as u32);
    let target = match link.kind {
        LinkKind::None => None,
        LinkKind::Reference => reference.map(LinkTarget::Url),
        LinkKind::LocalAnchor => anchor.map(LinkTarget::Anchor),
        LinkKind::LocalPage => page.map(LinkTarget::Page),
        LinkKind::RemoteAnchor => reference.zip(anchor).map(|(file, anchor)| LinkTarget::RemoteAnchor { file, anchor }),
        LinkKind::RemotePage => reference.zip(page).map(|(file, page)| LinkTarget::RemotePage { file, page }),
    };
    let tooltip = text(&link.tooltip)?;
    let anchor_name = text(&link.anchor_name)?;
    let bookmark = match &link.bookmark_level_expr {
        Some(e) => match eval(e, le.scope, le.env)? {
            Value::Null => link.bookmark_level,
            v => v.to_java_string().trim().parse::<f64>().map(|v| v.max(0.0) as u32).unwrap_or(link.bookmark_level),
        },
        None => link.bookmark_level,
    };
    if target.is_none() && anchor_name.is_none() {
        return Ok(None);
    }
    Ok(Some(LinkInfo { x, y, w, h, target, tooltip, anchor: anchor_name, bookmark }))
}

fn close_mark(out: &mut [Laid], mark: usize) {
    let len = out.len() - mark - 1;
    if let PrintItem::Element(info) = &mut out[mark].item {
        Arc::make_mut(info).len = len;
    }
}

fn box_items(boxed: &BoxStyle, x: f32, y: f32, w: f32, h: f32, out: &mut Vec<Laid>) {
    let mut push = |pen: Pen, x1: f32, y1: f32, x2: f32, y2: f32| {
        if pen.width > 0.0 {
            out.push(Laid { item: PrintItem::Line { x1, y1, x2, y2, pen }, deferred: None });
        }
    };
    // Like JasperReports, each side extends by half the width of its neighbours so corners
    // are closed.
    let (t, l, b, r) = (boxed.top.width / 2.0, boxed.left.width / 2.0, boxed.bottom.width / 2.0, boxed.right.width / 2.0);
    push(boxed.top, x - l, y, x + w + r, y);
    push(boxed.left, x, y - t, x, y + h + b);
    push(boxed.bottom, x - l, y + h, x + w + r, y + h);
    push(boxed.right, x + w, y - t, x + w, y + h + b);
}

/// Evaluates an image expression and loads the image it designates.
fn load_image(le: &mut LayoutEnv, img: &ImageElement) -> Result<Option<Arc<ImageData>>> {
    let value = match &img.expr {
        Some(e) => eval(e, le.scope, le.env)?,
        None => return Ok(None),
    };
    let loaded = match &value {
        Value::Null => return Ok(None),
        Value::Str(location) => le.images.load_location(location),
        Value::Bytes(bytes) => le.images.load_bytes(bytes),
        other => Err(Error::eval(format!("image expression returned {}", other.type_name()))),
    };
    match (loaded, img.on_error) {
        (Ok(data), _) => Ok(Some(data)),
        (Err(e), OnErrorType::Error) => Err(e),
        (Err(_), _) => Ok(None),
    }
}

/// Positions an image inside its box the way JasperReports' PDF exporter does: natural size
/// is one point per pixel, alignment offsets are whole points.
/// The height (and for `RealSize` the width) of an image element sized from its picture, the
/// way `JRFillImage.fitImage` works it out in whole points: the picture at one point a pixel,
/// narrowed to the element's width; on a page with less `room`, it goes to the next page,
/// or, at the top of a page already, shrinks to the room.
fn real_image_size(
    real: RealImage,
    data: &ImageData,
    el: &Element,
    room: Option<f32>,
    first_on_page: bool,
) -> (f32, Option<f32>) {
    let (real_w, real_h) = (f64::from(data.width), f64::from(data.height));
    let width = el.width.trunc() as f64;
    let (mut h, mut w) = (real_h, real_w);
    if real_w > width {
        h = (width / real_w * real_h).trunc();
        w = width;
    }
    if let Some(room) = room.map(|r| r.trunc() as f64) {
        if h > room && first_on_page {
            h = room.max(0.0);
            w = (h / real_h * real_w).trunc();
        }
    }
    (h as f32, (real == RealImage::Size).then_some(w as f32))
}

fn place_image(img: &ImageElement, data: Arc<ImageData>, x: f32, y: f32, w: f32, h: f32) -> PrintItem {
    let p = img.boxed.padding;
    let (bx, by) = (x + p.left, y + p.top);
    let (bw, bh) = ((w - p.left - p.right).max(0.0), (h - p.top - p.bottom).max(0.0));
    let (iw, ih) = (data.width as f32, data.height as f32);
    let factor = |a: HAlign| match a {
        HAlign::Center => 0.5,
        HAlign::Right => 1.0,
        _ => 0.0,
    };
    let vfactor = match img.valign {
        VAlign::Middle => 0.5,
        VAlign::Bottom => 1.0,
        VAlign::Top => 0.0,
    };
    let hfactor = factor(img.halign);
    match img.scale {
        ScaleImage::FillFrame => PrintItem::Image { x: bx, y: by, w: bw, h: bh, clip: None, image: data },
        ScaleImage::RetainShape => {
            // An SVG keeps its exact proportions (its size in whole points is for Clip).
            let (iw, ih) = match &data.pixels {
                crate::image::Pixels::Svg(svg) => (svg.tree.size().width(), svg.tree.size().height()),
                _ => (iw, ih),
            };
            let ratio = (bw / iw).min(bh / ih);
            let (dw, dh) = (iw * ratio, ih * ratio);
            let ox = (hfactor * (bw - dw)).trunc();
            let oy = (vfactor * (bh - dh)).trunc();
            PrintItem::Image { x: bx + ox, y: by + oy, w: dw, h: dh, clip: None, image: data }
        }
        ScaleImage::Clip => {
            let ox = (hfactor * (bw - iw)).trunc();
            let oy = (vfactor * (bh - ih)).trunc();
            PrintItem::Image { x: bx + ox, y: by + oy, w: iw, h: ih, clip: Some([bx, by, bw, bh]), image: data }
        }
    }
}

/// Per-element layout result before vertical positioning.
struct Measured {
    visible: bool,
    new_h: f32,
    /// Not printed or empty (for isRemoveLineWhenBlank).
    blank: bool,
    text: Option<String>,
    /// The number behind the text and its pattern (for exporters).
    number: Option<f64>,
    pattern: Option<String>,
    children: Option<BandLayout>,
    image: Option<Arc<ImageData>>,
    /// Filled subreport content.
    content: Option<Arc<Vec<Segment>>>,
    /// Laid-out barcode symbol.
    symbol: Option<crate::barcode::Symbol>,
    /// An image sized from its picture (`RealSize`): its x offset and width.
    image_box: Option<(f32, f32)>,
    /// A crosstab: where it starts, and the room it gets on this page (`None`: all of it).
    crosstab: Option<(crate::crosstab::Rest, Option<f32>)>,
    /// An icon label: its settings and what measuring it gave, to place its fields again once
    /// its final height is known.
    icon_label: Option<(crate::iconlabel::Settings, crate::iconlabel::Prepared)>,
}

/// Lays out `elements` in a container of design height `design_h`. With `avail`, content
/// below that height is not printed but returned as [`BandLayout::rest`], the way
/// JasperReports splits a stretching band or frame across pages.
fn layout_elements(le: &mut LayoutEnv, original: &[Element], design_h: f32, avail: Option<f32>) -> Result<BandLayout> {
    let min_height = std::mem::take(&mut le.min_height);
    // 0. Pick each element's conditional-style variant.
    let mut resolved: Vec<std::borrow::Cow<Element>> = Vec::with_capacity(original.len());
    for el in original {
        resolved.push(match &el.conditional {
            Some(c) => {
                let mut mask = 0;
                for (i, cond) in c.conditions.iter().enumerate() {
                    if condition(&eval(cond, le.scope, le.env)?, le.env)? {
                        mask |= 1 << i;
                    }
                }
                let v = &c.variants[mask];
                let mut e = el.clone();
                e.conditional = None;
                e.mode = v.mode;
                e.forecolor = v.forecolor;
                e.backcolor = v.backcolor;
                e.kind = v.kind.clone();
                std::borrow::Cow::Owned(e)
            }
            None => std::borrow::Cow::Borrowed(el),
        });
    }
    let elements: Vec<&Element> = resolved.iter().map(|c| c.as_ref()).collect();
    let mut printed_values = Vec::new();
    let mut returns = Vec::new();

    // 1. Evaluate and measure every element at its own size.
    let mut measured = Vec::with_capacity(elements.len());
    for (idx, el) in elements.iter().enumerate() {
        let visible = match &el.print_when {
            Some(e) => condition(&eval(e, le.scope, le.env)?, le.env)?,
            None => true,
        };
        let mut m = Measured {
            visible,
            blank: !visible,
            new_h: el.height,
            text: None,
            number: None,
            pattern: None,
            children: None,
            image: None,
            content: None,
            symbol: None,
            crosstab: None,
            image_box: None,
            icon_label: None,
        };
        if visible {
            match &el.kind {
                ElementKind::Text(te) => {
                    let deferred = matches!(&te.content, TextContent::Expression { evaluation_time, .. }
                        if !matches!(evaluation_time, EvaluationTime::Now | EvaluationTime::Band));
                    if !deferred {
                        let tv = text_value_full(&te.content, le.scope, le.env)?;
                        m.number = tv.number;
                        m.pattern = tv.pattern;
                        let text = tv.text;
                        if te.stretch {
                            let (_, needed) = text_item(le.fonts, te, el.forecolor, 0.0, 0.0, el.width, el.height, &text, false);
                            // JasperReports 6 keeps element geometry in whole points.
                            m.new_h = el.height.max((needed - 1e-3).ceil());
                        }
                        m.blank = text.is_empty();
                        m.text = Some(text);
                    }
                }
                ElementKind::Image(img) => {
                    m.image = load_image(le, img)?;
                    m.blank = m.image.is_none();
                    if let (Some(real), Some(data)) = (img.real, &m.image) {
                        let room = avail.map(|a| a - el.y - img.boxed.padding.top - img.boxed.padding.bottom);
                        let (h, w) = real_image_size(real, data, el, room, le.first_on_page);
                        m.new_h = h + img.boxed.padding.top + img.boxed.padding.bottom;
                        if let Some(w) = w.filter(|w| *w != el.width) {
                            let dx = match img.halign {
                                HAlign::Right => el.width - w,
                                HAlign::Center => (0.5 * (el.width - w)).trunc(),
                                _ => 0.0,
                            };
                            m.image_box = Some((dx, w));
                        }
                    }
                }
                ElementKind::Frame { children, boxed } if crate::iconlabel::of(children).is_some() => {
                    let Some((settings, label, icon)) = crate::iconlabel::of(children) else { unreachable!() };
                    // It takes the room it asks for in the band, never less than designed, but
                    // is printed as tall as it asks to be (see step 4), its fields placed for
                    // that height.
                    let prepared = prepare_icon_label(le, el, boxed, label, icon)?;
                    m.new_h = el.height.max(prepared.height);
                    let fields = icon_label_children(settings, &prepared, boxed, label, icon, el.width, prepared.height);
                    let inner = layout_elements(le, &fields, prepared.height, None)?;
                    printed_values.extend(inner.printed_values.iter().cloned());
                    returns.extend(inner.returns.iter().cloned());
                    m.children = Some(inner);
                    m.icon_label = Some((settings, prepared));
                }
                ElementKind::Frame { children, .. } => {
                    let inner = layout_elements(le, children, el.height, None)?;
                    m.new_h = el.height.max(inner.height);
                    printed_values.extend(inner.printed_values.iter().cloned());
                    returns.extend(inner.returns.iter().cloned());
                    m.children = Some(inner);
                }
                ElementKind::Subreport(sr) => {
                    let (segments, values) = run_subreport(le, sr)?;
                    returns.extend(values);
                    m.new_h = el.height.max(Segment::flow_height(&segments));
                    m.blank = segments.is_empty();
                    m.content = Some(Arc::new(segments));
                }
                ElementKind::List(l) => {
                    let report = crate::table::list_report(l, el.width);
                    let segments = run_dataset_report(le, &l.run, &report, "list")?;
                    m.new_h = el.height.max(Segment::flow_height(&segments));
                    m.blank = segments.is_empty();
                    m.content = Some(Arc::new(segments));
                }
                ElementKind::Table(t) => {
                    let segments = run_table(le, t, el.width)?;
                    m.new_h = el.height.max(Segment::flow_height(&segments));
                    m.blank = segments.is_empty();
                    m.content = Some(Arc::new(segments));
                }
                ElementKind::Barcode(bc) => {
                    m.symbol = barcode_symbol(bc, el, le)?;
                    m.blank = m.symbol.is_none();
                }
                ElementKind::Content { segments, .. } => {
                    m.new_h = el.height.max(Segment::flow_height(segments));
                    m.content = Some(segments.clone());
                }
                ElementKind::Crosstab(ct) => {
                    let filled = fill_crosstab(le, ct, el)?;
                    let rest = crate::crosstab::Rest { filled, part: 0, row: 0 };
                    m.new_h = el.height.max(rest.filled.plan(0, 0, f32::INFINITY).1);
                    m.blank = rest.filled.rows.is_empty();
                    m.crosstab = Some((rest, None));
                }
                ElementKind::CrosstabRest(rest) => {
                    m.new_h = rest.filled.plan(rest.part, rest.row, f32::INFINITY).1;
                    m.crosstab = Some((rest.clone(), None));
                }
                _ => {}
            }
            // isPrintRepeatedValues="false": skip when the value is the one printed last time.
            if !el.print_repeated_values {
                let key = &original[idx] as *const Element as usize;
                let value = m.text.clone().unwrap_or_default();
                let reprint = (el.print_in_first_whole_band && le.first_on_page)
                    || el.print_when_group_changes.as_ref().is_some_and(|g| le.changed_groups.contains(g));
                // A static text always says the same, so it counts as repeated from the start:
                // it only prints where a repeated value is printed again. (With
                // isPrintWhenDetailOverflows that makes the "(continued)" label of a band
                // that goes on to the next page.)
                let fixed = matches!(&el.kind, ElementKind::Text(te) if matches!(te.content, TextContent::Static(_)));
                if !reprint && (fixed || le.repeated.get(&key) == Some(&value)) {
                    m.visible = false;
                    m.blank = true;
                } else {
                    printed_values.push((key, value));
                }
            }
        }
        measured.push(m);
    }

    // 1b. isRemoveLineWhenBlank: blank elements print nothing; whether their space goes too is
    // decided once the others have stretched and moved (2b).
    let ey: Vec<f32> = elements.iter().map(|e| e.y).collect();
    let mut design_h = design_h;
    let mut removable: Vec<usize> = Vec::new();
    for (i, el) in elements.iter().enumerate() {
        if el.remove_line_when_blank && measured[i].blank {
            removable.push(i);
            measured[i].visible = false;
        }
    }

    // 2. Float elements down by the growth of the elements above them.
    let mut new_y: Vec<f32> = ey.clone();
    let mut order: Vec<usize> = (0..elements.len()).collect();
    order.sort_by(|&a, &b| ey[a].partial_cmp(&ey[b]).unwrap_or(std::cmp::Ordering::Equal));
    for (pos, &i) in order.iter().enumerate() {
        let el = &elements[i];
        if el.position_type != PositionType::Float {
            continue;
        }
        let mut shift: f32 = 0.0;
        for &j in &order[..pos] {
            let above = &elements[j];
            let overlaps = above.x < el.x + el.width && el.x < above.x + above.width;
            if ey[j] + above.height <= ey[i] + 1e-3 && overlaps && measured[j].visible {
                let growth = new_y[j] + measured[j].new_h - (ey[j] + above.height);
                shift = shift.max(growth);
            }
        }
        new_y[i] = ey[i] + shift.max(0.0);
    }

    // 2b. Each blank element's range collapses when nothing else printed overlaps it where it
    // is now, after the elements above it have stretched and floated: what is below moves up,
    // and what is above keeps that much less space to the container's bottom. This is
    // JasperReports' JRFillElementContainer.removeBlankElements, run in the elements' order;
    // without stretching it comes to the ranges of the design.
    let mut collapsed_below = vec![0.0f32; elements.len()];
    let mut removed = 0.0f32;
    for &i in &removable {
        let gap = elements[i].height;
        let (top, bottom) = (new_y[i], new_y[i] + gap);
        if gap <= 0.0 || top < -1e-3 {
            continue;
        }
        let printed = |j: usize| measured[j].visible && !measured[j].blank;
        let container_bottom = if (0..elements.len()).any(|j| measured[j].visible) {
            (0..elements.len())
                .filter(|&j| measured[j].visible)
                .map(|j| new_y[j] + measured[j].new_h + (design_h - (ey[j] + elements[j].height) - collapsed_below[j]).max(0.0))
                .fold(0.0f32, f32::max)
        } else {
            design_h - removed
        };
        if bottom > container_bottom + 1e-3 {
            continue;
        }
        let occupied = (0..elements.len())
            .any(|j| j != i && printed(j) && new_y[j] < bottom - 1e-3 && new_y[j] + measured[j].new_h > top + 1e-3);
        if occupied {
            continue;
        }
        for j in 0..elements.len() {
            if new_y[j] + measured[j].new_h <= top + 1e-3 && j != i {
                collapsed_below[j] += gap;
            }
            if new_y[j] >= bottom - 1e-3 {
                new_y[j] -= gap;
            }
        }
        removed += gap;
    }

    // 3. Container height.
    // Each element keeps the distance from its bottom to the band's bottom, as in
    // JasperReports: what stretches pushes the bottom down by its own growth, whatever else
    // sits lower in the band.
    let kept = (0..elements.len())
        .filter(|&i| measured[i].visible)
        .map(|i| new_y[i] + measured[i].new_h + (design_h - (ey[i] + elements[i].height) - collapsed_below[i]).max(0.0))
        .fold(0.0f32, f32::max);
    // The design's height, less what blank elements took away.
    design_h = (design_h - removed).max(0.0);
    let height = design_h.max(kept).max(min_height);
    let growth = height - design_h;

    // 4. Emit primitives, cutting at `avail` when the content does not fit.
    let limit = match avail {
        Some(a) if height > a + 1e-3 => Some(a),
        _ => None,
    };
    let mut out = BandLayout { height: limit.unwrap_or(height), printed_values, returns, ..Default::default() };
    let mut rest: Vec<Element> = Vec::new();
    let mut reprint: Vec<Element> = Vec::new();
    for (i, el) in elements.iter().enumerate() {
        let m = &mut measured[i];
        if !m.visible {
            // A blank element that removes its line still does where the band continues.
            if el.remove_line_when_blank && m.blank && limit.is_some_and(|a| new_y[i] >= a - 1e-3) {
                let mut blank = freeze(el, None);
                blank.print_when = Some(Expr::Lit(Value::Bool(false)));
                rest.push(blank);
            }
            continue;
        }
        let mut h = m.new_h;
        let mut y = new_y[i];
        match el.stretch_type {
            StretchType::RelativeToBandHeight | StretchType::ContainerHeight | StretchType::RelativeToTallestObject => {
                h = h.max(el.height + growth)
            }
            StretchType::ContainerBottom => h = height - y,
            StretchType::NoStretch => {}
        }
        // An icon label that does not stretch with its container is printed as tall as it
        // asked to be, even when that is less than its designed height: JasperReports gives
        // the component the height it prepared (JRFillComponentElement.setStretchHeight), not
        // the element's, unless the container stretches it further.
        if let (Some((_, p)), StretchType::NoStretch) = (&m.icon_label, el.stretch_type) {
            h = p.height;
        }
        if el.position_type == PositionType::FixRelativeToBottom {
            y += growth;
        }
        let (x, w) = (el.x, el.width);
        let mut frame_split: Option<(BandLayout, BoxStyle)> = None;
        if let Some(a) = limit {
            if y >= a - 1e-3 {
                // Entirely below the cut: continues on the next page.
                rest.push(frozen(le.fonts, el, m.text.take()));
                continue;
            }
            // Whether the element continues on the next page (a frame's children that did not
            // fit, the rest of a text): it is not printed again in full there.
            let continued = rest.len();
            if y + h > a + 1e-3 {
                match &el.kind {
                    // A text whose own height does not fit moves whole to the next page, as in
                    // JasperReports; only one that fits and then stretches past the cut splits.
                    ElementKind::Text(_) if y + el.height > a + 1e-3 => {
                        rest.push(frozen(le.fonts, el, m.text.take()));
                        continue;
                    }
                    ElementKind::Text(te) if te.stretch && m.text.is_some() => {
                        let text = m.text.take().unwrap_or_default();
                        let p = te.boxed.padding;
                        let (cw, room) = ((w - p.left - p.right).max(0.0), a - y - p.top - p.bottom);
                        // How many lines stay on this page, and what continues on the next.
                        let (k, cont) = match text_form(le.fonts, te, el.forecolor, &text) {
                            TextForm::Styled(st) if te.markup != crate::rich::Markup::None => {
                                // Styled text continues on the next page as styled markup.
                                let base = run_style(te, el.forecolor);
                                let (k, total, rest_at) = crate::rich::lines_that_fit_in(&st, le.fonts, te, w, room);
                                let raw = crate::rich::parse(&text, &base, te.markup);
                                let styled = |s: String| {
                                    let mut e = freeze(el, Some(s));
                                    if let ElementKind::Text(t) = &mut e.kind {
                                        t.markup = crate::rich::Markup::Styled;
                                    }
                                    e
                                };
                                let cont = match k {
                                    0 => Some(styled(raw.to_styled_markup(&base))),
                                    k if k < total => Some(styled(raw.tail(rest_at).to_styled_markup(&base))),
                                    _ => None,
                                };
                                (k, cont)
                            }
                            TextForm::Styled(st) => {
                                // Plain text in more than one font continues as plain text.
                                let (k, total, rest_at) = crate::rich::lines_that_fit_in(&st, le.fonts, te, w, room);
                                let cont = match k {
                                    0 => Some(freeze(el, Some(text.clone()))),
                                    k if k < total => Some(freeze(el, Some(st.text[rest_at..].to_string()))),
                                    _ => None,
                                };
                                (k, cont)
                            }
                            TextForm::Plain(te) => {
                                let hyphenator = le.fonts.hyphenator();
                                let face = le.fonts.resolve(&te.style.font);
                                let f = le.fonts.face(face);
                                let lines = break_lines_hyphenated(&text, f, te.style.font.size, cw, Measure::Fill, hyphenator);
                                let vm = vertical_metrics(f, te.style.font.size, te.style.line_spacing);
                                let k = vm.lines_that_fit(room);
                                let cont = match k {
                                    0 => Some(freeze(el, Some(text.clone()))),
                                    k if k < lines.len() => Some(freeze(el, Some(text_after_lines(&text, &lines, k)))),
                                    _ => None,
                                };
                                (k, cont)
                            }
                        };
                        if let Some(c) = &cont {
                            carry_paragraph(le.fonts, el, c);
                        }
                        rest.extend(cont);
                        if k == 0 {
                            continue;
                        }
                        m.text = Some(text);
                        h = a - y;
                    }
                    // An icon label is not split: one whose designed height does not fit moves
                    // whole to the next page (JRFillComponentElement.prepare); the rest ends at
                    // the cut.
                    ElementKind::Frame { children, .. } if crate::iconlabel::of(children).is_some() => {
                        if y + el.height > a + 1e-3 {
                            rest.push(frozen(le.fonts, el, None));
                            continue;
                        }
                        h = a - y;
                    }
                    ElementKind::Frame { children, boxed } => {
                        let inner = layout_elements(le, children, el.height, Some(a - y))?;
                        h = a - y;
                        if let Some(mut pending) = inner.rest.clone() {
                            // The continuation has no top border, the printed part no bottom one.
                            // Jasper moves the pending children to the top and keeps the
                            // frame's declared height.
                            move_to_top(&mut pending);
                            let mut cont: Element = (*el).clone();
                            cont.print_when = None;
                            let mut cont_box = *boxed;
                            cont_box.top.width = 0.0;
                            cont.kind = ElementKind::Frame { children: pending, boxed: cont_box };
                            rest.push(cont);
                        }
                        let mut part_box = *boxed;
                        part_box.bottom.width = 0.0;
                        frame_split = Some((inner, part_box));
                    }
                    ElementKind::Crosstab(_) | ElementKind::CrosstabRest(_) => {
                        // A crosstab breaks between its rows; the column headers print again
                        // where it continues.
                        let Some((rest_at, _)) = m.crosstab.take() else { continue };
                        let room = a - y;
                        let (chunks, _, next) = rest_at.filled.plan(rest_at.part, rest_at.row, room);
                        if chunks.is_empty() {
                            let mut cont = freeze(el, None);
                            cont.kind = ElementKind::CrosstabRest(rest_at);
                            rest.push(cont);
                            continue;
                        }
                        if let Some((part, row)) = next {
                            let mut cont = freeze(el, None);
                            cont.kind =
                                ElementKind::CrosstabRest(crate::crosstab::Rest { filled: rest_at.filled.clone(), part, row });
                            rest.push(cont);
                        }
                        m.crosstab = Some((rest_at, Some(room)));
                        h = a - y;
                    }
                    ElementKind::Subreport(_) | ElementKind::Table(_) | ElementKind::List(_) | ElementKind::Content { .. } => {
                        let framed = is_framed(&el.kind);
                        // Subreport content breaks between its bands; the page and column
                        // headers of the subreport are printed again where it continues.
                        let all = m.content.take().unwrap_or_default();
                        let footers: Vec<Segment> = all.iter().filter(|s| s.footer).cloned().collect();
                        let segments: Vec<Segment> = all.iter().filter(|s| !s.footer).cloned().collect();
                        let room = a - y;
                        let fits_whole = Segment::flow_height(&segments) <= room + 1e-3;
                        // When the content continues on the next page, its page footer prints on
                        // this page too, so leave room for it.
                        let footer_h = if fits_whole { 0.0 } else { footers.iter().map(|f| f.height).sum() };
                        let mut used = 0.0;
                        let mut k = 0;
                        while k < segments.len() && used + segments[k].height + footer_h <= room + 1e-3 {
                            used += segments[k].height;
                            k += 1;
                        }
                        let only_headers = segments[..k].iter().all(|s| s.header);
                        if k == 0 || (only_headers && k < segments.len()) {
                            let mut cont = freeze(el, None);
                            cont.kind = ElementKind::Content { segments: all, framed };
                            rest.push(cont);
                            continue;
                        }
                        let mut part = segments[..k].to_vec();
                        if k < segments.len() {
                            let mut tail: Vec<Segment> = segments.iter().filter(|s| s.header).cloned().collect();
                            tail.extend(segments[k..].iter().cloned());
                            tail.extend(footers.iter().cloned());
                            let mut cont = freeze(el, None);
                            cont.kind = ElementKind::Content { segments: Arc::new(tail), framed };
                            rest.push(cont);
                            // Jasper prints it right after the last band that fits.
                            part.extend(footers.iter().map(|f| Segment { footer: false, ..f.clone() }));
                        }
                        m.content = Some(Arc::new(part));
                        h = a - y;
                    }
                    // Elements stretching with their container end at the cut.
                    _ if el.stretch_type != StretchType::NoStretch => h = a - y,
                    _ => {
                        rest.push(frozen(le.fonts, el, m.text.take()));
                        continue;
                    }
                }
            }
            // printWhenDetailOverflows prints an element that ended on this page again where the
            // band continues; one that continues there is not printed twice (JasperReports
            // reprints only what is already printed whole: JRFillFrame, JRFillTextField.prepare).
            if el.print_when_detail_overflows && rest.len() == continued {
                reprint.push(frozen(le.fonts, el, m.text.clone()));
            }
        }
        if let Some(link) = &el.link {
            if let Some(info) = link_info(le, link, x, y, w, h)? {
                out.items.push(Laid { item: PrintItem::Link(Arc::new(info)), deferred: None });
            }
        }
        match &el.kind {
            ElementKind::Text(te) => {
                if el.mode == Mode::Opaque {
                    out.items.push(Laid {
                        item: PrintItem::Rect { x, y, w, h, fill: Some(el.backcolor), stroke: None, radius: 0.0, path: false },
                        deferred: None,
                    });
                }
                match m.text.take() {
                    Some(text) => {
                        let back = (el.mode == Mode::Opaque).then_some(el.backcolor);
                        let item = if le.images.skip_cells && te.field.is_none() {
                            text_item(le.fonts, te, el.forecolor, x, y, w, h, &text, true).0
                        } else {
                            let (item, printed) = text_item_with_cell(le.fonts, te, el.forecolor, x, y, w, h, &text);
                            let value = TextValue { text, number: m.number, pattern: m.pattern.take() };
                            tagged(cell_item(le.fonts, te, el.forecolor, back, x, y, w, h, &value, item, printed), el.tag)
                        };
                        out.items.push(Laid { item, deferred: None });
                    }
                    None => {
                        let (time, group) = match &te.content {
                            TextContent::Expression { evaluation_time, evaluation_group, .. } => {
                                (*evaluation_time, evaluation_group.clone())
                            }
                            _ => (EvaluationTime::Now, None),
                        };
                        let placeholder = PrintText {
                            face: le.fonts.resolve(&te.style.font),
                            size: te.style.font.size,
                            color: el.forecolor,
                            underline: false,
                            strike: false,
                            lines: Vec::new(),
                            standard: None,
                            halign: te.style.halign,
                        };
                        out.items.push(Laid {
                            item: PrintItem::Text(placeholder),
                            deferred: Some(DeferredText {
                                what: Deferred::Text(Box::new(te.clone())),
                                forecolor: el.forecolor,
                                back: (el.mode == Mode::Opaque).then_some(el.backcolor),
                                x,
                                y,
                                w,
                                h,
                                time,
                                group,
                                auto: None,
                                tag: el.tag,
                            }),
                        });
                    }
                }
                box_items(&te.boxed, x, y, w, h, &mut out.items);
            }
            ElementKind::Line { direction, pen } => {
                let mark = open_mark(
                    &mut out.items,
                    el,
                    Shape::Line { pen: *pen, top_down: *direction == LineDirection::TopDown },
                    None,
                    x,
                    y,
                    w,
                    h,
                );
                // Jasper draws 1pt-thin lines along the element's center edge.
                let item = if w <= 1.0 && h > 1.0 {
                    PrintItem::Line { x1: x + 0.5, y1: y, x2: x + 0.5, y2: y + h, pen: *pen }
                } else if h <= 1.0 {
                    PrintItem::Line { x1: x, y1: y + 0.5, x2: x + w, y2: y + 0.5, pen: *pen }
                } else if *direction == LineDirection::TopDown {
                    PrintItem::Line { x1: x, y1: y, x2: x + w, y2: y + h, pen: *pen }
                } else {
                    PrintItem::Line { x1: x, y1: y + h, x2: x + w, y2: y, pen: *pen }
                };
                if pen.width > 0.0 {
                    out.items.push(Laid { item, deferred: None });
                }
                close_mark(&mut out.items, mark);
            }
            ElementKind::Rectangle { radius, pen } => {
                let stroke = (pen.width > 0.0).then_some(*pen);
                let mark = open_mark(&mut out.items, el, Shape::Rectangle(stroke), None, x, y, w, h);
                out.items.push(Laid {
                    item: PrintItem::Rect {
                        x,
                        y,
                        w,
                        h,
                        fill: (el.mode == Mode::Opaque).then_some(el.backcolor),
                        stroke,
                        radius: *radius,
                        path: true,
                    },
                    deferred: None,
                });
                close_mark(&mut out.items, mark);
            }
            ElementKind::Ellipse { pen } => {
                let stroke = (pen.width > 0.0).then_some(*pen);
                let mark = open_mark(&mut out.items, el, Shape::Ellipse(stroke), None, x, y, w, h);
                out.items.push(Laid {
                    item: PrintItem::Ellipse { x, y, w, h, fill: (el.mode == Mode::Opaque).then_some(el.backcolor), stroke },
                    deferred: None,
                });
                close_mark(&mut out.items, mark);
            }
            ElementKind::Frame { boxed, children } => {
                // An icon label printed taller than it asked to be (stretched with its container)
                // or cut places its fields again for the height it has: JasperReports fills the
                // component after giving it its final height.
                if let (Some((settings, prepared)), Some((_, label, icon))) = (&m.icon_label, crate::iconlabel::of(children)) {
                    if (h - prepared.height).abs() > 1e-3 {
                        let fields = icon_label_children(*settings, prepared, boxed, label, icon, w, h);
                        m.children = Some(layout_elements(le, &fields, h, None)?);
                    }
                }
                let frame_box = frame_split.as_ref().map(|f| f.1).unwrap_or(*boxed);
                let mark = open_mark(&mut out.items, el, Shape::Frame, Some(frame_box), x, y, w, h);
                if el.mode == Mode::Opaque {
                    out.items.push(Laid {
                        item: PrintItem::Rect { x, y, w, h, fill: Some(el.backcolor), stroke: None, radius: 0.0, path: false },
                        deferred: None,
                    });
                }
                let (inner, frame_box) = match frame_split.take() {
                    Some((inner, b)) => (Some(inner), b),
                    None => (m.children.take(), *boxed),
                };
                if let Some(inner) = inner {
                    out.has_break |= inner.has_break;
                    out.has_column_break |= inner.has_column_break;
                    for mut laid in inner.items {
                        translate(&mut laid.item, x, y);
                        if let Some(d) = &mut laid.deferred {
                            d.x += x;
                            d.y += y;
                        }
                        out.items.push(laid);
                    }
                }
                box_items(&frame_box, x, y, w, h, &mut out.items);
                close_mark(&mut out.items, mark);
            }
            ElementKind::Image(img) => {
                let (x, w) = m.image_box.map_or((x, w), |(dx, iw)| (x + dx, iw));
                let shape = Shape::Image { data: m.image.clone(), scale: img.scale, halign: img.halign, valign: img.valign };
                let mark = open_mark(&mut out.items, el, shape, Some(img.boxed), x, y, w, h);
                // The image's tooltip is what it shows, in words.
                if let Some(e) = &img.tooltip {
                    let said = eval(e, le.scope, le.env)?;
                    if let (false, PrintItem::Element(info)) = (matches!(said, Value::Null), &mut out.items[mark].item) {
                        Arc::make_mut(info).alt = Some(said.to_java_string());
                    }
                }
                if el.mode == Mode::Opaque {
                    out.items.push(Laid {
                        item: PrintItem::Rect { x, y, w, h, fill: Some(el.backcolor), stroke: None, radius: 0.0, path: false },
                        deferred: None,
                    });
                }
                if let Some(data) = m.image.take() {
                    out.items.push(Laid { item: place_image(img, data, x, y, w, h), deferred: None });
                }
                box_items(&img.boxed, x, y, w, h, &mut out.items);
                close_mark(&mut out.items, mark);
            }
            ElementKind::Subreport(_) | ElementKind::Table(_) | ElementKind::List(_) | ElementKind::Content { .. } => {
                // JasperReports puts a subreport's elements on the page itself; tables and lists
                // with content are printed in a frame as tall as the content.
                let content = m.content.take();
                let printed = || content.iter().flat_map(|c| c.iter()).filter(|s| !s.footer);
                let content_h: f32 = printed().map(|s| s.height).sum();
                let has_items = printed().any(|s| !s.items.is_empty());
                let mark = (is_framed(&el.kind) && has_items)
                    .then(|| open_mark(&mut out.items, el, Shape::Frame, None, x, y, w, content_h));
                let mut top = y;
                for seg in printed() {
                    for item in &seg.items {
                        let mut item = item.clone();
                        translate(&mut item, x, top);
                        out.items.push(Laid { item, deferred: None });
                    }
                    top += seg.height;
                }
                if let Some(mark) = mark {
                    close_mark(&mut out.items, mark);
                }
            }
            ElementKind::Chart(ch) => {
                let mark = open_mark(&mut out.items, el, Shape::Picture { symbol: None }, Some(ch.boxed), x, y, w, h);
                if ch.evaluation_time == EvaluationTime::Now {
                    let items = match &ch.dataset.run {
                        Some(run) => chart_over_run(ch, run, le.scope, le.env, le.fonts, le.images, x, y, w, h)?,
                        None => {
                            let texts = chart_texts(ch, le.scope, le.env)?;
                            crate::chart::render(ch, le.charts, &texts, le.fonts, le.env.locale, x, y, w, h)
                        }
                    };
                    out.items.push(Laid { item: PrintItem::Group(items), deferred: None });
                } else {
                    out.items.push(Laid {
                        item: PrintItem::Group(Vec::new()),
                        deferred: Some(DeferredText {
                            what: Deferred::Chart(ch.clone()),
                            forecolor: el.forecolor,
                            back: None,
                            x,
                            y,
                            w,
                            h,
                            time: ch.evaluation_time,
                            group: ch.evaluation_group.clone(),
                            auto: None,
                            tag: None,
                        }),
                    });
                }
                box_items(&ch.boxed, x, y, w, h, &mut out.items);
                close_mark(&mut out.items, mark);
            }
            ElementKind::Barcode(bc) => {
                // The size of the barcode's picture in points (barcode4j works in mm).
                let unit = match bc.symbology {
                    Symbology::Qr { .. } | Symbology::Barbecue { .. } => 1.0,
                    _ => 72.0 / 25.4,
                };
                let symbol = m.symbol.as_ref().map(|s| (s.width * unit, s.height * unit));
                let mark = open_mark(&mut out.items, el, Shape::Picture { symbol }, Some(bc.boxed), x, y, w, h);
                // JasperReports draws no background behind a Barbecue barcode (which paints its
                // own white).
                if el.mode == Mode::Opaque && !matches!(bc.symbology, Symbology::Barbecue { .. }) {
                    out.items.push(Laid {
                        item: PrintItem::Rect { x, y, w, h, fill: Some(el.backcolor), stroke: None, radius: 0.0, path: false },
                        deferred: None,
                    });
                }
                if let Some(sym) = m.symbol.take() {
                    place_symbol(le.fonts, bc, el, sym, x, y, &mut out.items);
                }
                box_items(&bc.boxed, x, y, w, h, &mut out.items);
                close_mark(&mut out.items, mark);
            }
            ElementKind::Crosstab(_) | ElementKind::CrosstabRest(_) => {
                if let Some((at, room)) = m.crosstab.take() {
                    render_crosstab(le, &at, room.unwrap_or(f32::INFINITY), el, x, y, &mut out.items)?;
                }
            }
            ElementKind::Break => out.has_break = true,
            ElementKind::ColumnBreak => out.has_column_break = true,
            ElementKind::Unsupported(_) => {}
        }
    }
    if limit.is_some() {
        if !rest.is_empty() {
            out.rest_reprints = !reprint.is_empty();
            rest.extend(reprint);
        }
        out.rest = Some(rest);
    }
    Ok(out)
}

/// Evaluates a barcode element's code and lays out its symbol (`None` for a null code).
fn barcode_symbol(bc: &BarcodeElement, el: &Element, le: &mut LayoutEnv) -> Result<Option<crate::barcode::Symbol>> {
    let code = match &bc.expr {
        Some(e) => eval(e, le.scope, le.env)?,
        None => return Ok(None),
    };
    if code.is_null() {
        return Ok(None);
    }
    let code = code.to_java_string();
    let p = bc.boxed.padding;
    let symbol = match &bc.symbology {
        Symbology::Qr { level, margin, version } => {
            // Jasper works on whole points here.
            let w = el.width as i32 - (p.left + p.right) as i32;
            let h = el.height as i32 - (p.top + p.bottom) as i32;
            crate::barcode::qr_symbol(&code, *level, *margin, *version, w, h)
        }
        Symbology::Code128 { module_width, text_position, quiet_zone, vertical_quiet_zone } => crate::barcode::code128_symbol(
            &code,
            *module_width,
            el.height as f64,
            *text_position,
            bc.font_size.map(|s| s as f64),
            *quiet_zone,
            *vertical_quiet_zone,
        ),
        Symbology::Linear(spec) => crate::barcode::linear::symbol(spec, &code, el.height as f64, bc.font_size.map(|s| s as f64)),
        Symbology::Ean128 { module_width, text_position, quiet_zone, vertical_quiet_zone, checksum } => {
            crate::barcode::ean128_message(&code, *checksum).and_then(|(data, human)| {
                crate::barcode::code128_layout(
                    &data,
                    Some(&human),
                    *module_width,
                    el.height as f64,
                    *text_position,
                    bc.font_size.map(|s| s as f64),
                    *quiet_zone,
                    *vertical_quiet_zone,
                )
            })
        }
        Symbology::Pdf417 { spec, text_position } => {
            crate::barcode::pdf417_symbol(&code, spec, el.height as f64, *text_position, bc.font_size.map(|s| s as f64))
        }
        Symbology::DataMatrix { module_width, shape } => crate::barcode::datamatrix_symbol(&code, *module_width, *shape),
        Symbology::Barbecue { spec, application_identifier, rotation } => {
            if !crate::barcode::barbecue::TYPES.contains(&spec.kind.as_str()) {
                return Ok(None);
            }
            let mut spec = spec.clone();
            if let Some(e) = application_identifier {
                let ai = eval(e, le.scope, le.env)?;
                spec.application_identifier = (!ai.is_null()).then(|| ai.to_java_string());
            }
            crate::barcode::barbecue::symbol(&spec, &code).map(|s| rotate_symbol(s, *rotation))
        }
        Symbology::Postnet { module_width, short_bar, gap, add_checksum } => {
            let mm = crate::barcode::pt2mm;
            let bar = module_width.map(mm).unwrap_or(0.508);
            crate::barcode::postal::postnet(
                &code,
                *add_checksum,
                bar,
                gap.map(mm).unwrap_or(bar),
                short_bar.map(mm).unwrap_or(1.27),
                el.height as f64,
            )
        }
        Symbology::FourState { usps, module_width, ascender, track, gap, checksum } => {
            let mm = crate::barcode::pt2mm;
            let g = if *usps {
                crate::barcode::postal::FourState {
                    bar: module_width.map(mm).unwrap_or(0.508),
                    gap: gap.map(mm).unwrap_or(0.635),
                    track: track.map(mm).unwrap_or(1.27),
                    ascender: ascender.map(mm).unwrap_or(1.27),
                    quiet: 3.175,
                    vquiet: 0.7112,
                    fit_height: ascender.is_none() && track.is_none(),
                }
            } else {
                crate::barcode::postal::FourState {
                    bar: module_width.map(mm).unwrap_or(0.53),
                    gap: gap.map(mm).unwrap_or(0.53),
                    track: track.map(mm).unwrap_or(1.25),
                    ascender: ascender.map(mm).unwrap_or(1.8),
                    quiet: 2.0,
                    vquiet: 2.0,
                    fit_height: ascender.is_none() && track.is_none(),
                }
            };
            if *usps {
                crate::barcode::postal::usps(&code, &g, el.height as f64)
            } else {
                crate::barcode::postal::royal_mail(
                    &code,
                    *checksum != crate::barcode::linear::Checksum::Ignore,
                    &g,
                    el.height as f64,
                )
            }
        }
    };
    symbol.map(Some).map_err(|e| Error::eval(format!("barcode `{code}`: {e}")))
}

/// A symbol turned a quarter left or right, or upside down.
fn rotate_symbol(s: crate::barcode::Symbol, rotation: Rotation) -> crate::barcode::Symbol {
    let (w, h) = (s.width, s.height);
    let turn = |[x, y, rw, rh]: [f64; 4]| match rotation {
        Rotation::None => [x, y, rw, rh],
        Rotation::Left => [y, w - x - rw, rh, rw],
        Rotation::Right => [h - y - rh, x, rh, rw],
        Rotation::UpsideDown => [w - x - rw, h - y - rh, rw, rh],
    };
    let (width, height) = match rotation {
        Rotation::Left | Rotation::Right => (h, w),
        _ => (w, h),
    };
    crate::barcode::Symbol { width, height, rects: s.rects.into_iter().map(turn).collect(), texts: s.texts }
}

/// Scales a symbol into its box like Jasper scales the SVG image (RetainShape, top-left).
fn place_symbol(
    fonts: &mut FontSet,
    bc: &BarcodeElement,
    el: &Element,
    sym: crate::barcode::Symbol,
    x: f32,
    y: f32,
    out: &mut Vec<Laid>,
) {
    let p = bc.boxed.padding;
    let (bw, bh) = ((el.width - p.left - p.right).max(0.0) as f64, (el.height - p.top - p.bottom).max(0.0) as f64);
    if sym.width <= 0.0 || sym.height <= 0.0 {
        return;
    }
    let scale = (bw / sym.width).min(bh / sym.height);
    let (ox, oy) = (x + p.left, y + p.top);
    // QR codes take the element's color; barcode4j draws black.
    let color = match bc.symbology {
        Symbology::Qr { .. } => el.forecolor,
        _ => Color::BLACK,
    };
    // Human-readable text: JasperReports draws the SVG text as glyph outlines, centered on
    // its anchor with fractional advances.
    let mut path = Vec::new();
    for t in &sym.texts {
        let spec = FontSpec {
            name: bc.font_name.clone().unwrap_or_else(|| "Helvetica".into()),
            size: t.size as f32,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            pdf_font: None,
        };
        let id = fonts.resolve(&spec);
        let face = fonts.face(id);
        let upem = face.units_per_em as f64;
        let advances: Vec<f64> = t
            .text
            .chars()
            .map(|c| if c.is_control() { 0.0 } else { face.advance(face.glyph(c)) as f64 * t.size / upem })
            .collect();
        // SVG textLength with lengthAdjust="spacing", as Batik does it: glyph positions are
        // scaled about the first one so that the ink of the last glyph ends at the given length.
        let n = advances.len();
        let (start, scale) = match t.length {
            Some(len) if n > 1 => {
                let natural: f64 = advances.iter().sum();
                let last = t.text.chars().last().unwrap_or(' ');
                let last_w = face.glyph_x_max(last) * t.size / upem;
                (t.x - len / 2.0, (len - last_w) / (natural - advances[n - 1]))
            }
            Some(len) => (t.x - len / 2.0, 1.0),
            None => (t.x - advances.iter().sum::<f64>() / 2.0, 1.0),
        };
        let mut prefix = 0.0;
        for (c, adv) in t.text.chars().zip(&advances) {
            face.glyph_outline(c, t.size, start + prefix * scale, t.y, &mut path);
            prefix += adv;
        }
    }
    let (mut oy, mut scale) = (oy, scale);
    if let Symbology::Barbecue { .. } = bc.symbology {
        // JasperReports draws the barcode into a form scaled by a factor iText writes with two
        // decimals (five below 1), from the form's bottom edge.
        let digits = if scale >= 1.0 { 100.0 } else { 100_000.0 };
        let written = (scale * digits).round() / digits;
        oy += (sym.height * (scale - written)) as f32;
        scale = written;
        // Barbecue paints its spaces and quiet zones white.
        out.push(Laid {
            item: PrintItem::Rect {
                x: ox,
                y: oy,
                w: (sym.width * scale) as f32,
                h: (sym.height * scale) as f32,
                fill: Some(Color::WHITE),
                stroke: None,
                radius: 0.0,
                path: false,
            },
            deferred: None,
        });
    }
    let (rects, path) = match bc.symbology {
        Symbology::Qr { .. } | Symbology::Barbecue { .. } => (sym.rects, path),
        _ => snap_like_jasper(sym.rects, path, sym.height),
    };
    out.push(Laid {
        item: PrintItem::Shapes { x: ox, y: oy, scale, color, rects: Arc::new(rects), path: Arc::new(path) },
        deferred: None,
    });
}

/// JasperReports draws barcode4j's SVG into a PDF form whose units are 96-dpi pixels, and iText
/// writes each coordinate there with 2 decimals. Snapping our millimetre geometry to the same grid
/// puts every bar edge exactly where Jasper's lands, so they anti-alias the same way.
fn snap_like_jasper(rects: Vec<[f64; 4]>, path: Vec<PathOp>, height_mm: f64) -> (Vec<[f64; 4]>, Vec<PathOp>) {
    const PX_PER_MM: f64 = 96.0 / 25.4;
    let h = height_mm * PX_PER_MM;
    let round2 = |v: f64| (v * 100.0).round() / 100.0;
    let sx = |x: f64| round2(x * PX_PER_MM) / PX_PER_MM;
    // The form's y axis points up from its bottom edge.
    let sy = |y: f64| (h - round2(h - y * PX_PER_MM)) / PX_PER_MM;
    let rects = rects
        .into_iter()
        .map(|[x, y, w, hh]| {
            let (x0, x1, y0, y1) = (sx(x), sx(x + w), sy(y), sy(y + hh));
            [x0, y0, x1 - x0, y1 - y0]
        })
        .collect();
    let path = path
        .into_iter()
        .map(|op| match op {
            PathOp::Move(x, y) => PathOp::Move(sx(x), sy(y)),
            PathOp::Line(x, y) => PathOp::Line(sx(x), sy(y)),
            PathOp::Quad(a, b, x, y) => PathOp::Quad(sx(a), sy(b), sx(x), sy(y)),
            PathOp::Cubic(a, b, c, d, x, y) => PathOp::Cubic(sx(a), sy(b), sx(c), sy(d), sx(x), sy(y)),
            PathOp::Close => PathOp::Close,
        })
        .collect();
    (rects, path)
}

/// Subreports nested deeper than this are reported as an error (a template including itself).
const MAX_SUBREPORT_DEPTH: usize = 8;

/// Most subreports, tables and lists filled in one render: enough for a subreport on each of
/// hundreds of thousands of rows, while a template that nests ten copies of itself per level
/// stops instead of running for hours.
const MAX_SUBREPORT_RUNS: usize = 500_000;

fn count_run(le: &mut LayoutEnv, what: &str) -> Result<()> {
    check_time(le.images)?;
    le.images.runs += 1;
    if le.images.runs > MAX_SUBREPORT_RUNS {
        return Err(Error::Unsupported(format!(
            "{what}: more than {MAX_SUBREPORT_RUNS} subreports, tables and lists filled in one report"
        )));
    }
    Ok(())
}

/// A subreport return value to apply: (variable, calculation, value).
type Returned = (String, Calculation, Value);

/// Fills a subreport element: its content as segments, and its return values.
fn run_subreport(le: &mut LayoutEnv, sr: &SubreportElement) -> Result<(Vec<Segment>, Vec<Returned>)> {
    let location = match &sr.expr {
        Some(e) => eval(e, le.scope, le.env)?,
        None => return Ok((Vec::new(), Vec::new())),
    };
    let location = match location {
        Value::Str(s) => s,
        Value::Null => return Ok((Vec::new(), Vec::new())),
        other => return Err(Error::eval(format!("subreport expression returned {}", other.type_name()))),
    };
    if le.images.depth >= MAX_SUBREPORT_DEPTH {
        return Err(Error::Unsupported(format!("subreport `{location}`: nested more than {MAX_SUBREPORT_DEPTH} levels")));
    }
    let report = le.images.load_report(&location)?;

    let mut params = HashMap::new();
    if let Some(e) = &sr.parameters_map {
        if let Value::Json(map) = eval(e, le.scope, le.env)? {
            if let Some(obj) = map.as_object() {
                for (k, v) in obj {
                    let class = report.parameters.iter().find(|p| &p.name == k).map(|p| p.class).unwrap_or(JavaType::Object);
                    params.insert(k.clone(), crate::data::json_to_value(v, class)?);
                }
            }
        }
    }
    for (name, e) in &sr.parameters {
        params.insert(name.clone(), eval(e, le.scope, le.env)?);
    }
    params.entry("REPORT_LOCALE".into()).or_insert_with(|| le.scope.param("REPORT_LOCALE").unwrap_or_default());
    inherit_hyphenation(&mut params, &report, le.scope);

    let connection = le.images.connection.clone();
    let records = match (&sr.data_source, sr.connection, connection, &report.query) {
        (None, true, Some(conn), Some(_)) => {
            let env = EvalEnv { locale: le.env.locale, groovy: report.language == Language::Groovy, now: le.env.now };
            query_records(&report, &params, &conn, &env, rows_left(le.images))?
        }
        _ => {
            let rows = match &sr.data_source {
                Some(e) => match eval(e, le.scope, le.env)? {
                    Value::Json(j) => j,
                    Value::Null => Arc::new(serde_json::Value::Array(Vec::new())),
                    other => {
                        return Err(Error::eval(format!("subreport data source is a {}, expected a list", other.type_name())))
                    }
                },
                None => Arc::new(serde_json::Value::Array(Vec::new())),
            };
            crate::data::records_from_json(&report, &rows)?
        }
    };
    let env = EvalEnv { locale: le.env.locale, groovy: report.language == Language::Groovy, now: le.env.now };

    count_run(le, &location)?;
    le.images.depth += 1;
    let filled = fill_flow(&report, &records, params, le.fonts, le.images, &env);
    le.images.depth -= 1;
    let (segments, vars) = filled?;
    let returns =
        sr.returns.iter().map(|r| (r.to.clone(), r.calculation, vars.get(&r.from).cloned().unwrap_or(Value::Null))).collect();
    Ok((segments, returns))
}

// ---------------------------------------------------------------------------------------------
// Crosstabs

/// Evaluates bucket and measure expressions on one row of a crosstab's data.
struct RecordScope<'a> {
    record: &'a Record,
    fields: &'a [Field],
    params: Option<&'a HashMap<String, Value>>,
    outer: &'a dyn Scope,
}

impl Scope for RecordScope<'_> {
    fn field(&self, name: &str) -> Result<Value> {
        if !self.fields.iter().any(|f| f.name == name) {
            return Err(Error::eval(format!("field `{name}` is not declared")));
        }
        Ok(self.record.get(name).cloned().unwrap_or(Value::Null))
    }
    fn param(&self, name: &str) -> Result<Value> {
        match self.params.and_then(|p| p.get(name)) {
            Some(v) => Ok(v.clone()),
            None => self.outer.param(name),
        }
    }
    fn variable(&self, name: &str) -> Result<Value> {
        Err(Error::eval(format!("variable `{name}` cannot be used in crosstab buckets or measures")))
    }
}

/// Evaluates the contents of a crosstab cell or header: measures of the cell, bucket values of
/// its groups, and the crosstab's parameters.
struct CellScope<'a> {
    filled: &'a crate::crosstab::Filled,
    row: &'a [Value],
    column: &'a [Value],
    outer: &'a dyn Scope,
    /// `ROW_COUNT` and `COLUMN_COUNT` of the cell: how many detail rows (columns) of the grid
    /// come up to its first one, totals not counted (`JRFillCrosstab.computeCounts`). `None`
    /// where JasperReports sets them to null: the header cell, row headers for the columns,
    /// column headers for the rows, and order-by expressions.
    counts: (Option<usize>, Option<usize>),
}

impl<'a> CellScope<'a> {
    fn new(filled: &'a crate::crosstab::Filled, row: &'a [Value], column: &'a [Value], outer: &'a dyn Scope) -> Self {
        CellScope { filled, row, column, outer, counts: (None, None) }
    }
}

impl Scope for CellScope<'_> {
    fn field(&self, name: &str) -> Result<Value> {
        Err(Error::eval(format!("field `{name}` cannot be used in a crosstab cell; use a measure")))
    }
    fn param(&self, name: &str) -> Result<Value> {
        match self.filled.params.get(name) {
            Some(v) => Ok(v.clone()),
            None => self.outer.param(name),
        }
    }
    fn variable(&self, name: &str) -> Result<Value> {
        let ct = &self.filled.crosstab;
        if let Some(i) = ct.row_groups.iter().position(|g| g.name == name) {
            return Ok(self.row.get(i).cloned().unwrap_or(Value::Null));
        }
        if let Some(j) = ct.column_groups.iter().position(|g| g.name == name) {
            return Ok(self.column.get(j).cloned().unwrap_or(Value::Null));
        }
        // `measure`, `measure_rowGroup_ALL`, `measure_columnGroup_ALL`, `measure_row_column_ALL`.
        for (k, m) in ct.measures.iter().enumerate() {
            let (mut row, mut column) = (self.row, self.column);
            let rest = match name.strip_prefix(m.name.as_str()) {
                Some("") => "",
                Some(r) => match r.strip_prefix('_').and_then(|r| r.strip_suffix("_ALL")) {
                    Some(groups) => groups,
                    None => continue,
                },
                None => continue,
            };
            if !rest.is_empty() {
                let mut ok = false;
                for (i, g) in ct.row_groups.iter().enumerate() {
                    if rest == g.name || rest.starts_with(&format!("{}_", g.name)) {
                        row = &row[..i.min(row.len())];
                        ok = rest == g.name;
                        if !ok {
                            let col_name = &rest[g.name.len() + 1..];
                            if let Some(j) = ct.column_groups.iter().position(|c| c.name == col_name) {
                                column = &column[..j.min(column.len())];
                                ok = true;
                            }
                        }
                        break;
                    }
                }
                if !ok {
                    match ct.column_groups.iter().position(|c| c.name == rest) {
                        Some(j) => column = &column[..j.min(column.len())],
                        None => continue,
                    }
                }
            }
            let value = self.filled.cell_values(row, column).get(k).cloned().unwrap_or(Value::Null);
            if m.percentage {
                let total = self.filled.cell_values(&[], &[]).get(k).cloned().unwrap_or(Value::Null);
                return Ok(percentage(&value, &total, m.class));
            }
            return Ok(value);
        }
        // The crosstab's built-in variables (`JRDesignCrosstab.BUILT_IN_VARIABLES`).
        let count = |n: Option<usize>| n.map_or(Value::Null, |n| Value::Int(n as i32));
        let even = |n: Option<usize>| n.map_or(Value::Null, |n| Value::Bool(n % 2 == 0));
        match name {
            "ROW_COUNT" => Ok(count(self.counts.0)),
            "IS_EVEN_ROW" => Ok(even(self.counts.0)),
            "COLUMN_COUNT" => Ok(count(self.counts.1)),
            "IS_EVEN_COLUMN" => Ok(even(self.counts.1)),
            _ => Err(Error::eval(format!("variable `{name}` is not a measure or group of the crosstab"))),
        }
    }
}

/// `value` as a percentage of `total` (`JRPercentageCalculatorFactory`): `100 * value / total`
/// in the measure's class (whole numbers divide as integers, BigDecimal rounds half up to the
/// value's scale), and zero when the total is zero or missing.
fn percentage(value: &Value, total: &Value, class: JavaType) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if total.as_f64().is_none_or(|t| t == 0.0) {
        return class.zero();
    }
    match class {
        JavaType::BigDecimal => {
            let (Some(v), Some(t)) = (value.as_decimal(), total.as_decimal()) else { return class.zero() };
            let scaled = v.saturating_mul(rust_decimal::Decimal::from(100));
            let q = scaled.checked_div(t).unwrap_or_default();
            Value::Decimal(q.round_dp_with_strategy(scaled.scale(), rust_decimal::RoundingStrategy::MidpointAwayFromZero))
        }
        JavaType::Integer | JavaType::Short | JavaType::Byte | JavaType::Long | JavaType::BigInteger => {
            let (v, t) = (value.as_i64().unwrap_or(0), total.as_i64().unwrap_or(1).max(1));
            class.coerce(Value::Long(v.wrapping_mul(100) / t))
        }
        _ => class.coerce(Value::Double(100.0 * value.as_f64().unwrap_or(0.0) / total.as_f64().unwrap_or(1.0))),
    }
}

/// Reads a crosstab's rows, buckets them and aggregates its measures.
fn fill_crosstab(le: &mut LayoutEnv, ct: &CrosstabElement, el: &Element) -> Result<Arc<crate::crosstab::Filled>> {
    use crate::crosstab::{key, leaves, sizes, split_parts, Filled};
    let mut params = HashMap::new();
    for (name, e) in &ct.parameters {
        params.insert(name.clone(), eval(e, le.scope, le.env)?);
    }
    // The rows: a subdataset run's, or the report's own.
    let mut own_records: Vec<Row> = Vec::new();
    let mut run_params = HashMap::new();
    let mut dataset_fields = Vec::new();
    let sub = ct.run.is_some();
    if let Some(run) = &ct.run {
        let list = ListElement {
            run: run.clone(),
            contents: Band { height: 0.0, split_type: SplitType::Stretch, print_when: None, elements: Vec::new() },
            across: None,
        };
        let report = crate::table::list_report(&list, el.width);
        for (name, e) in &run.parameters {
            run_params.insert(name.clone(), eval(e, le.scope, le.env)?);
        }
        let connection = le.images.connection.clone();
        let records = match (&run.data_source, run.connection, connection, &report.query) {
            (None, true, Some(conn), Some(_)) => query_records(&report, &run_params, &conn, le.env, rows_left(le.images))?,
            _ => {
                let rows = match &run.data_source {
                    Some(e) => match eval(e, le.scope, le.env)? {
                        Value::Json(j) => j,
                        Value::Null => Arc::new(serde_json::Value::Array(Vec::new())),
                        other => {
                            return Err(Error::eval(format!("crosstab data source is a {}, expected a list", other.type_name())))
                        }
                    },
                    None => Arc::new(serde_json::Value::Array(Vec::new())),
                };
                crate::data::records_from_json(&report, &rows)?
            }
        };
        // Read here rather than by a fill of their own: counted here.
        count_rows(le.images, records.len())?;
        own_records = records.into_iter().map(Row::Owned).collect();
        dataset_fields = report.fields.clone();
    }
    let (records, fields): (&[Row], &[Field]) =
        if sub { (&own_records, &dataset_fields) } else { (le.records, &le.report.fields) };

    let defs: Vec<VarDef> = ct
        .measures
        .iter()
        .map(|m| VarDef {
            name: m.name.clone(),
            class: m.class,
            calculation: m.calculation,
            reset: ResetType::Report,
            increment: ResetType::None,
            expression: m.expr.clone(),
            initial: None,
        })
        .collect();
    let (n, mm) = (ct.row_groups.len(), ct.column_groups.len());
    let mut row_paths = Vec::new();
    let mut column_paths = Vec::new();
    let mut seen_rows = HashSet::new();
    let mut seen_columns = HashSet::new();
    let mut aggregates: HashMap<String, Vec<VarState>> = HashMap::new();
    for record in records {
        let scope = RecordScope { record, fields, params: if sub { Some(&run_params) } else { None }, outer: le.scope };
        let bucket = |g: &CrosstabGroup| -> Result<Value> {
            Ok(match &g.bucket {
                Some(e) => g.class.coerce(eval(e, &scope, le.env)?),
                None => Value::Null,
            })
        };
        let row = ct.row_groups.iter().map(bucket).collect::<Result<Vec<_>>>()?;
        let column = ct.column_groups.iter().map(bucket).collect::<Result<Vec<_>>>()?;
        let values = ct
            .measures
            .iter()
            .map(|m| match &m.expr {
                Some(e) => eval(e, &scope, le.env),
                None => Ok(Value::Null),
            })
            .collect::<Result<Vec<_>>>()?;
        if seen_rows.insert(key(&row)) {
            row_paths.push(row.clone());
        }
        if seen_columns.insert(key(&column)) {
            column_paths.push(column.clone());
        }
        for k in 0..=n {
            for l in 0..=mm {
                let cell = format!("{}|{}", key(&row[..k]), key(&column[..l]));
                let states = aggregates.entry(cell).or_insert_with(|| vec![VarState::default(); defs.len()]);
                for (i, v) in values.iter().enumerate() {
                    update_var(&mut states[i], &defs[i], v.clone());
                }
            }
        }
    }
    let totals = |gs: &[CrosstabGroup]| gs.iter().map(|g| g.total_position).collect::<Vec<_>>();
    let order = |gs: &[CrosstabGroup]| gs.iter().map(|g| g.descending).collect::<Vec<_>>();
    // Like JasperReports, totals add up the values of the buckets below them in print order
    // (not the rows again), which decides how floating-point sums round.
    {
        use crate::crosstab::{prefixes, sorted_paths};
        let natural = HashMap::new();
        let rows_sorted = sorted_paths(&row_paths, &order(&ct.row_groups), &natural);
        let columns_sorted = sorted_paths(&column_paths, &order(&ct.column_groups), &natural);
        let cell = |r: &[Value], c: &[Value]| format!("{}|{}", key(r), key(c));
        let merge_into = |target: String, parts: Vec<String>, aggregates: &mut HashMap<String, Vec<VarState>>| {
            let merged: Vec<Option<VarState>> = (0..defs.len())
                .map(|i| {
                    let states: Vec<&VarState> = parts.iter().filter_map(|p| aggregates.get(p)).map(|s| &s[i]).collect();
                    merge_states(&defs[i], &states)
                })
                .collect();
            if let Some(t) = aggregates.get_mut(&target) {
                for (i, m) in merged.into_iter().enumerate() {
                    if let Some(m) = m {
                        t[i] = m;
                    }
                }
            }
        };
        // Column totals of every detail row, deepest first.
        for r in &rows_sorted {
            for l in (0..mm).rev() {
                for p in prefixes(&columns_sorted, l) {
                    let children: Vec<String> = prefixes(&columns_sorted, l + 1)
                        .into_iter()
                        .filter(|c| key(&c[..l]) == key(&p))
                        .map(|c| cell(r, &c))
                        .collect();
                    merge_into(cell(r, &p), children, &mut aggregates);
                }
            }
        }
        // Then row totals of every column and column total.
        for l in 0..=mm {
            for p in prefixes(&columns_sorted, l) {
                for k in (0..n).rev() {
                    for q in prefixes(&rows_sorted, k) {
                        let children: Vec<String> = prefixes(&rows_sorted, k + 1)
                            .into_iter()
                            .filter(|c| key(&c[..k]) == key(&q))
                            .map(|c| cell(&c, &p))
                            .collect();
                        merge_into(cell(&q, &p), children, &mut aggregates);
                    }
                }
            }
        }
    }
    // A cell without data shows zero for sums and counts, nothing for the rest.
    let empty: Vec<Value> = ct
        .measures
        .iter()
        .map(|m| match m.calculation {
            Calculation::Sum | Calculation::Count | Calculation::DistinctCount => m.class.zero(),
            _ => Value::Null,
        })
        .collect();
    let values: HashMap<String, Vec<Value>> =
        aggregates.into_iter().map(|(k, states)| (k, states.into_iter().map(|s| s.value).collect())).collect();
    let mut filled = Filled {
        crosstab: ct.clone(),
        rows: Vec::new(),
        columns: Vec::new(),
        row_heights: Vec::new(),
        column_widths: Vec::new(),
        values,
        empty,
        params,
        parts: Vec::new(),
    };
    // `orderByExpression`: each bucket's sort value, computed on its totals.
    let mut row_order = HashMap::new();
    let mut column_order = HashMap::new();
    for (row_axis, groups, paths) in [(true, &ct.row_groups, &row_paths), (false, &ct.column_groups, &column_paths)] {
        for (i, g) in groups.iter().enumerate() {
            let Some(e) = &g.order_by else { continue };
            let sorted = crate::crosstab::sorted_paths(paths, &order(groups), &HashMap::new());
            for prefix in crate::crosstab::prefixes(&sorted, i + 1) {
                let none: &[Value] = &[];
                let (row, column) = if row_axis { (&prefix[..], none) } else { (none, &prefix[..]) };
                let v = eval(e, &CellScope::new(&filled, row, column, le.scope), le.env)?;
                if row_axis {
                    row_order.insert(key(&prefix), v);
                } else {
                    column_order.insert(key(&prefix), v);
                }
            }
        }
    }
    if !records.is_empty() {
        filled.rows = leaves(&row_paths, &totals(&ct.row_groups), &order(&ct.row_groups), &row_order);
        filled.columns = leaves(&column_paths, &totals(&ct.column_groups), &order(&ct.column_groups), &column_order);
    }
    let (widths, heights) = sizes(ct);
    filled.column_widths = filled.columns.iter().map(|c| widths[c.level]).collect();
    filled.row_heights = filled.rows.iter().map(|r| heights[r.level]).collect();
    let header_w: f32 = ct.row_groups.iter().map(|g| g.size).sum();
    filled.parts = split_parts(&filled.column_widths, header_w, ct.repeat_row_headers, el.width);
    Ok(Arc::new(filled))
}

/// Combines the aggregates of several buckets in order, the way JasperReports computes crosstab
/// totals. `None` for calculations that are kept as computed from the rows.
fn merge_states(def: &VarDef, states: &[&VarState]) -> Option<VarState> {
    let class = def.class;
    let mut out = VarState { initialized: states.iter().any(|s| s.initialized), ..VarState::default() };
    match def.calculation {
        Calculation::Sum => {
            for s in states {
                if let Some(v) = &s.sum {
                    out.sum = Some(match &out.sum {
                        Some(t) => add_values(t, v, class),
                        None => v.clone(),
                    });
                }
            }
            out.value = out.sum.clone().unwrap_or(Value::Null);
        }
        Calculation::Count => {
            out.count = states.iter().map(|s| s.count).sum();
            out.value = class.coerce(Value::Long(out.count));
            if matches!(class, JavaType::Object | JavaType::Number) {
                out.value = Value::Int(out.count as i32);
            }
        }
        Calculation::Average => {
            for s in states {
                out.count += s.count;
                if let Some(v) = &s.sum {
                    out.sum = Some(match &out.sum {
                        Some(t) => add_values(t, v, class),
                        None => v.clone(),
                    });
                }
            }
            out.value = match &out.sum {
                Some(s) if out.count > 0 => match class {
                    JavaType::BigDecimal => crate::value::java_decimal_divide(
                        s.as_decimal().unwrap_or_default(),
                        rust_decimal::Decimal::from(out.count),
                        16,
                    )
                    .map(Value::Decimal)
                    .unwrap_or(Value::Null),
                    _ => class.coerce(Value::Double(s.as_f64().unwrap_or(0.0) / out.count as f64)),
                },
                _ => Value::Null,
            };
        }
        Calculation::Highest | Calculation::Lowest => {
            let want =
                if def.calculation == Calculation::Highest { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Less };
            for s in states {
                if !s.value.is_null() && (out.value.is_null() || s.value.compare(&out.value) == Some(want)) {
                    out.value = s.value.clone();
                }
            }
        }
        Calculation::DistinctCount => {
            for s in states {
                out.distinct.extend(s.distinct.iter().cloned());
            }
            out.value = class.coerce(Value::Long(out.distinct.len() as i64));
        }
        _ => return None,
    }
    Some(out)
}

/// Prints the pieces of a crosstab that fit in `room`, at `(x, y)`: one frame per piece with
/// a frame per cell, as JasperReports prints them.
fn render_crosstab(
    le: &mut LayoutEnv,
    at: &crate::crosstab::Rest,
    room: f32,
    el: &Element,
    x: f32,
    y: f32,
    out: &mut Vec<Laid>,
) -> Result<()> {
    let f = &*at.filled;
    let (chunks, _, _) = f.plan(at.part, at.row, room);
    let mut outer = el.clone();
    outer.mode = Mode::Transparent;
    outer.tag = outer.tag.or(Some(Tag::Table));
    let none: &[Value] = &[];
    let (row_counts, column_counts) = (f.detail_counts(true), f.detail_counts(false));
    for ch in chunks {
        let top = y + ch.y;
        let hh = if ch.with_headers { f.header_height() } else { 0.0 };
        let rows_h: f32 = ch.rows.clone().map(|r| f.row_heights[r]).sum();
        let hw = f.headers_width(ch.part);
        let mark = open_mark(out, &outer, Shape::Frame, None, x, top, f.part_width(ch.part), hh + rows_h);
        if ch.with_headers {
            if let (Some(c), true) = (&f.crosstab.header_cell, hw > 0.0 && hh > 0.0) {
                place_cell(le, c, [x, top, hw, hh], (0.0, 0.0), &CellScope::new(f, none, none, le.scope), out, Tag::HeaderCell)?;
            }
            for (first, hc) in f.column_headers_from(ch.part) {
                let Some(c) = hc.contents else { continue };
                let g = &f.crosstab.column_groups[hc.group];
                let dx = if hc.total { 0.0 } else { header_offset(g.header_position, hc.w, f.design_width(hc.group)) };
                let scope =
                    CellScope { counts: (None, Some(column_counts[first])), ..CellScope::new(f, none, &hc.values, le.scope) };
                place_cell(le, c, [x + hc.x, top + hc.y, hc.w, hc.h], (dx, 0.0), &scope, out, Tag::HeaderCell)?;
            }
        }
        let mut ry = top + hh;
        for r in ch.rows.clone() {
            let row = &f.rows[r];
            if hw > 0.0 {
                for hc in f.row_headers(r, &ch.rows, ry) {
                    let Some(c) = hc.contents else { continue };
                    let g = &f.crosstab.row_groups[hc.group];
                    let dy = if hc.total { 0.0 } else { header_offset(g.header_position, hc.h, f.design_height(hc.group)) };
                    let scope =
                        CellScope { counts: (Some(row_counts[r]), None), ..CellScope::new(f, &hc.values, none, le.scope) };
                    place_cell(le, c, [x + hc.x, hc.y, hc.w, hc.h], (0.0, dy), &scope, out, Tag::RowHeaderCell)?;
                }
            }
            let mut cx = x + hw;
            for c in f.parts[ch.part].clone() {
                let column = &f.columns[c];
                let w = f.column_widths[c];
                if let Some(contents) = f.cell_contents(row, column) {
                    let scope = CellScope {
                        counts: (Some(row_counts[r]), Some(column_counts[c])),
                        ..CellScope::new(f, &row.values, &column.values, le.scope)
                    };
                    place_cell(le, contents, [cx, ry, w, f.row_heights[r]], (0.0, 0.0), &scope, out, Tag::DataCell)?;
                }
                cx += w;
            }
            ry += f.row_heights[r];
        }
        close_mark(out, mark);
    }
    Ok(())
}

/// How far a group header's contents move inside the cells they span (`headerPosition`).
fn header_offset(position: HeaderPosition, span: f32, extent: f32) -> f32 {
    match position {
        HeaderPosition::Middle => ((span - extent) / 2.0).max(0.0).floor(),
        HeaderPosition::End => (span - extent).max(0.0),
        _ => 0.0,
    }
}

/// Lays out a cell's contents frame at `[x, y, w, h]`, its elements moved by `shift`.
#[allow(clippy::too_many_arguments)]
fn place_cell(
    le: &mut LayoutEnv,
    contents: &Element,
    [x, y, w, h]: [f32; 4],
    shift: (f32, f32),
    scope: &dyn Scope,
    out: &mut Vec<Laid>,
    tag: Tag,
) -> Result<()> {
    let mut frame = contents.clone();
    frame.tag = frame.tag.or(Some(tag));
    frame.x = x;
    frame.y = y;
    frame.width = w;
    frame.height = h;
    if let ElementKind::Frame { children, .. } = &mut frame.kind {
        for c in children.iter_mut() {
            c.x += shift.0;
            c.y += shift.1;
        }
    }
    let mut cell_le = LayoutEnv {
        fonts: &mut *le.fonts,
        images: &mut *le.images,
        repeated: le.repeated,
        first_on_page: le.first_on_page,
        changed_groups: le.changed_groups,
        env: le.env,
        scope,
        charts: le.charts,
        report: le.report,
        records: le.records,
        min_height: 0.0,
    };
    let layout = layout_elements(&mut cell_le, std::slice::from_ref(&frame), y + h, None)?;
    out.extend(layout.items);
    Ok(())
}

/// Fills a table component: the report Jasper builds from the printed columns, run like a
/// subreport over the rows of the table's data source.
fn run_table(le: &mut LayoutEnv, t: &TableElement, width: f32) -> Result<Vec<Segment>> {
    let (scope, env) = (le.scope, le.env);
    let columns = crate::table::fill_columns(&t.columns, &mut |e| Ok(matches!(eval(e, scope, env)?, Value::Bool(true))))?;
    if columns.is_empty() {
        return Ok(Vec::new());
    }
    let report = crate::table::report(t, &columns, width);
    run_dataset_report(le, &t.run, &report, "table")
}

/// Fills a report built for a table or list over the rows of its dataset run.
fn run_dataset_report(le: &mut LayoutEnv, run: &DatasetRun, report: &Report, what: &str) -> Result<Vec<Segment>> {
    if le.images.depth >= MAX_SUBREPORT_DEPTH {
        return Err(Error::Unsupported(format!("{what}: nested more than {MAX_SUBREPORT_DEPTH} levels")));
    }
    let (params, records) = run_inputs(run, report, le.scope, le.env, what, le.images)?;
    let env = EvalEnv { locale: le.env.locale, groovy: report.language == Language::Groovy, now: le.env.now };
    count_run(le, what)?;
    le.images.depth += 1;
    let filled = fill_flow(report, &records, params, le.fonts, le.images, &env);
    le.images.depth -= 1;
    Ok(filled?.0)
}

/// The parameters and rows of a dataset run, evaluated where the run is.
fn run_inputs(
    run: &DatasetRun,
    report: &Report,
    scope: &dyn Scope,
    env: &EvalEnv,
    what: &str,
    images: &ImageStore,
) -> Result<(HashMap<String, Value>, Vec<Record>)> {
    let mut params = HashMap::new();
    if let Some(e) = &run.parameters_map {
        if let Value::Json(map) = eval(e, scope, env)? {
            if let Some(obj) = map.as_object() {
                for (k, v) in obj {
                    let class = report.parameters.iter().find(|p| &p.name == k).map(|p| p.class).unwrap_or(JavaType::Object);
                    params.insert(k.clone(), crate::data::json_to_value(v, class)?);
                }
            }
        }
    }
    for (name, e) in &run.parameters {
        params.insert(name.clone(), eval(e, scope, env)?);
    }
    params.entry("REPORT_LOCALE".into()).or_insert_with(|| scope.param("REPORT_LOCALE").unwrap_or_default());
    inherit_hyphenation(&mut params, report, scope);
    if let Ok(bundle) = scope.param(crate::expr::eval::RESOURCE_BUNDLE) {
        params.entry(crate::expr::eval::RESOURCE_BUNDLE.into()).or_insert(bundle);
    }
    if let (None, true, Some(conn), Some(_)) = (&run.data_source, run.connection, &images.connection, &report.query) {
        let records = query_records(report, &params, conn, env, rows_left(images))?;
        return Ok((params, records));
    }
    let rows = match &run.data_source {
        Some(e) => match eval(e, scope, env)? {
            Value::Json(j) => j,
            Value::Null => Arc::new(serde_json::Value::Array(Vec::new())),
            other => return Err(Error::eval(format!("{what} data source is a {}, expected a list", other.type_name()))),
        },
        None => Arc::new(serde_json::Value::Array(Vec::new())),
    };
    let records = crate::data::records_from_json(report, &rows)?;
    Ok((params, records))
}

/// A subreport, table or list without a hyphenation setting of its own hyphenates as the
/// report it is in does, unless it is handed one.
fn inherit_hyphenation(params: &mut HashMap<String, Value>, report: &Report, outer: &dyn Scope) {
    let own = report.parameters.iter().any(|p| p.name == crate::text::HYPHENATE);
    if let (false, Ok(v)) = (own, outer.param(crate::text::HYPHENATE)) {
        if !v.is_null() {
            params.entry(crate::text::HYPHENATE.into()).or_insert(v);
        }
    }
}

/// The parameters a report fills with: those given, the built-in ones, then the defaults of
/// the others (which may use earlier ones), as JasperReports resolves them before running the
/// report's query.
pub(crate) fn resolve_params(
    report: &Report,
    mut params: HashMap<String, Value>,
    env: &EvalEnv,
) -> Result<HashMap<String, Value>> {
    for name in ["REPORT_PARAMETERS_MAP", "REPORT_CONNECTION", "REPORT_DATA_SOURCE", "REPORT_SCRIPTLET", "REPORT_MAX_COUNT"] {
        params.entry(name.to_string()).or_insert(Value::Null);
    }
    params.entry("REPORT_LOCALE".into()).or_insert_with(|| Value::str(env.locale.tag));
    params.entry("REPORT_TIME_ZONE".into()).or_insert_with(|| Value::str("UTC"));
    for p in &report.parameters {
        if let Some(v) = params.remove(&p.name) {
            params.insert(p.name.clone(), p.class.coerce(v));
            continue;
        }
        let v = match &p.default {
            Some(e) => {
                let empty = HashMap::new();
                let ctx = Ctx {
                    record: None,
                    report,
                    params: &params,
                    vars: &[],
                    var_index: &empty,
                    page_number: 1,
                    column_number: 1,
                    master: None,
                };
                eval(e, &ctx, env)?
            }
            None => Value::Null,
        };
        params.insert(p.name.clone(), v);
    }
    Ok(params)
}

/// The rows of a (sub)report's or dataset's query, run on the render's connection.
fn query_records(
    report: &Report,
    params: &HashMap<String, Value>,
    conn: &crate::sql::Shared,
    env: &EvalEnv,
    rows_left: Option<usize>,
) -> Result<Vec<Record>> {
    let params = resolve_params(report, params.clone(), env)?;
    let rows = crate::sql::query_rows(report, &params, conn.0.as_ref())?;
    match rows_left {
        Some(n) => rows.take(n).collect(),
        None => rows.collect(),
    }
}

// ---------------------------------------------------------------------------------------------
// Filler

/// Options that affect evaluation.
pub struct FillOptions<'a> {
    pub env: EvalEnv<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Values {
    /// Previous record, committed variables (group footers).
    Old,
    /// Current record, committed variables.
    Committed,
    /// Current record, variables including the current record.
    Estimated,
}

struct Pending {
    page: usize,
    item: usize,
    text: DeferredText,
}

struct Filler<'a> {
    report: &'a Report,
    rows: Rows<'a>,
    params: HashMap<String, Value>,
    fonts: &'a mut FontSet,
    images: &'a mut ImageStore,
    env: &'a EvalEnv<'a>,
    defs: Vec<VarDef>,
    var_index: HashMap<String, usize>,
    committed: Vec<VarState>,
    estimated: Option<Vec<VarState>>,
    pages: Vec<PrintPage>,
    /// Pages already handed to the sink (the index of `pages[0]` in the document).
    page_base: usize,
    sink: Option<&'a mut dyn PageSink>,
    /// A page is waiting for a value that cannot be added to it later: nothing more is
    /// handed to the sink until that value is known.
    held: bool,
    limits: Limits,
    page_number: i32,
    offset_y: f32,
    /// Something other than page/column headers was printed on the current page.
    page_has_content: bool,
    pending: Vec<Pending>,
    /// Elements evaluated at the end of the report ("page X of Y"), kept apart so the
    /// checks made on every page and group do not walk them.
    pending_final: Vec<Pending>,
    group_values: Vec<Option<Value>>,
    /// For variables with an `incrementType`: their state when their group (page, column)
    /// last ended. Each row is added to this, not to the running value, so the variable moves
    /// once per group: a count with `incrementType="Group"` counts groups.
    stepped: Vec<Option<VarState>>,
    groups_read_variables: bool,
    /// A band split across columns is being continued: its record has been counted.
    split_carry: bool,
    /// The values group headers are printed again with on a new page.
    reprint_mode: Values,
    /// With `printOrder="Horizontal"`: the row of columns being filled (its top, its height).
    row: Option<(f32, f32)>,
    /// The height of each row filled so far, or, given beforehand, the height each row of a
    /// horizontal list ends with; and which row is being filled.
    row_heights: Vec<f32>,
    row_index: usize,
    /// Whether the column/page footer of the current page has been printed already.
    footers_done: bool,
    /// Groups from this index on are starting: their headers must not be reprinted by a
    /// page break they cause.
    starting_group: Option<usize>,
    /// Last value printed by each element with isPrintRepeatedValues="false".
    repeated: HashMap<usize, String>,
    /// Groups that changed with the current record (until its first detail band prints).
    changed_groups: Vec<String>,
    /// Charts of the report and the data collected for each (by chart id).
    charts: Vec<&'a crate::chart::ChartElement>,
    chart_data: HashMap<usize, crate::chart::DataState>,
    /// Current column (multi-column reports) and where columns start on this page.
    column: u32,
    column_top: f32,
    /// The band being printed belongs to a column (detail, groups, column header/footer).
    in_column: bool,
    /// Subreport return values applied while printing the current record.
    returned: Vec<(usize, Calculation, Value)>,
    /// Subreport mode: one unbounded page, with the extent of every band recorded.
    flow: Option<Vec<FlowMark>>,
    /// The bands being printed are page or column headers.
    flow_header: bool,
}

/// One band printed in subreport mode: its position and its range of page items.
struct FlowMark {
    y: f32,
    height: f32,
    items: std::ops::Range<usize>,
    header: bool,
}

impl<'a> Filler<'a> {
    fn ctx_vars(&self, mode: Values) -> &[VarState] {
        match (mode, &self.estimated) {
            (Values::Estimated, Some(e)) => e,
            _ => &self.committed,
        }
    }

    fn eval_with(&self, e: &Expr, vars: &[VarState], record: Option<&Record>) -> Result<Value> {
        let ctx = Ctx {
            record,
            report: self.report,
            params: &self.params,
            vars,
            var_index: &self.var_index,
            page_number: self.page_number,
            column_number: self.column as i32 + 1,
            master: None,
        };
        eval(e, &ctx, self.env)
    }

    fn reset_var(&self, vars: &mut [VarState], i: usize) -> Result<()> {
        let initial = match &self.defs[i].initial {
            Some(e) => {
                let rec = self.rows.current();
                self.defs[i].class.coerce(self.eval_with(e, vars, rec)?)
            }
            None => Value::Null,
        };
        vars[i] = VarState { value: initial, ..Default::default() };
        Ok(())
    }

    /// Adds the current values to the datasets of charts whose increment matches.
    fn increment_charts(&mut self, pred: impl Fn(&crate::chart::Increment) -> bool, mode: Values) -> Result<()> {
        use crate::chart::DataKind;
        // Datasets take values from rows: with none read, there is nothing to add.
        if self.charts.is_empty() || self.rows.current().is_none() {
            return Ok(());
        }
        let record = self.rows.record(mode);
        let vars: Vec<VarState> = self.ctx_vars(mode).to_vec();
        let ctx = Ctx {
            record,
            report: self.report,
            params: &self.params,
            vars: &vars,
            var_index: &self.var_index,
            page_number: self.page_number,
            column_number: self.column as i32 + 1,
            master: None,
        };
        let env = self.env;
        let ev = |e: &Option<Expr>| -> Result<Value> {
            match e {
                Some(e) => eval(e, &ctx, env),
                None => Ok(Value::Null),
            }
        };
        for ch in self.charts.clone() {
            let ds = &ch.dataset;
            if !pred(&ds.increment) || ds.run.is_some() {
                continue;
            }
            if let Some(w) = &ds.increment_when {
                if !condition(&eval(w, &ctx, env)?, env)? {
                    continue;
                }
            }
            let date = |v: Value| match v {
                Value::Date(d) => Some(d),
                _ => None,
            };
            for s in &ds.series {
                let state = self.chart_data.entry(ch.id).or_default();
                match ds.kind {
                    DataKind::Pie => {
                        if let (k, Some(v)) = (ev(&s.key)?, ev(&s.value)?.as_f64()) {
                            if !k.is_null() {
                                state.add_pie(k.to_java_string(), v);
                            }
                        }
                    }
                    DataKind::Xy => {
                        if let (Some(x), Some(y)) = (ev(&s.x)?.as_f64(), ev(&s.y)?.as_f64()) {
                            state.add_xy(ev(&s.series)?.to_java_string(), x, y);
                        }
                    }
                    DataKind::TimeSeries => {
                        if let (Some(t), Some(v)) = (date(ev(&s.period)?), ev(&s.value)?.as_f64()) {
                            let (a, b) = ds.time_period.bounds(t);
                            state.add_period(ev(&s.series)?.to_java_string(), a, b, v, true);
                        }
                    }
                    DataKind::TimePeriod => {
                        if let (Some(a), Some(b), Some(v)) = (date(ev(&s.start)?), date(ev(&s.end)?), ev(&s.value)?.as_f64()) {
                            state.add_period(
                                ev(&s.series)?.to_java_string(),
                                crate::chart::millis(a),
                                crate::chart::millis(b),
                                v,
                                false,
                            );
                        }
                    }
                    DataKind::Xyz => {
                        if let (Some(x), Some(y), Some(z)) = (ev(&s.x)?.as_f64(), ev(&s.y)?.as_f64(), ev(&s.z)?.as_f64()) {
                            state.add_xyz(ev(&s.series)?.to_java_string(), x, y, z);
                        }
                    }
                    DataKind::HighLow => {
                        if let Some(t) = date(ev(&s.date)?) {
                            let num = |e: &Option<Expr>| -> Result<f64> { Ok(ev(e)?.as_f64().unwrap_or(f64::NAN)) };
                            let item = [
                                crate::chart::millis(t),
                                num(&s.high)?,
                                num(&s.low)?,
                                num(&s.open)?,
                                num(&s.close)?,
                                num(&s.volume)?,
                            ];
                            state.add_ohlc(ev(&s.series)?.to_java_string(), item);
                        }
                    }
                    DataKind::Value => {
                        if let Some(v) = ev(&s.value)?.as_f64() {
                            state.value = Some(v);
                        }
                    }
                    DataKind::Gantt => {
                        if let (Some(a), Some(b)) = (date(ev(&s.start)?), date(ev(&s.end)?)) {
                            let task = ev(&s.task)?;
                            if task.is_null() {
                                continue;
                            }
                            let sub = ev(&s.subtask)?;
                            let percent = ev(&s.percent)?.as_f64();
                            state.add_gantt(
                                ev(&s.series)?.to_java_string(),
                                task.to_java_string(),
                                !sub.is_null(),
                                crate::chart::millis(a),
                                crate::chart::millis(b),
                                percent,
                            );
                        }
                    }
                    DataKind::Category => {
                        let (series, category) = (ev(&s.series)?, ev(&s.category)?);
                        if let Some(v) = ev(&s.value)?.as_f64() {
                            if !category.is_null() {
                                state.add_category(series.to_java_string(), category.to_java_string(), v);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// A record already counted where its band began: the variables that just started again
    /// keep their new values instead of taking it in once more.
    fn carry_estimate(&mut self, pred: impl Fn(&ResetType) -> bool) {
        if let Some(est) = self.estimated.as_mut() {
            for (i, def) in self.defs.iter().enumerate() {
                if pred(&def.reset) {
                    est[i] = self.committed[i].clone();
                }
            }
        }
    }

    fn reset_where(&mut self, pred: impl Fn(&ResetType) -> bool) -> Result<()> {
        for ch in &self.charts {
            if pred(&ch.dataset.reset) {
                self.chart_data.remove(&ch.id);
            }
        }
        let mut vars = std::mem::take(&mut self.committed);
        for i in 0..self.defs.len() {
            let stepped = self.stepped[i].is_some();
            // What ended takes its last value in, before anything starts again.
            if stepped && pred(&self.defs[i].increment) {
                self.stepped[i] = Some(vars[i].clone());
            }
            if matches!(self.defs[i].reset, ResetType::None) {
                // Never reset: instead, whenever other variables start again (a group, a page,
                // the report), it takes the value of its expression for the row at hand.
                // That is why it can be printed in a group header, which comes before the
                // row is added to anything.
                if let Some(e) = &self.defs[i].expression {
                    let value = self.eval_with(e, &vars, self.rows.record(self.reprint_mode))?;
                    vars[i].value = self.defs[i].class.coerce(value);
                }
            } else if pred(&self.defs[i].reset) {
                self.reset_var(&mut vars, i)?;
                if stepped {
                    self.stepped[i] = Some(vars[i].clone());
                }
            }
        }
        self.committed = vars;
        Ok(())
    }

    /// Variables after adding the current record to `base`.
    fn calculate(&self, base: &[VarState]) -> Result<Vec<VarState>> {
        let mut vars = base.to_vec();
        let rec = self.rows.current();
        for (i, def) in self.defs.iter().enumerate() {
            let ev = match &def.expression {
                Some(e) => self.eval_with(e, &vars, rec)?,
                None => Value::Null,
            };
            if let Some(base) = &self.stepped[i] {
                vars[i] = base.clone();
            }
            update_var(&mut vars[i], def, ev);
        }
        Ok(vars)
    }

    fn layout(&mut self, band: &Band, mode: Values) -> Result<Option<BandLayout>> {
        self.layout_within(band, mode, None, 0.0)
    }

    /// Lays out (with `lay`) a band that may already have been laid out for the same row, to
    /// see whether it fits or to cut it: the rows its subreports, tables, lists, charts and
    /// crosstabs read count once toward the row limit, however many times it is laid out.
    /// `charged` is what this band has counted so far (0 before its first layout).
    fn once<T>(&mut self, charged: &mut usize, lay: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let base = self.images.rows_read.saturating_sub(*charged);
        self.images.rows_read = base;
        let laid = lay(self);
        *charged = (*charged).max(self.images.rows_read.saturating_sub(base));
        self.images.rows_read = base.saturating_add(*charged);
        laid
    }

    /// Lays out a band, cut at `avail` points when given (what does not fit is dropped),
    /// and at least `min_height` tall.
    fn layout_within(&mut self, band: &Band, mode: Values, avail: Option<f32>, min_height: f32) -> Result<Option<BandLayout>> {
        let record = self.rows.record(mode);
        let vars: Vec<VarState> = self.ctx_vars(mode).to_vec();
        let ctx = Ctx {
            record,
            report: self.report,
            params: &self.params,
            vars: &vars,
            var_index: &self.var_index,
            page_number: self.page_number,
            column_number: self.column as i32 + 1,
            master: None,
        };
        if let Some(pw) = &band.print_when {
            if !condition(&eval(pw, &ctx, self.env)?, self.env)? {
                return Ok(None);
            }
        }
        let mut le = LayoutEnv {
            fonts: self.fonts,
            images: self.images,
            repeated: &self.repeated,
            first_on_page: !self.page_has_content,
            changed_groups: &self.changed_groups,
            env: self.env,
            scope: &ctx,
            charts: &self.chart_data,
            report: self.report,
            records: self.rows.seen(),
            min_height,
        };
        let mut layout = layout_elements(&mut le, &band.elements, band.height, avail)?;
        self.plan_auto(&mut layout, record, &vars);
        Ok(Some(layout))
    }

    /// Lays out a list of elements (a band or the continuation of a split band).
    fn layout_elems(&mut self, elements: &[Element], design_h: f32, mode: Values, avail: Option<f32>) -> Result<BandLayout> {
        let record = self.rows.record(mode);
        let vars: Vec<VarState> = self.ctx_vars(mode).to_vec();
        let ctx = Ctx {
            record,
            report: self.report,
            params: &self.params,
            vars: &vars,
            var_index: &self.var_index,
            page_number: self.page_number,
            column_number: self.column as i32 + 1,
            master: None,
        };
        let mut le = LayoutEnv {
            fonts: self.fonts,
            images: self.images,
            repeated: &self.repeated,
            first_on_page: !self.page_has_content,
            changed_groups: &self.changed_groups,
            env: self.env,
            scope: &ctx,
            charts: &self.chart_data,
            report: self.report,
            records: self.rows.seen(),
            min_height: 0.0,
        };
        let mut layout = layout_elements(&mut le, elements, design_h, avail)?;
        self.plan_auto(&mut layout, record, &vars);
        Ok(layout)
    }

    fn plan_auto(&self, layout: &mut BandLayout, record: Option<&Record>, vars: &[VarState]) {
        for laid in &mut layout.items {
            if let Some(d) = laid.deferred.as_mut().filter(|d| d.time == EvaluationTime::Auto) {
                d.auto = Some(Box::new(self.auto_plan(d, record, vars)));
            }
        }
    }

    /// When each variable of an `evaluationTime="Auto"` element is to be read.
    fn auto_plan(&self, d: &DeferredText, record: Option<&Record>, vars: &[VarState]) -> Auto {
        let mut names = Vec::new();
        if let Deferred::Text(te) = &d.what {
            if let TextContent::Expression { expr, pattern_expr, .. } = &te.content {
                expr.iter().chain(pattern_expr.iter()).for_each(|e| e.variables(&mut names));
            }
        }
        let mut waits: Vec<(EvaluationTime, Option<String>, Vec<usize>)> = Vec::new();
        for name in names {
            let index = self.var_index.get(&*name).copied();
            let (time, group) = match index.map(|i| &self.defs[i].reset) {
                Some(ResetType::None) => continue,
                Some(ResetType::Page) => (EvaluationTime::Page, None),
                Some(ResetType::Column) => (EvaluationTime::Column, None),
                Some(ResetType::Group(g)) => (EvaluationTime::Group, Some(g.clone())),
                // The report's variables, and the page number, which is only final at the end.
                _ => (EvaluationTime::Report, None),
            };
            let at = match waits.iter().position(|w| w.0 == time && w.1 == group) {
                Some(at) => at,
                None => {
                    waits.push((time, group, Vec::new()));
                    waits.len() - 1
                }
            };
            waits[at].2.extend(index);
        }
        Auto { record: record.cloned(), vars: vars.to_vec(), waits }
    }

    fn emit(&mut self, layout: BandLayout, y: f32) {
        for (key, value) in &layout.printed_values {
            self.repeated.insert(*key, value.clone());
        }
        for (name, calculation, value) in layout.returns.iter().cloned() {
            let Some(&i) = self.var_index.get(&name) else { continue };
            self.apply_return(i, calculation, value.clone());
            if self.estimated.is_some() {
                self.returned.push((i, calculation, value));
            }
        }
        let r = self.report;
        let dx = r.margins.left + if self.in_column { self.column as f32 * (r.column_width + r.column_spacing) } else { 0.0 };
        let local = self.pages.len() - 1;
        let page = self.page_base + local;
        let first_item = self.pages[local].items.len();
        if let Some(marks) = &mut self.flow {
            marks.push(FlowMark {
                y,
                height: layout.height,
                items: first_item..first_item + layout.items.len(),
                header: self.flow_header,
            });
        }
        for mut laid in layout.items {
            translate(&mut laid.item, dx, y);
            let idx = self.pages[local].items.len();
            if let Some(mut d) = laid.deferred {
                d.x += dx;
                d.y += y;
                let list = if d.time == EvaluationTime::Report { &mut self.pending_final } else { &mut self.pending };
                list.push(Pending { page, item: idx, text: d });
            }
            self.pages[local].items.push(laid.item);
        }
    }

    /// Sets a variable from a subreport return value. The variables seen by the record being
    /// printed are updated; like JasperReports, other variables computed from it only see the
    /// new value from the next record on.
    fn apply_return(&mut self, i: usize, calculation: Calculation, value: Value) {
        let mut def = self.defs[i].clone();
        def.calculation = match calculation {
            Calculation::System => Calculation::Nothing,
            c => c,
        };
        let vars = match &mut self.estimated {
            Some(est) => est,
            None => &mut self.committed,
        };
        update_var(&mut vars[i], &def, value);
    }

    /// Hands the pages filled so far to the sink (called when a page is finished and at the
    /// end, `all`). A page still waiting for a value goes only if the sink can add to it
    /// later and nothing drawn after that value would end up underneath it.
    fn flush_pages(&mut self, all: bool) -> Result<()> {
        let Some(sink) = self.sink.as_deref_mut() else { return Ok(()) };
        if self.held && !all {
            return Ok(());
        }
        let mut n = 0;
        while n < self.pages.len() {
            let abs = self.page_base + n;
            let items = &self.pages[n].items;
            let mut waiting = [&self.pending, &self.pending_final]
                .into_iter()
                .flat_map(|list| list.iter().rev().take_while(move |p| p.page >= abs).filter(move |p| p.page == abs))
                .peekable();
            if waiting.peek().is_some() && !(sink.accepts_late() && waiting.all(|p| stands_alone(items, p))) {
                self.held = true;
                break;
            }
            n += 1;
        }
        for page in self.pages.drain(..n) {
            sink.page(page, self.fonts)?;
        }
        self.page_base += n;
        Ok(())
    }

    fn document(self) -> PrintDocument {
        PrintDocument {
            name: self.report.name.clone(),
            page_width: self.report.page_width,
            page_height: self.report.page_height,
            landscape: self.report.landscape,
            pdf: self.report.pdf.clone(),
            pages: self.pages,
        }
    }

    fn page_footer_band(&self, last: bool) -> Option<&'a Band> {
        if last {
            self.report.last_page_footer.as_ref().or(self.report.page_footer.as_ref())
        } else {
            self.report.page_footer.as_ref()
        }
    }

    /// Lowest y a body band may reach on the current page.
    fn body_limit(&self) -> f32 {
        if self.flow.is_some() {
            return f32::INFINITY;
        }
        let r = self.report;
        let pf = r.page_footer.as_ref().map(|b| b.height).unwrap_or(0.0);
        let cf = r.column_footer.as_ref().map(|b| b.height).unwrap_or(0.0);
        r.page_height - r.margins.bottom - pf - cf
    }

    fn new_page(&mut self) -> Result<()> {
        if self.flow.is_some() {
            // Subreport mode has a single page that grows with the content.
            if self.pages.is_empty() {
                self.pages.push(PrintPage::default());
                self.page_number = 1;
                self.offset_y = self.report.margins.top;
            }
            return Ok(());
        }
        self.flush_pages(false)?;
        self.limits.check_pages(self.page_base + self.pages.len() + 1)?;
        check_time(self.images)?;
        self.pages.push(PrintPage::default());
        self.page_number += 1;
        self.column = 0;
        self.offset_y = self.report.margins.top;
        self.page_has_content = false;
        self.footers_done = false;
        if self.page_number > 1 {
            self.reset_where(|r| matches!(r, ResetType::Page | ResetType::Column))?;
            if self.split_carry {
                self.carry_estimate(|r| matches!(r, ResetType::Page | ResetType::Column));
            } else if self.estimated.is_some() {
                // Re-estimate the record being moved to this page with page variables reset.
                self.estimated = Some(self.calculate(&self.committed.clone())?);
                for (i, calculation, value) in self.returned.clone() {
                    self.apply_return(i, calculation, value);
                }
            }
        }
        if let Some(bg) = &self.report.background {
            if let Some(l) = self.layout(bg, Values::Committed)? {
                let y = self.report.margins.top;
                self.emit(l, y);
            }
        }
        Ok(())
    }

    fn print_band_at_offset(&mut self, band: &Band, mode: Values) -> Result<bool> {
        if let Some(l) = self.layout(band, mode)? {
            let h = l.height;
            let brk = l.has_break;
            let y = self.offset_y;
            self.emit(l, y);
            self.offset_y += h;
            return Ok(brk);
        }
        Ok(false)
    }

    /// Prints the title. A title taller than what is left of the page, down to its bottom
    /// margin, goes on on the next pages, which get no page header or footer of their own
    /// (as in JasperReports).
    fn print_title(&mut self, band: &Band) -> Result<()> {
        let mut charged = 0;
        let Some(full) = self.once(&mut charged, |f| f.layout(band, Values::Committed))? else { return Ok(()) };
        let limit = self.report.page_height - self.report.margins.bottom;
        if self.flow.is_some() || full.height <= limit - self.offset_y + 1e-3 {
            let (y, h) = (self.offset_y, full.height);
            self.emit(full, y);
            self.offset_y += h;
            return Ok(());
        }
        let mut elements: Vec<Element> = band.elements.clone();
        let mut design_h = band.height;
        let mut first = Some(full);
        loop {
            let space = limit - self.offset_y;
            let part = self.once(&mut charged, |f| f.layout_elems(&elements, design_h, Values::Committed, Some(space)))?;
            if part.items.is_empty() {
                // Nothing fits even on a new page: print it whole rather than loop forever.
                let whole = match first.take() {
                    Some(l) => l,
                    None => self.once(&mut charged, |f| f.layout_elems(&elements, design_h, Values::Committed, None))?,
                };
                let (y, h) = (self.offset_y, whole.height);
                self.emit(whole, y);
                self.offset_y += h;
                return Ok(());
            }
            first = None;
            let rest = part.rest.clone().unwrap_or_default();
            let keep_top = part.rest_reprints;
            let (y, h) = (self.offset_y, part.height);
            self.emit(part, y);
            self.offset_y += h;
            self.page_has_content = true;
            if rest.is_empty() {
                return Ok(());
            }
            self.resolve_pending(|t| matches!(t.time, EvaluationTime::Page | EvaluationTime::Column), Values::Committed)?;
            self.new_page()?;
            elements = rest;
            let moved = if keep_top { 0.0 } else { move_to_top(&mut elements) };
            design_h = (design_h - moved).max(0.0);
        }
    }

    fn page_headers(&mut self) -> Result<()> {
        self.flow_header = true;
        if let Some(b) = &self.report.page_header {
            self.print_band_at_offset(b, Values::Committed)?;
        }
        self.column_top = self.offset_y;
        if self.report.horizontal {
            // Every column has its header at once: the rows run across them.
            let mut bottom = self.column_top;
            for c in 0..self.report.column_count.max(1) {
                self.column = c;
                self.offset_y = self.column_top;
                self.column_header()?;
                bottom = bottom.max(self.offset_y);
            }
            self.column = 0;
            self.offset_y = bottom;
        } else {
            self.column_header()?;
        }
        self.flow_header = false;
        if self.page_number > 1 {
            let limit = self.starting_group.unwrap_or(self.report.groups.len());
            for gi in 0..limit {
                if self.report.groups[gi].reprint_header_on_each_page && self.group_values[gi].is_some() {
                    for b in &self.report.groups[gi].header {
                        self.print_band_at_offset(b, self.reprint_mode)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn page_footers(&mut self, last: bool, mode: Values) -> Result<()> {
        if self.footers_done {
            return Ok(());
        }
        self.footers_done = true;
        self.increment_charts(|i| *i == crate::chart::Increment::Page, mode)?;
        let r = self.report;
        let footer = self.page_footer_band(last);
        if self.flow.is_some() {
            // Subreport mode: footers follow the content.
            for band in [r.column_footer.as_ref(), footer].into_iter().flatten() {
                self.print_band_at_offset(band, mode)?;
            }
            return self.resolve_pending(|t| matches!(t.time, EvaluationTime::Page | EvaluationTime::Column), mode);
        }
        let pf_h = footer.map(|b| b.height).unwrap_or(0.0);
        if r.horizontal {
            self.end_row();
            for c in 0..r.column_count.max(1) {
                self.column = c;
                self.column_footer(mode, pf_h)?;
            }
            self.column = 0;
        } else {
            self.column_footer(mode, pf_h)?;
        }
        if let Some(pf) = footer {
            if let Some(l) = self.layout(pf, mode)? {
                let y = r.page_height - r.margins.bottom - pf_h;
                self.emit(l, y);
            }
        }
        self.resolve_pending(|t| matches!(t.time, EvaluationTime::Page | EvaluationTime::Column), mode)?;
        Ok(())
    }

    fn column_header(&mut self) -> Result<()> {
        if let Some(b) = &self.report.column_header {
            self.in_column = true;
            let r = self.print_band_at_offset(b, Values::Committed);
            self.in_column = false;
            r?;
        }
        Ok(())
    }

    /// The column footer of the current column: at the bottom of the column, or right after
    /// its content with isFloatColumnFooter.
    fn column_footer(&mut self, mode: Values, page_footer_h: f32) -> Result<()> {
        let r = self.report;
        if let Some(cf) = &r.column_footer {
            let y =
                if r.float_column_footer { self.offset_y } else { r.page_height - r.margins.bottom - page_footer_h - cf.height };
            if let Some(l) = self.layout(cf, mode)? {
                self.in_column = true;
                self.emit(l, y);
                self.in_column = false;
            }
        }
        Ok(())
    }

    /// Moves on to the next column, or to the next page after the last column.
    fn column_break(&mut self, mode: Values) -> Result<()> {
        if self.flow.is_some() {
            return Ok(());
        }
        // Across the columns, a new column means a new page: the columns are one row.
        if self.column + 1 >= self.report.column_count.max(1) || self.report.horizontal {
            self.end_row();
            return self.page_break(mode);
        }
        let pf_h = self.page_footer_band(false).map(|b| b.height).unwrap_or(0.0);
        self.column_footer(mode, pf_h)?;
        self.column += 1;
        self.offset_y = self.column_top;
        self.page_has_content = false;
        self.reset_where(|r| matches!(r, ResetType::Column))?;
        if self.split_carry {
            self.carry_estimate(|r| matches!(r, ResetType::Column));
        } else if self.estimated.is_some() {
            self.estimated = Some(self.calculate(&self.committed.clone())?);
            for (i, calculation, value) in self.returned.clone() {
                self.apply_return(i, calculation, value);
            }
        }
        self.column_header()
    }

    /// With `printOrder="Horizontal"`: the detail bands of the current record, in the next
    /// column of the current row, or at the start of a new row. JasperReports does not let
    /// them grow there: what does not fit in a band's height is cut.
    fn print_detail_across(&mut self) -> Result<()> {
        let r = self.report;
        let cell_h: f32 = r.detail.iter().map(|b| b.height).sum();
        if self.row.is_none() {
            if self.page_has_content && self.offset_y + cell_h > self.body_limit() + 1e-3 {
                self.page_break(Values::Committed)?;
            }
            self.row = Some((self.offset_y, 0.0));
            self.column = 0;
        }
        let (top, height) = self.row.unwrap_or((self.offset_y, 0.0));
        let mut y = top;
        // In a report, JasperReports does not let a band in a row grow; in a list it does,
        // and every cell of a row is as tall as the tallest (see `fill_flow`).
        let (avail, min) = match self.flow {
            Some(_) => (None, self.row_heights.get(self.row_index).copied().filter(|_| r.detail.len() == 1).unwrap_or(0.0)),
            None => (Some(0.0), 0.0),
        };
        self.in_column = true;
        for band in &r.detail {
            let avail = avail.map(|_| band.height);
            if let Some(l) = self.layout_within(band, Values::Estimated, avail, min)? {
                let h = l.height;
                self.emit(l, y);
                y += h;
                self.page_has_content = true;
            }
        }
        self.in_column = false;
        self.row = Some((top, height.max(y - top)));
        self.column += 1;
        if self.column >= r.column_count.max(1) {
            self.end_row();
        }
        Ok(())
    }

    /// Ends the row of columns being filled, if any: what follows goes below it.
    fn end_row(&mut self) {
        if let Some((top, height)) = self.row.take() {
            self.offset_y = top + height;
            self.column = 0;
            if self.row_index >= self.row_heights.len() {
                self.row_heights.push(height);
            }
            self.row_index += 1;
        }
    }

    fn page_break(&mut self, mode: Values) -> Result<()> {
        if self.flow.is_some() {
            return Ok(());
        }
        self.row = None;
        self.page_footers(false, mode)?;
        // Group headers printed again on the new page show the groups being ended when the
        // break came in their footers.
        self.reprint_mode = if mode == Values::Old { Values::Old } else { Values::Committed };
        let started = self.new_page();
        if let Err(e) = started {
            self.reprint_mode = Values::Committed;
            return Err(e);
        }
        let printed = self.page_headers();
        self.reprint_mode = Values::Committed;
        printed
    }

    /// Prints a body band. When it does not fit, it either moves to the next page or, like
    /// JasperReports' default `splitType="Stretch"`, prints what fits and continues the rest
    /// on the following pages.
    fn print_body_band(&mut self, band: &Band, mode: Values) -> Result<()> {
        self.in_column = true;
        let result = self.print_body_band_in_column(band, mode);
        self.in_column = false;
        result
    }

    fn print_body_band_in_column(&mut self, band: &Band, mode: Values) -> Result<()> {
        let mut charged = 0;
        let Some(first) = self.once(&mut charged, |f| f.layout(band, mode))? else { return Ok(()) };
        let footer_mode = if mode == Values::Estimated { Values::Committed } else { mode };
        let mut elements: Vec<Element> = band.elements.clone();
        let mut design_h = band.height;
        let mut layout = Some(first);
        loop {
            let full = match layout.take() {
                Some(l) => l,
                None => self.once(&mut charged, |f| f.layout_elems(&elements, design_h, mode, None))?,
            };
            let space = self.body_limit() - self.offset_y;
            if full.height <= space + 1e-3 {
                let (y, h, brk, col_brk) = (self.offset_y, full.height, full.has_break, full.has_column_break);
                self.emit(full, y);
                self.offset_y += h;
                self.page_has_content = true;
                if brk {
                    self.in_column = false;
                    self.page_break(mode)?;
                } else if col_brk {
                    self.in_column = false;
                    self.column_break(mode)?;
                }
                self.in_column = true;
                return Ok(());
            }
            let may_split = !self.page_has_content
                || match band.split_type {
                    SplitType::Prevent => false,
                    SplitType::Stretch => design_h <= space + 1e-3,
                    SplitType::Immediate => true,
                };
            if !may_split {
                self.in_column = false;
                self.column_break(footer_mode)?;
                self.in_column = true;
                continue;
            }
            let part = self.once(&mut charged, |f| f.layout_elems(&elements, design_h, mode, Some(space)))?;
            let rest = part.rest.clone().unwrap_or_default();
            let keep_top = part.rest_reprints;
            if part.items.is_empty() && !self.page_has_content {
                // Nothing fits even on an empty page: print it whole rather than loop forever.
                let (y, h) = (self.offset_y, full.height);
                self.emit(full, y);
                self.offset_y += h;
                self.page_has_content = true;
                return Ok(());
            }
            let y = self.offset_y;
            self.emit(part, y);
            self.offset_y = self.body_limit();
            self.page_has_content = true;
            self.in_column = false;
            // Part of the band is on this page: its record counts in this page's footers,
            // and not again in the next page's or column's.
            self.split_carry = true;
            let moved = self.column_break(mode);
            self.split_carry = false;
            moved?;
            self.in_column = true;
            if rest.is_empty() {
                return Ok(());
            }
            elements = rest;
            let moved = if keep_top { 0.0 } else { move_to_top(&mut elements) };
            design_h = (design_h - moved).max(0.0);
        }
    }

    fn resolve_pending(&mut self, pred: impl Fn(&DeferredText) -> bool, mode: Values) -> Result<()> {
        // Elements with evaluationTime="Auto" take from this moment the variables that belong
        // to it, and are ready when they have them all.
        if self.pending.iter().any(|p| p.text.auto.is_some()) {
            let now = self.ctx_vars(mode).to_vec();
            for p in &mut self.pending {
                let Some(mut auto) = p.text.auto.take() else { continue };
                let mut still = Vec::new();
                for (time, group, indexes) in std::mem::take(&mut auto.waits) {
                    p.text.time = time;
                    p.text.group = group;
                    if pred(&p.text) {
                        for i in indexes {
                            auto.vars[i] = now[i].clone();
                        }
                    } else {
                        still.push((time, p.text.group.take(), indexes));
                    }
                }
                auto.waits = still;
                p.text.time = EvaluationTime::Auto;
                p.text.group = None;
                p.text.auto = Some(auto);
            }
        }
        let pred = |d: &DeferredText| match &d.auto {
            Some(auto) => auto.waits.is_empty(),
            None => pred(d),
        };
        let mut ready = Vec::new();
        if self.pending.iter().any(|p| pred(&p.text)) {
            let (hit, rest): (Vec<Pending>, Vec<Pending>) =
                std::mem::take(&mut self.pending).into_iter().partition(|p| pred(&p.text));
            self.pending = rest;
            ready = hit;
        }
        if self.pending_final.first().is_some_and(|p| pred(&p.text)) {
            ready.append(&mut self.pending_final);
        }
        if ready.is_empty() {
            return Ok(());
        }
        self.held = false;
        let record = self.rows.record(mode);
        let vars = self.ctx_vars(mode).to_vec();
        let total_pages = (self.page_base + self.pages.len()) as i32;
        for p in ready {
            let (record, vars) = match &p.text.auto {
                Some(auto) => (auto.record.as_ref(), &auto.vars),
                None => (record, &vars),
            };
            let ctx = Ctx {
                record,
                report: self.report,
                params: &self.params,
                vars,
                var_index: &self.var_index,
                page_number: self.page_number,
                column_number: self.column as i32 + 1,
                // A subreport's pages are not the master's: its master values stay null.
                master: (p.text.group.as_deref() == Some(MASTER_EVALUATION) && self.flow.is_none())
                    .then(|| (p.page as i32 + 1, total_pages)),
            };
            let d = &p.text;
            let item = match &d.what {
                Deferred::Text(te) => {
                    let value = text_value_full(&te.content, &ctx, self.env)?;
                    if self.images.skip_cells && te.field.is_none() {
                        text_item(self.fonts, te, d.forecolor, d.x, d.y, d.w, d.h, &value.text, true).0
                    } else {
                        let (item, printed) = text_item_with_cell(self.fonts, te, d.forecolor, d.x, d.y, d.w, d.h, &value.text);
                        tagged(cell_item(self.fonts, te, d.forecolor, d.back, d.x, d.y, d.w, d.h, &value, item, printed), d.tag)
                    }
                }
                Deferred::Chart(ch) => match &ch.dataset.run {
                    Some(run) => {
                        PrintItem::Group(chart_over_run(ch, run, &ctx, self.env, self.fonts, self.images, d.x, d.y, d.w, d.h)?)
                    }
                    None => {
                        let texts = chart_texts(ch, &ctx, self.env)?;
                        PrintItem::Group(crate::chart::render(
                            ch,
                            &self.chart_data,
                            &texts,
                            self.fonts,
                            self.env.locale,
                            d.x,
                            d.y,
                            d.w,
                            d.h,
                        ))
                    }
                },
            };
            if p.page >= self.page_base {
                self.pages[p.page - self.page_base].items[p.item] = item;
            } else if let Some(sink) = self.sink.as_deref_mut() {
                sink.late(p.page, item, self.fonts)?;
            }
        }
        Ok(())
    }

    fn group_footers(&mut self, from: usize, mode: Values) -> Result<()> {
        let r = self.report;
        for g in r.groups.iter().skip(from).rev() {
            self.increment_charts(|i| matches!(i, crate::chart::Increment::Group(n) if *n == g.name), mode)?;
            for b in &g.footer {
                self.print_body_band(b, mode)?;
            }
            let name = g.name.clone();
            self.resolve_pending(|t| t.time == EvaluationTime::Group && t.group.as_deref() == Some(name.as_str()), mode)?;
        }
        Ok(())
    }

    fn group_headers(&mut self, from: usize) -> Result<()> {
        let r = self.report;
        for (gi, g) in r.groups.iter().enumerate().skip(from) {
            self.starting_group = Some(gi);
            let header_h: f32 = g.header.iter().map(|b| b.height).sum();
            let remaining = self.body_limit() - self.offset_y;
            if self.page_has_content && g.start_new_page {
                self.page_break(Values::Committed)?;
            } else if self.page_has_content
                && (g.start_new_column
                    || (g.min_height_to_start_new_page > 0.0 && remaining < g.min_height_to_start_new_page)
                    || remaining < header_h.min(1.0))
            {
                self.column_break(Values::Committed)?;
            }
            for b in &g.header {
                self.print_body_band(b, Values::Committed)?;
            }
        }
        self.starting_group = None;
        Ok(())
    }

    /// Index of the outermost group whose value changes with the current record.
    fn group_rupture(&mut self) -> Result<Option<usize>> {
        if self.groups_read_variables {
            return self.group_rupture_with_variables();
        }
        let rec = self.rows.current();
        let mut first = None;
        let groups = &self.report.groups;
        for (gi, group) in groups.iter().enumerate() {
            let v = match &group.expression {
                Some(e) => self.eval_with(e, &self.committed, rec)?,
                None => Value::Null,
            };
            let changed = match &self.group_values[gi] {
                None => true,
                Some(prev) => !prev.java_equals(&v),
            };
            if changed && first.is_none() {
                first = Some(gi);
            }
            self.group_values[gi] = Some(v);
        }
        Ok(first)
    }

    /// The same question when a group expression reads variables, asked the way JasperReports
    /// asks it (`JRCalculator.estimateGroupRuptures`), because the answer depends on it:
    ///
    /// 1. each group on its own: does its expression say something else for this row, with
    ///    the variables as the row would leave them, than for the row before, with the
    ///    variables as that row left them?
    /// 2. variables that move once per group (`incrementType="Group"`) take their step if
    ///    their group changes, and the variables are worked out again;
    /// 3. the groups in order, with those values: the first that changes takes the ones
    ///    inside it along.
    fn group_rupture_with_variables(&mut self) -> Result<Option<usize>> {
        let r = self.report;
        let first_row = self.group_values.iter().any(|v| v.is_none());
        let changes = |f: &Filler, with_row: &[VarState], group: &Group| -> Result<(bool, Value)> {
            let Some(e) = &group.expression else { return Ok((false, Value::Null)) };
            let new = f.eval_with(e, with_row, f.rows.current())?;
            // The first row starts every group: there is no row before to ask about.
            if first_row {
                return Ok((true, new));
            }
            let old = f.eval_with(e, &f.committed, f.rows.record(Values::Old))?;
            Ok((!old.java_equals(&new), new))
        };
        let with_row = self.calculate(&self.committed)?;
        let mut moved = false;
        for group in &r.groups {
            if changes(self, &with_row, group)?.0 {
                for i in 0..self.defs.len() {
                    if self.stepped[i].is_some() && matches!(&self.defs[i].increment, ResetType::Group(g) if *g == group.name) {
                        self.stepped[i] = Some(self.committed[i].clone());
                        moved = true;
                    }
                }
            }
        }
        let with_row = if moved { self.calculate(&self.committed)? } else { with_row };
        let mut first = None;
        for (gi, group) in r.groups.iter().enumerate() {
            let (changed, value) = changes(self, &with_row, group)?;
            if changed && first.is_none() {
                first = Some(gi);
            }
            self.group_values[gi] = Some(value);
        }
        Ok(first)
    }

    fn run(&mut self) -> Result<()> {
        let r = self.report;
        let no_data = !self.rows.advance(self.images)?;
        if no_data {
            match r.when_no_data {
                WhenNoData::NoPages => return Ok(()),
                WhenNoData::BlankPage => {
                    self.pages.push(PrintPage::default());
                    return Ok(());
                }
                WhenNoData::NoDataSection => {
                    self.new_page()?;
                    if let Some(b) = &r.no_data {
                        self.print_band_at_offset(b, Values::Committed)?;
                    }
                    return Ok(());
                }
                WhenNoData::AllSectionsNoDetail => {}
            }
        }

        self.reset_where(|_| true)?;
        self.new_page()?;
        if let Some(t) = &r.title {
            self.print_title(t)?;
            if r.title_new_page {
                self.footers_done = true;
                self.new_page()?;
            }
        }
        self.page_headers()?;

        let mut idx = 0usize;
        let mut more = !no_data;
        while more {
            if let Some(gi) = self.group_rupture()? {
                self.end_row();
                if idx > 0 {
                    self.group_footers(gi, Values::Old)?;
                }
                let groups: Vec<String> = r.groups[gi..].iter().map(|g| g.name.clone()).collect();
                self.changed_groups = groups.clone();
                // The first row starts every group, and every variable has just started with
                // the report: starting them again would run their initial values twice (a
                // group number kept as `$V{N} == null ? 1 : $V{N} + 1` would begin at 2).
                if idx > 0 {
                    self.reset_where(|rt| matches!(rt, ResetType::Group(g) if groups.contains(g)))?;
                }
                self.group_headers(gi)?;
            }
            self.estimated = Some(self.calculate(&self.committed.clone())?);
            self.returned.clear();
            self.increment_charts(|i| *i == crate::chart::Increment::Record, Values::Estimated)?;
            if r.horizontal {
                self.print_detail_across()?;
            } else {
                for band in &r.detail {
                    self.print_body_band(band, Values::Estimated)?;
                }
            }
            self.changed_groups.clear();
            if let Some(est) = self.estimated.take() {
                self.committed = est;
            }
            idx += 1;
            if idx % 256 == 0 {
                check_time(self.images)?;
            }
            more = self.rows.advance(self.images)?;
        }

        // End of data: closing group footers see the last record.
        self.end_row();
        if !no_data {
            self.group_footers(0, Values::Committed)?;
        } else if r.when_no_data == WhenNoData::AllSectionsNoDetail && !r.groups.is_empty() {
            // Without rows, every section but the detail prints once: the groups' headers
            // and footers too, with nothing in their fields.
            self.group_headers(0)?;
            self.group_footers(0, Values::Committed)?;
        }
        self.finish()
    }

    fn finish(&mut self) -> Result<()> {
        let r = self.report;
        self.increment_charts(|i| *i == crate::chart::Increment::Report, Values::Committed)?;
        match &r.summary {
            None => self.page_footers(true, Values::Committed)?,
            Some(summary) => {
                let mut charged = 0;
                let layout = self.once(&mut charged, |f| f.layout(summary, Values::Committed))?;
                let h = layout.as_ref().map(|l| l.height).unwrap_or(0.0);
                // A summary with splitType="Immediate" starts on this page when its first
                // element fits (JasperReports' break height), and goes on over the next pages.
                let break_height = summary.elements.iter().map(|e| e.y + e.height).fold(summary.height, f32::min);
                let splits_here = summary.split_type == SplitType::Immediate
                    && layout.is_some()
                    && !r.summary_new_page
                    && self.column == 0
                    && self.offset_y + h > self.body_limit() + 1e-3
                    && self.offset_y + break_height <= self.body_limit() + 1e-3;
                if splits_here {
                    self.split_summary(summary, &mut charged)?;
                } else if r.summary_new_page || self.column > 0 || self.offset_y + h > self.body_limit() + 1e-3 {
                    self.page_footers(false, Values::Committed)?;
                    self.new_page()?;
                    if r.summary_with_page_header_and_footer {
                        self.page_headers()?;
                    } else {
                        self.footers_done = true;
                    }
                    if let Some(l) = self.once(&mut charged, |f| f.layout(summary, Values::Committed))? {
                        let y = self.offset_y;
                        let h = l.height;
                        self.emit(l, y);
                        self.offset_y += h;
                    }
                    if r.summary_with_page_header_and_footer {
                        self.page_footers(true, Values::Committed)?;
                    }
                } else {
                    if let Some(l) = layout {
                        let y = self.offset_y;
                        self.emit(l, y);
                        self.offset_y += h;
                    }
                    self.page_footers(true, Values::Committed)?;
                }
            }
        }
        self.resolve_pending(|_| true, Values::Committed)
    }

    /// Prints a summary that does not fit the rest of the page and may split there, as
    /// JasperReports does (`fillSummaryNoLastFooterSamePage`): what fits on this page, which
    /// keeps its page footer, then the rest on new pages, which have the page header and
    /// footer only with `isSummaryWithPageHeaderAndFooter`.
    fn split_summary(&mut self, summary: &Band, charged: &mut usize) -> Result<()> {
        let r = self.report;
        let mode = Values::Committed;
        let mut elements = summary.elements.clone();
        let mut design_h = summary.height;
        let mut first = true;
        loop {
            let limit = if first {
                self.body_limit()
            } else {
                let pf =
                    if r.summary_with_page_header_and_footer { r.page_footer.as_ref().map_or(0.0, |b| b.height) } else { 0.0 };
                r.page_height - r.margins.bottom - pf
            };
            let space = limit - self.offset_y;
            let full = self.once(charged, |f| f.layout_elems(&elements, design_h, mode, None))?;
            if full.height <= space + 1e-3 {
                let (y, h) = (self.offset_y, full.height);
                self.emit(full, y);
                self.offset_y += h;
                self.page_has_content = true;
                break;
            }
            let part = self.once(charged, |f| f.layout_elems(&elements, design_h, mode, Some(space)))?;
            let rest = part.rest.clone().unwrap_or_default();
            if part.items.is_empty() && !self.page_has_content {
                // Nothing fits even on an empty page: print it whole rather than loop forever.
                let (y, h) = (self.offset_y, full.height);
                self.emit(full, y);
                self.offset_y += h;
                break;
            }
            let y = self.offset_y;
            self.emit(part, y);
            self.page_has_content = true;
            if first || r.summary_with_page_header_and_footer {
                self.page_footers(false, mode)?;
            }
            self.new_page()?;
            if r.summary_with_page_header_and_footer {
                self.page_headers()?;
            } else {
                self.footers_done = true;
            }
            first = false;
            if rest.is_empty() {
                break;
            }
            elements = rest;
            let moved = move_to_top(&mut elements);
            design_h = (design_h - moved).max(0.0);
        }
        if first || r.summary_with_page_header_and_footer {
            self.page_footers(true, mode)?;
        }
        Ok(())
    }
}

/// Fills `report` with `records` and `params` into a print document.
pub fn fill(
    report: &Report,
    records: &[Record],
    params: HashMap<String, Value>,
    fonts: &mut FontSet,
    images: &mut ImageStore,
    env: &EvalEnv,
) -> Result<PrintDocument> {
    images.runs = 0;
    images.rows_read = 0;
    fonts.clear_paragraphs();
    images.skip_cells = false;
    images.deadline = None;
    let mut filler = new_filler(report, Rows::from_slice(records, report), params, fonts, images, env)?;
    filler.run()?;
    Ok(filler.document())
}

/// Like [`fill`], with limits on what the render may use.
pub fn fill_slice(
    report: &Report,
    records: &[Record],
    params: HashMap<String, Value>,
    fonts: &mut FontSet,
    images: &mut ImageStore,
    env: &EvalEnv,
    limits: Limits,
) -> Result<PrintDocument> {
    images.runs = 0;
    images.rows_read = 0;
    fonts.clear_paragraphs();
    images.skip_cells = false;
    images.deadline = limits.deadline();
    let mut filler = new_filler(report, Rows::from_slice(records, report), params, fonts, images, env)?;
    filler.limits = limits;
    filler.run()?;
    Ok(filler.document())
}

/// Like [`fill`], with rows that arrive one at a time and limits on what the render may use.
pub fn fill_rows(
    report: &Report,
    rows: RowIter,
    params: HashMap<String, Value>,
    fonts: &mut FontSet,
    images: &mut ImageStore,
    env: &EvalEnv,
    limits: Limits,
) -> Result<PrintDocument> {
    images.runs = 0;
    images.rows_read = 0;
    fonts.clear_paragraphs();
    images.skip_cells = false;
    images.deadline = limits.deadline();
    let mut filler = new_filler(report, Rows::new(rows, report), params, fonts, images, env)?;
    filler.limits = limits;
    filler.run()?;
    Ok(filler.document())
}

/// Fills `report` and hands each page to `sink` as soon as it is finished, so memory stays
/// flat however long the report is.
#[allow(clippy::too_many_arguments)]
pub fn fill_to(
    report: &Report,
    rows: RowIter,
    params: HashMap<String, Value>,
    fonts: &mut FontSet,
    images: &mut ImageStore,
    env: &EvalEnv,
    limits: Limits,
    sink: &mut dyn PageSink,
) -> Result<FillStats> {
    images.runs = 0;
    images.rows_read = 0;
    fonts.clear_paragraphs();
    images.skip_cells = !sink.wants_cells();
    images.deadline = limits.deadline();
    let mut filler = new_filler(report, Rows::new(rows, report), params, fonts, images, env)?;
    filler.limits = limits;
    filler.sink = Some(sink);
    filler.run()?;
    filler.flush_pages(true)?;
    Ok(FillStats { pages: filler.page_base, rows: filler.rows.count })
}

/// Fills a subreport without pagination: its bands as segments, and its final variables.
fn fill_flow(
    report: &Report,
    records: &[Record],
    params: HashMap<String, Value>,
    fonts: &mut FontSet,
    images: &mut ImageStore,
    env: &EvalEnv,
) -> Result<(Vec<Segment>, HashMap<String, Value>)> {
    // A horizontal list is filled twice: once to learn how tall each row ends, since every
    // cell of a row is as tall as its tallest, and once to lay the cells out at that height.
    // A nested report's hyphenation lasts while it is filled.
    let outer = fonts.set_hyphenation(None);
    let filled = fill_flow_inner(report, records, params, fonts, images, env);
    fonts.restore_hyphenation(outer);
    filled
}

fn fill_flow_inner(
    report: &Report,
    records: &[Record],
    params: HashMap<String, Value>,
    fonts: &mut FontSet,
    images: &mut ImageStore,
    env: &EvalEnv,
) -> Result<(Vec<Segment>, HashMap<String, Value>)> {
    let mut row_heights = Vec::new();
    if report.horizontal {
        // The rows are read again below: this pass does not count them.
        let read = images.rows_read;
        let mut first = new_filler(report, Rows::from_slice(records, report), params.clone(), fonts, images, env)?;
        first.flow = Some(Vec::new());
        first.run()?;
        row_heights = std::mem::take(&mut first.row_heights);
        drop(first);
        images.rows_read = read;
    }
    let mut filler = new_filler(report, Rows::from_slice(records, report), params, fonts, images, env)?;
    filler.flow = Some(Vec::new());
    filler.row_heights = row_heights;
    filler.run()?;
    let vars = filler.var_index.iter().map(|(name, &i)| (name.clone(), filler.committed[i].value.clone())).collect();
    let marks = filler.flow.take().unwrap_or_default();
    let mut items: Vec<Option<PrintItem>> = match filler.pages.first_mut() {
        Some(p) => std::mem::take(&mut p.items).into_iter().map(Some).collect(),
        None => Vec::new(),
    };
    let mut segments = Vec::with_capacity(marks.len() + 2);
    if let Some(first) = marks.first() {
        if first.y > 0.0 {
            segments.push(Segment { height: first.y, items: Vec::new(), header: false, footer: false });
        }
    }
    let mut last_y = f32::NAN;
    for mark in &marks {
        let mut seg = Vec::with_capacity(mark.items.len());
        for slot in &mut items[mark.items.clone()] {
            if let Some(mut item) = slot.take() {
                translate(&mut item, 0.0, -mark.y);
                seg.push(item);
            }
        }
        // The cells of one row of a horizontal list start at the same height: one segment.
        match segments.last_mut() {
            Some(row) if report.horizontal && (mark.y - last_y).abs() < 1e-3 => {
                row.height = row.height.max(mark.height);
                row.items.extend(seg);
            }
            _ => segments.push(Segment { height: mark.height, items: seg, header: mark.header, footer: false }),
        }
        last_y = mark.y;
    }
    if !marks.is_empty() && report.margins.bottom > 0.0 {
        segments.push(Segment { height: report.margins.bottom, items: Vec::new(), header: false, footer: false });
    }
    // The page footer for the pages this content continues past (see `Segment::footer`). A
    // report that printed nothing (no rows and `whenNoDataType="NoPages"`) has no pages to
    // continue past: it stays empty, so a subreport of it is blank (isRemoveLineWhenBlank).
    if let Some(pf) = report.page_footer.as_ref().filter(|_| !marks.is_empty()) {
        if let Some(l) = filler.layout(pf, Values::Committed)? {
            let items = l
                .items
                .into_iter()
                .map(|laid| {
                    let mut item = laid.item;
                    translate(&mut item, report.margins.left, 0.0);
                    item
                })
                .collect();
            segments.push(Segment { height: l.height, items, header: false, footer: true });
        }
    }
    Ok((segments, vars))
}

fn new_filler<'a>(
    report: &'a Report,
    rows: Rows<'a>,
    mut params: HashMap<String, Value>,
    fonts: &'a mut FontSet,
    images: &'a mut ImageStore,
    env: &'a EvalEnv<'a>,
) -> Result<Filler<'a>> {
    fonts.set_language(env.locale.tag);
    // Built-in variables, in the order Jasper declares them.
    let count_var = |name: String, reset: ResetType| VarDef {
        name,
        class: JavaType::Integer,
        calculation: Calculation::Count,
        reset,
        increment: ResetType::None,
        expression: Some(Expr::Lit(Value::Int(1))),
        initial: Some(Expr::Lit(Value::Int(0))),
    };
    let mut defs = vec![
        count_var("REPORT_COUNT".into(), ResetType::Report),
        count_var("PAGE_COUNT".into(), ResetType::Page),
        count_var("COLUMN_COUNT".into(), ResetType::Column),
    ];
    for g in &report.groups {
        defs.push(count_var(format!("{}_COUNT", g.name), ResetType::Group(g.name.clone())));
    }
    for v in &report.variables {
        defs.push(VarDef {
            name: v.name.clone(),
            class: v.class,
            calculation: v.calculation,
            reset: v.reset.clone(),
            increment: v.increment.clone(),
            expression: v.expression.clone(),
            initial: v.initial.clone(),
        });
    }
    let stepped = defs.iter().map(|d| (!matches!(d.increment, ResetType::None)).then(VarState::default)).collect();
    // A group expression that reads variables reads them as they will be with the row at hand.
    let groups_read_variables =
        report.groups.iter().any(|g| g.expression.as_ref().is_some_and(|e| format!("{e:?}").contains("Var(")));
    let var_index = defs.iter().enumerate().map(|(i, d)| (d.name.clone(), i)).collect();

    // Parameters: supplied values, then defaults (which may reference earlier parameters).
    let builtin_params =
        ["REPORT_PARAMETERS_MAP", "REPORT_CONNECTION", "REPORT_DATA_SOURCE", "REPORT_SCRIPTLET", "REPORT_MAX_COUNT"];
    for name in builtin_params {
        params.entry(name.to_string()).or_insert(Value::Null);
    }
    params.entry("REPORT_LOCALE".into()).or_insert_with(|| Value::str(env.locale.tag));
    // The texts of `$R{key}`: the report's own bundle, or the one it was handed (a table or
    // list inside a report uses the report's).
    if !params.contains_key(crate::expr::eval::RESOURCE_BUNDLE) {
        let mut texts: serde_json::Map<String, serde_json::Value> = report
            .resource_bundle
            .as_deref()
            .and_then(|base| images.load_bundle(base, env.locale.tag))
            .map(|t| t.into_iter().map(|(k, v)| (k, serde_json::Value::String(v))).collect())
            .unwrap_or_default();
        let missing = format!("{:?}", report.when_resource_missing);
        texts.insert(crate::expr::eval::RESOURCE_MISSING.into(), serde_json::Value::String(missing));
        params.insert(crate::expr::eval::RESOURCE_BUNDLE.into(), Value::Json(Arc::new(serde_json::Value::Object(texts))));
    }
    params.entry("REPORT_TIME_ZONE".into()).or_insert_with(|| Value::str("UTC"));
    params.entry("IS_IGNORE_PAGINATION".into()).or_insert(Value::Bool(false));
    for p in &report.parameters {
        if params.contains_key(&p.name) {
            let v = params.remove(&p.name).unwrap_or_default();
            params.insert(p.name.clone(), p.class.coerce(v));
            continue;
        }
        let v = match &p.default {
            Some(e) => {
                let empty = HashMap::new();
                let ctx = Ctx {
                    record: None,
                    report,
                    params: &params,
                    vars: &[],
                    var_index: &empty,
                    page_number: 1,
                    column_number: 1,
                    master: None,
                };
                eval(e, &ctx, env)?
            }
            None => Value::Null,
        };
        params.insert(p.name.clone(), v);
    }
    fonts.set_hyphenation(params.get(crate::text::HYPHENATE));
    fonts.add_paragraphs(params.get(crate::text::PARAGRAPHS));

    let committed = vec![VarState::default(); defs.len()];
    Ok(Filler {
        report,
        rows,
        params,
        fonts,
        images,
        env,
        group_values: vec![None; report.groups.len()],
        stepped,
        groups_read_variables,
        split_carry: false,
        reprint_mode: Values::Committed,
        row: None,
        row_heights: Vec::new(),
        row_index: 0,
        defs,
        var_index,
        committed,
        estimated: None,
        pages: Vec::new(),
        page_base: 0,
        sink: None,
        held: false,
        limits: Limits::default(),
        page_number: 0,
        offset_y: 0.0,
        page_has_content: false,
        pending: Vec::new(),
        pending_final: Vec::new(),
        footers_done: false,
        starting_group: None,
        repeated: HashMap::new(),
        changed_groups: Vec::new(),
        returned: Vec::new(),
        charts: {
            let mut charts = Vec::new();
            let r = report;
            let mut bands: Vec<&Band> = Vec::new();
            for b in [
                &r.background,
                &r.title,
                &r.page_header,
                &r.column_header,
                &r.column_footer,
                &r.page_footer,
                &r.last_page_footer,
                &r.summary,
                &r.no_data,
            ]
            .into_iter()
            .flatten()
            {
                bands.push(b);
            }
            bands.extend(r.detail.iter());
            for g in &r.groups {
                bands.extend(g.header.iter());
                bands.extend(g.footer.iter());
            }
            for b in bands {
                collect_charts(&b.elements, &mut charts);
            }
            charts
        },
        chart_data: HashMap::new(),
        column: 0,
        column_top: 0.0,
        in_column: false,
        flow: None,
        flow_header: false,
    })
}
