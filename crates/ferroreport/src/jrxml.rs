//! JRXML template parser (JasperReports 6 and 7 schemas).
//!
//! Styles are resolved here, so the filler only ever sees concrete fonts, colors and pens.
//! Elements this engine cannot render are kept as [`ElementKind::Unsupported`] and listed in
//! [`Report::warnings`], so a template always loads and the `doctor` command can explain
//! what is missing.

use crate::error::{Error, Result};
use crate::expr::{parse_expr, Expr};
use crate::model::*;
use crate::value::JavaType;
use crate::xml::{self, Node};
use std::collections::HashMap;

/// Reads a file a template refers to (a style template), by its location; `None` when
/// there is no such file.
pub type Loader<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// Parses JRXML source into a [`Report`], without the files it refers to.
pub fn parse_report(source: &str) -> Result<Report> {
    parse_report_with(source, &|_| None)
}

/// Parses JRXML source into a [`Report`], reading the style templates it uses with `load`.
pub fn parse_report_with(source: &str, load: Loader) -> Result<Report> {
    parse_report_located(source, load).map_err(|e| match e {
        // Where the expression is in the template (the first place its text appears).
        Error::ExprParse { expr, message, line: 0, .. } => {
            let text = expr.trim_end_matches('…');
            let (line, column) = match source.find(text) {
                Some(at) => crate::error::line_column(source, at),
                None => (0, 0),
            };
            Error::ExprParse { expr, message, line, column }
        }
        other => other,
    })
}

fn parse_report_located(source: &str, load: Loader) -> Result<Report> {
    // ferroreport's own format (`.ferro`) is compiled to JRXML first.
    let compiled;
    let source = if crate::template::is_template(source) {
        compiled = crate::template::to_jrxml_with(source, Some(load))?;
        compiled.as_str()
    } else {
        source
    };
    let root = xml::parse(source)?;
    if root.name != "jasperReport" {
        return Err(Error::template(format!("root element is <{}>, expected <jasperReport>", root.name)));
    }
    let mut report = Parser::new(&root, load)?.report(&root)?;
    report.uses_data_source = source.contains("REPORT_DATA_SOURCE");
    Ok(report)
}

/// Templates within templates go no deeper than this.
const MAX_STYLE_TEMPLATES: usize = 8;

/// The styles of a report in the order JasperReports takes them: those of each style
/// template (`<template>`), after the templates it includes, then the report's own.
fn style_nodes(root: &Node, load: Loader, warnings: &mut Vec<String>) -> Vec<Node> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for t in root.children_named("template") {
        match template_location(root, &t.text) {
            Some(location) => style_template(&location, None, load, 0, &mut seen, &mut out, warnings),
            None => warnings.push(format!(
                "style template `{}`: only a file name in quotes, or a parameter whose default is one, can be read before the report runs",
                t.text.trim()
            )),
        }
    }
    out.extend(root.children_named("style").cloned());
    out
}

/// The file a `<template>` of a report names: a string, or a parameter with a string default.
fn template_location(root: &Node, text: &str) -> Option<String> {
    let literal = |e: &str| match parse_expr(e, false).ok()? {
        Expr::Lit(crate::value::Value::Str(s)) => Some(s.to_string()),
        _ => None,
    };
    if let Some(location) = literal(text) {
        return Some(location);
    }
    let name = text.trim().strip_prefix("$P{")?.strip_suffix('}')?;
    let p = root.children_named("parameter").find(|p| p.attr("name") == Some(name))?;
    literal(&p.child("defaultValueExpression")?.text)
}

/// Reads one style template and those it includes, adding their styles to `out`.
fn style_template(
    location: &str,
    from: Option<&str>,
    load: Loader,
    depth: usize,
    seen: &mut Vec<String>,
    out: &mut Vec<Node>,
    warnings: &mut Vec<String>,
) {
    if depth >= MAX_STYLE_TEMPLATES || seen.iter().any(|s| s == location) {
        warnings.push(format!("style template `{location}` is included again or too deep, and is skipped"));
        return;
    }
    // Next to the template that includes it, or else where the report's resources are.
    let beside = from.and_then(|f| f.rsplit_once('/')).map(|(dir, _)| format!("{dir}/{location}"));
    let found = beside.iter().map(String::as_str).chain([location]).find_map(|l| load(l).map(|bytes| (l.to_string(), bytes)));
    let Some((path, bytes)) = found else {
        warnings.push(format!("style template `{location}` was not found next to the report or in its resource folders"));
        return;
    };
    let parsed = std::str::from_utf8(&bytes).map_err(|e| e.to_string()).and_then(|t| {
        // A style template may be in ferroreport's own format too (`styles` at the top).
        let compiled = if crate::template::is_template(t) {
            crate::template::to_jrxml(t).map_err(|e| e.to_string())?
        } else {
            t.to_string()
        };
        xml::parse(&compiled).map_err(|e| e.to_string())
    });
    let root = match parsed {
        Ok(root) if root.name == "jasperTemplate" => root,
        Ok(root) => return warnings.push(format!("style template `{location}`: <{}> is not a <jasperTemplate>", root.name)),
        Err(e) => return warnings.push(format!("style template `{location}`: {e}")),
    };
    seen.push(location.to_string());
    for t in root.children_named("template") {
        style_template(t.text.trim(), Some(&path), load, depth + 1, seen, out, warnings);
    }
    out.extend(root.children_named("style").cloned());
}

// ---------------------------------------------------------------------------------------------
// Attribute helpers

fn parse_f32(v: &str, what: &str) -> Result<f32> {
    number(v).ok_or_else(|| Error::template(format!("invalid number `{v}` for {what}")))
}

/// A number from an attribute. Values that are not finite (`inf`, `NaN`, `1e999`) or are
/// far beyond any page are refused: a size like that would send layout into a loop.
fn number<T: std::str::FromStr>(v: &str) -> Option<T> {
    let v = v.trim();
    let f: f64 = v.parse().ok()?;
    if f.is_finite() && f.abs() <= 1e7 {
        v.parse().ok()
    } else {
        None
    }
}

fn parse_bool(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("true")
}

/// First attribute among `names` present on `node`.
fn attr<'a>(node: &'a Node, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|n| node.attr(n))
}

fn parse_color(v: &str) -> Option<Color> {
    let v = v.trim();
    if let Some(hex) = v.strip_prefix('#') {
        // Only hexadecimal digits make a color (and anything else could not be sliced by bytes).
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let hex = if hex.len() == 3 { hex.chars().flat_map(|c| [c, c]).collect::<String>() } else { hex.to_string() };
        let n = u32::from_str_radix(&hex[hex.len().saturating_sub(6)..], 16).ok()?;
        return Some(Color { r: (n >> 16) as u8, g: (n >> 8) as u8, b: n as u8 });
    }
    // JRXML 7 writes a color with transparency as rgba(r, g, b, alpha): drawn here over white.
    if let Some(inner) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() < 3 {
            return None;
        }
        let alpha = parts.get(3).copied().unwrap_or(1.0).clamp(0.0, 1.0);
        let over_white = |c: f32| (c.clamp(0.0, 255.0) * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        return Some(Color { r: over_white(parts[0]), g: over_white(parts[1]), b: over_white(parts[2]) });
    }
    let (r, g, b) = match v.to_ascii_lowercase().as_str() {
        "black" => (0, 0, 0),
        "white" => (255, 255, 255),
        "red" => (255, 0, 0),
        "green" => (0, 255, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "cyan" => (0, 255, 255),
        "magenta" => (255, 0, 255),
        "orange" => (255, 200, 0),
        "pink" => (255, 175, 175),
        "gray" => (128, 128, 128),
        "lightgray" => (192, 192, 192),
        "darkgray" => (64, 64, 64),
        _ => return None,
    };
    Some(Color { r, g, b })
}

// ---------------------------------------------------------------------------------------------
// Style properties: every field optional so styles can be layered.

#[derive(Debug, Clone, Default)]
struct PenProps {
    width: Option<f32>,
    color: Option<Color>,
    style: Option<LineStyle>,
}

impl PenProps {
    fn read(node: &Node) -> PenProps {
        PenProps {
            width: node.attr("lineWidth").and_then(number),
            color: node.attr("lineColor").and_then(parse_color),
            style: node.attr("lineStyle").map(|s| match s {
                "Dashed" => LineStyle::Dashed,
                "Dotted" => LineStyle::Dotted,
                "Double" => LineStyle::Double,
                _ => LineStyle::Solid,
            }),
        }
    }
    fn over(&self, base: &PenProps) -> PenProps {
        PenProps { width: self.width.or(base.width), color: self.color.or(base.color), style: self.style.or(base.style) }
    }
    fn resolve(&self, default_width: f32, default_color: Color) -> Pen {
        Pen {
            width: self.width.unwrap_or(default_width),
            color: self.color.unwrap_or(default_color),
            style: self.style.unwrap_or(LineStyle::Solid),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct StyleProps {
    font_name: Option<String>,
    pdf_font_name: Option<String>,
    font_size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    forecolor: Option<Color>,
    backcolor: Option<Color>,
    mode: Option<Mode>,
    halign: Option<HAlign>,
    valign: Option<VAlign>,
    /// `hImageAlign` and `vImageAlign` of a style, as written (icon labels take them).
    himage: Option<String>,
    vimage: Option<String>,
    line_spacing: Option<String>,
    line_spacing_size: Option<f32>,
    first_line_indent: Option<i32>,
    left_indent: Option<i32>,
    right_indent: Option<i32>,
    tab_stop_width: Option<i32>,
    tab_stops: Option<Vec<crate::paragraph::TabStop>>,
    pattern: Option<String>,
    blank_when_null: Option<bool>,
    radius: Option<f32>,
    /// all, top, left, bottom, right
    padding: [Option<f32>; 5],
    /// box pen, top, left, bottom, right
    box_pens: [PenProps; 5],
    graphic_pen: PenProps,
}

macro_rules! layer {
    ($self:ident, $base:ident, $($f:ident),*) => { StyleProps { $($f: $self.$f.clone().or($base.$f.clone()),)* ..Default::default() } };
}

impl StyleProps {
    /// `self` on top of `base`.
    fn over(&self, base: &StyleProps) -> StyleProps {
        let mut out = layer!(
            self,
            base,
            font_name,
            pdf_font_name,
            font_size,
            bold,
            italic,
            underline,
            strike,
            forecolor,
            backcolor,
            mode,
            halign,
            valign,
            himage,
            vimage,
            line_spacing,
            line_spacing_size,
            first_line_indent,
            left_indent,
            right_indent,
            tab_stop_width,
            tab_stops,
            pattern,
            blank_when_null,
            radius
        );
        for i in 0..5 {
            out.padding[i] = self.padding[i].or(base.padding[i]);
            out.box_pens[i] = self.box_pens[i].over(&base.box_pens[i]);
        }
        out.graphic_pen = self.graphic_pen.over(&base.graphic_pen);
        out
    }

    /// Reads font-ish attributes that may sit on `<font>`, `<style>` or a v7 `<element>`.
    fn read_font_attrs(&mut self, n: &Node) {
        if let Some(v) = attr(n, &["fontName"]) {
            self.font_name = Some(v.to_string());
        }
        if let Some(v) = attr(n, &["pdfFontName"]) {
            self.pdf_font_name = Some(v.to_string());
        }
        if let Some(v) = attr(n, &["size", "fontSize"]).and_then(number) {
            self.font_size = Some(v);
        }
        let b = |names: &[&str]| attr(n, names).map(parse_bool);
        self.bold = b(&["isBold", "bold"]).or(self.bold);
        self.italic = b(&["isItalic", "italic"]).or(self.italic);
        self.underline = b(&["isUnderline", "underline"]).or(self.underline);
        self.strike = b(&["isStrikeThrough", "strikeThrough"]).or(self.strike);
    }

    fn read_alignment(&mut self, n: &Node) {
        if let Some(v) = attr(n, &["hTextAlign", "textAlignment", "hAlign", "horizontalTextAlign"]) {
            self.halign = Some(match v {
                "Center" => HAlign::Center,
                "Right" => HAlign::Right,
                "Justified" => HAlign::Justified,
                _ => HAlign::Left,
            });
        }
        if let Some(v) = attr(n, &["vTextAlign", "verticalAlignment", "vAlign", "verticalTextAlign"]) {
            self.valign = Some(match v {
                "Middle" => VAlign::Middle,
                "Bottom" => VAlign::Bottom,
                _ => VAlign::Top,
            });
        }
    }

    fn read_colors(&mut self, n: &Node) {
        if let Some(c) = n.attr("forecolor").and_then(parse_color) {
            self.forecolor = Some(c);
        }
        if let Some(c) = n.attr("backcolor").and_then(parse_color) {
            self.backcolor = Some(c);
        }
        if let Some(m) = n.attr("mode") {
            self.mode = Some(if m == "Opaque" { Mode::Opaque } else { Mode::Transparent });
        }
    }

    fn read_box(&mut self, b: &Node) {
        let p = |names: &[&str]| attr(b, names).and_then(number::<f32>);
        self.padding[0] = p(&["padding"]).or(self.padding[0]);
        self.padding[1] = p(&["topPadding"]).or(self.padding[1]);
        self.padding[2] = p(&["leftPadding"]).or(self.padding[2]);
        self.padding[3] = p(&["bottomPadding"]).or(self.padding[3]);
        self.padding[4] = p(&["rightPadding"]).or(self.padding[4]);
        for (i, name) in ["pen", "topPen", "leftPen", "bottomPen", "rightPen"].iter().enumerate() {
            if let Some(pn) = b.child(name) {
                self.box_pens[i] = PenProps::read(pn).over(&self.box_pens[i]);
            }
        }
    }

    fn read_paragraph(&mut self, p: &Node) {
        if let Some(v) = p.attr("lineSpacing") {
            self.line_spacing = Some(v.to_string());
        }
        if let Some(v) = p.attr("lineSpacingSize").and_then(number) {
            self.line_spacing_size = Some(v);
        }
        // Whole points, as JasperReports reads them.
        let int = |name: &str| p.attr(name).and_then(number::<f32>).filter(|v| v.is_finite()).map(|v| v.clamp(-1e6, 1e6) as i32);
        self.first_line_indent = int("firstLineIndent").or(self.first_line_indent);
        self.left_indent = int("leftIndent").or(self.left_indent);
        self.right_indent = int("rightIndent").or(self.right_indent);
        self.tab_stop_width = int("tabStopWidth").or(self.tab_stop_width);
        let stops: Vec<crate::paragraph::TabStop> = p
            .children_named("tabStop")
            .filter_map(|t| {
                let position = t.attr("position").and_then(number::<f32>).filter(|v| v.is_finite())?.clamp(-1e6, 1e6) as i32;
                let align = match t.attr("alignment") {
                    Some("Center") => crate::paragraph::TabAlign::Center,
                    Some("Right") => crate::paragraph::TabAlign::Right,
                    _ => crate::paragraph::TabAlign::Left,
                };
                Some(crate::paragraph::TabStop { position, align })
            })
            .collect();
        if !stops.is_empty() {
            self.tab_stops = Some(stops);
        }
    }

    /// The paragraph settings that change how a text is laid out across its width.
    fn paragraph(&self) -> crate::paragraph::Paragraph {
        let d = crate::paragraph::Paragraph::default();
        crate::paragraph::Paragraph {
            first_line_indent: self.first_line_indent.unwrap_or(d.first_line_indent),
            left_indent: self.left_indent.unwrap_or(d.left_indent),
            right_indent: self.right_indent.unwrap_or(d.right_indent),
            tab_stop_width: self.tab_stop_width.filter(|w| *w > 0).unwrap_or(d.tab_stop_width),
            tab_stops: self.tab_stops.clone().unwrap_or_default(),
        }
    }

    /// Properties declared directly by a `<style>` element.
    fn from_style_node(n: &Node) -> StyleProps {
        let mut s = StyleProps::default();
        s.read_font_attrs(n);
        s.read_alignment(n);
        s.himage = attr(n, &["hImageAlign", "horizontalImageAlign"]).map(str::to_string);
        s.vimage = attr(n, &["vImageAlign", "verticalImageAlign"]).map(str::to_string);
        s.read_colors(n);
        if let Some(v) = n.attr("pattern") {
            s.pattern = Some(v.to_string());
        }
        if let Some(v) = attr(n, &["isBlankWhenNull", "blankWhenNull"]) {
            s.blank_when_null = Some(parse_bool(v));
        }
        if let Some(v) = n.attr("radius").and_then(number) {
            s.radius = Some(v);
        }
        if let Some(pen) = n.child("pen") {
            s.graphic_pen = PenProps::read(pen);
        }
        if let Some(b) = n.child("box") {
            s.read_box(b);
        }
        if let Some(p) = n.child("paragraph") {
            s.read_paragraph(p);
        }
        s
    }

    fn line_spacing(&self) -> LineSpacing {
        let size = self.line_spacing_size.unwrap_or(1.0);
        match self.line_spacing.as_deref() {
            Some("1_1_2") => LineSpacing::OneAndHalf,
            Some("Double") => LineSpacing::Double,
            Some("Proportional") => LineSpacing::Proportional(size),
            Some("AtLeast") => LineSpacing::AtLeast(size),
            Some("Fixed") => LineSpacing::Fixed(size),
            _ => LineSpacing::Single,
        }
    }

    fn box_style(&self, forecolor: Color) -> BoxStyle {
        let all = self.padding[0].unwrap_or(0.0);
        let side = |i: usize| self.box_pens[i].over(&self.box_pens[0]).resolve(0.0, forecolor);
        BoxStyle {
            padding: Padding {
                top: self.padding[1].unwrap_or(all),
                left: self.padding[2].unwrap_or(all),
                bottom: self.padding[3].unwrap_or(all),
                right: self.padding[4].unwrap_or(all),
            },
            top: side(1),
            left: side(2),
            bottom: side(3),
            right: side(4),
        }
    }
}

// ---------------------------------------------------------------------------------------------

struct Parser {
    chart_count: usize,
    groovy: bool,
    styles: HashMap<String, StyleProps>,
    default_style: StyleProps,
    default_name: Option<String>,
    /// Conditional styles per style name: (condition, properties of the inner style).
    conditionals: HashMap<String, Vec<(Expr, StyleProps)>>,
    warnings: Vec<String>,
    /// `<subDataset>`s by name, parsed before the bands so tables can use them.
    datasets: HashMap<String, Dataset>,
    page_height: f32,
    /// The report's `<property>` values, defaults for element properties.
    report_properties: Vec<(String, String)>,
    /// How charts are drawn (`net.sf.jasperreports.chart.render.type`), from the report's
    /// properties or a `jasperreports.properties` among the resources.
    chart_render_type: Option<String>,
    /// Paragraph settings of the text elements that have any, by fingerprint (see
    /// [`crate::paragraph`]).
    paragraphs: Vec<(u64, crate::paragraph::Paragraph)>,
    /// The application's own classes that parameters, fields and variables are declared with
    /// (simple names): their static fields are taken as enum constants, see [`enum_constants`].
    own_classes: Vec<String>,
}

/// The classes outside the Java and JasperReports libraries that the template declares its
/// parameters, fields and variables with (`class="Greeting"`), by simple name.
fn own_classes(root: &Node) -> Vec<String> {
    fn visit(n: &Node, out: &mut Vec<String>) {
        if matches!(n.name.as_str(), "parameter" | "field" | "variable") {
            let class = n.attr("class").unwrap_or("").trim();
            let library = ["java.", "javax.", "net.sf.jasperreports.", "groovy.", "org.", "com.lowagie."];
            if !class.is_empty()
                && !library.iter().any(|p| class.starts_with(p))
                && JavaType::from_class_name(class) == JavaType::Object
            {
                let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
                if simple.chars().next().is_some_and(char::is_uppercase) && !out.iter().any(|c| c == simple) {
                    out.push(simple.to_string());
                }
            }
        }
        n.children.iter().for_each(|c| visit(c, out));
    }
    let mut out = Vec::new();
    visit(root, &mut out);
    out
}

/// A static field of one of the template's own classes (`Greeting.bye`) is an enum constant,
/// and a Java enum comes from JSON as its name: it is read as the text `"bye"`, so that
/// `$P{greeting} == Greeting.bye` holds when the parameter is `"bye"`.
fn enum_constants(expr: &mut Expr, classes: &[String]) {
    if classes.is_empty() {
        return;
    }
    expr.walk_mut(&mut |e| {
        if let Expr::StaticField { class, name } = e {
            if classes.iter().any(|c| **c == **class) {
                *e = Expr::Lit(crate::value::Value::str(&**name));
            }
        }
    });
}

/// The value of `key` in a Java properties file.
fn java_property(text: &str, key: &str) -> Option<String> {
    text.lines().map(str::trim).filter(|l| !l.starts_with('#') && !l.starts_with('!')).find_map(|l| {
        let at = l.find(['=', ':']).unwrap_or(l.len());
        (l[..at].trim() == key).then(|| l.get(at + 1..).unwrap_or("").trim().to_string())
    })
}

/// Most conditional styles one element may combine (variants are precomputed per subset).
const MAX_CONDITIONAL_STYLES: usize = 4;

impl Parser {
    fn new(root: &Node, load: Loader) -> Result<Parser> {
        let groovy = root.attr("language").map(|l| l.eq_ignore_ascii_case("groovy")).unwrap_or(false);
        let mut warnings = Vec::new();
        let nodes = style_nodes(root, load, &mut warnings);
        let parent_of = |s: &Node| attr(s, &["style", "parentStyle"]).map(str::to_string);
        let is_default = |s: &Node| attr(s, &["isDefault", "default"]).is_some_and(parse_bool);

        // Styles are resolved in the order they are defined, and a parent is the style of that
        // name defined before: a style redefined later (in a report over a style template)
        // replaces the earlier one for what comes after, not for what already inherited it.
        // First pass: each style's parent, and the props of each style through its parents.
        let mut parents: Vec<Option<usize>> = Vec::with_capacity(nodes.len());
        let mut plain: Vec<StyleProps> = Vec::with_capacity(nodes.len());
        let mut latest: HashMap<&str, usize> = HashMap::new();
        for (i, s) in nodes.iter().enumerate() {
            let parent = parent_of(s).and_then(|p| latest.get(p.as_str()).copied());
            let props = StyleProps::from_style_node(s);
            plain.push(match parent {
                Some(p) => props.over(&plain[p]),
                None => props,
            });
            parents.push(parent);
            if let Some(name) = s.attr("name") {
                latest.insert(name, i);
            }
        }
        // The default style is the last one marked so. A style without a parent takes what it
        // does not set from it, as an element without a style does; the default style and its
        // own ancestors are the exception, since they are what the others fall back on.
        let default_index = nodes.iter().rposition(is_default);
        let mut base_chain = Vec::new();
        let mut at = default_index;
        while let Some(i) = at.filter(|i| !base_chain.contains(i)) {
            base_chain.push(i);
            at = parents[i];
        }
        let fallback = default_index.map(|i| plain[i].clone()).unwrap_or_default();

        // Second pass: the styles as elements see them.
        let mut resolved: Vec<StyleProps> = Vec::with_capacity(nodes.len());
        let mut styles: HashMap<String, StyleProps> = HashMap::new();
        let mut raw_conditionals: Vec<(String, String, StyleProps)> = Vec::new();
        for (i, s) in nodes.iter().enumerate() {
            let props = StyleProps::from_style_node(s);
            let props = match parents[i] {
                Some(p) => props.over(&resolved[p]),
                None if !base_chain.contains(&i) => props.over(&fallback),
                None => props,
            };
            resolved.push(props.clone());
            let Some(name) = s.attr("name") else { continue };
            // A style defined again replaces the conditions of the one before.
            raw_conditionals.retain(|(n, ..)| n != name);
            for cs in s.children_named("conditionalStyle") {
                let cond = cs.child("conditionExpression").map(|c| c.text.clone()).unwrap_or_default();
                // JRXML 6 puts the style in a `<style>` inside; JRXML 7 on the condition itself.
                let inner = cs.child("style").map(StyleProps::from_style_node).unwrap_or_else(|| StyleProps::from_style_node(cs));
                raw_conditionals.push((name.to_string(), cond, inner));
            }
            styles.insert(name.to_string(), props);
        }
        let default_style = default_index.map(|i| resolved[i].clone()).unwrap_or_default();
        let default_name = default_index.and_then(|i| nodes[i].attr("name")).map(str::to_string);
        let mut parser = Parser {
            chart_count: 0,
            groovy,
            styles,
            default_style,
            default_name,
            conditionals: HashMap::new(),
            warnings: Vec::new(),
            datasets: HashMap::new(),
            page_height: attr(root, &["pageHeight"]).and_then(number).unwrap_or(842.0),
            report_properties: root
                .children_named("property")
                .filter_map(|p| {
                    let value = p.attr("value").map(str::to_string).unwrap_or_else(|| p.text.clone());
                    p.attr("name").map(|n| (n.to_string(), value))
                })
                .collect(),
            chart_render_type: None,
            paragraphs: Vec::new(),
            own_classes: own_classes(root),
        };
        const RENDER_TYPE: &str = "net.sf.jasperreports.chart.render.type";
        parser.chart_render_type =
            parser.report_properties.iter().find(|(k, _)| k == RENDER_TYPE).map(|(_, v)| v.clone()).or_else(|| {
                load("jasperreports.properties").and_then(|b| java_property(&String::from_utf8_lossy(&b), RENDER_TYPE))
            });
        for (name, cond, props) in raw_conditionals {
            if let Some(e) = parser.expr(&cond)? {
                parser.conditionals.entry(name).or_default().push((e, props));
            }
        }
        for (name, list) in &parser.conditionals {
            if list.len() > MAX_CONDITIONAL_STYLES {
                warnings.push(format!("style `{name}`: only the first {MAX_CONDITIONAL_STYLES} conditional styles are applied"));
            }
        }
        parser.warnings = warnings;
        Ok(parser)
    }

    fn expr(&self, text: &str) -> Result<Option<Expr>> {
        let t = text.trim();
        if t.is_empty() {
            return Ok(None);
        }
        let mut parsed = parse_expr(t, self.groovy);
        if let Ok(e) = &mut parsed {
            enum_constants(e, &self.own_classes);
        }
        parsed.map(Some).map_err(|message| {
            // Quote a long expression only in part.
            let expr = match t.char_indices().nth(120) {
                Some((i, _)) => format!("{}…", &t[..i]),
                None => t.to_string(),
            };
            Error::ExprParse { expr, message, line: 0, column: 0 }
        })
    }

    fn child_expr(&self, n: &Node, names: &[&str]) -> Result<Option<Expr>> {
        match names.iter().find_map(|name| n.child(name)) {
            Some(c) => self.expr(&c.text),
            None => Ok(None),
        }
    }

    fn report(mut self, root: &Node) -> Result<Report> {
        let num = |names: &[&str], default: f32| -> Result<f32> {
            match attr(root, names) {
                Some(v) => parse_f32(v, names[0]),
                None => Ok(default),
            }
        };
        let page_width = num(&["pageWidth"], 595.0)?;
        let left = num(&["leftMargin"], 20.0)?;
        let right = num(&["rightMargin"], 20.0)?;
        // Subdatasets first: tables in the bands refer to them.
        for d in root.children_named("subDataset").chain(root.children_named("dataset")) {
            let dataset = self.dataset(d)?;
            self.datasets.insert(dataset.name.clone(), dataset);
        }
        let Dataset { mut parameters, fields, variables, groups, query, .. } = self.dataset(root)?;
        // ferroreport's own hyphenation (JasperReports does not hyphenate): the report's
        // property becomes the default of a parameter of the same name, which a render can
        // also set, and which tables, lists and subreports without their own setting inherit.
        if let Some(value) = self.report_properties.iter().find(|(k, _)| k == crate::text::HYPHENATE).map(|(_, v)| v.clone()) {
            if !parameters.iter().any(|p| p.name == crate::text::HYPHENATE) {
                parameters.push(Parameter {
                    name: crate::text::HYPHENATE.to_string(),
                    class: JavaType::String,
                    default: Some(Expr::Lit(crate::value::Value::str(value.as_str()))),
                });
            }
        }
        if root.children_named("property").any(|p| {
            p.attr("name") == Some("net.sf.jasperreports.evaluator.ignore.npe") && p.attr("value").is_some_and(|v| !parse_bool(v))
        }) {
            self.warnings.push(
                "net.sf.jasperreports.evaluator.ignore.npe=false is not supported: a NullPointerException still makes its expression null"
                    .into(),
            );
        }
        if root.child("scriptlet").is_some() {
            self.warnings.push("<scriptlet> is not supported yet".into());
        }
        if root.attr("scriptletClass").is_some_and(|c| !c.trim().is_empty() && !c.ends_with("JRDefaultScriptlet")) {
            self.warnings.push("scriptletClass (a Java scriptlet) is not run: expressions that call it fail".into());
        }

        let band =
            |p: &mut Parser, name: &str| -> Result<Option<Band>> { Ok(p.section_bands(root.child(name))?.into_iter().next()) };
        let report = Report {
            name: root.attr("name").unwrap_or("report").to_string(),
            resource_bundle: root.attr("resourceBundle").filter(|b| !b.is_empty()).map(str::to_string),
            when_resource_missing: match root.attr("whenResourceMissingType") {
                Some("Empty") => WhenResourceMissing::Empty,
                Some("Key") => WhenResourceMissing::Key,
                Some("Error") => WhenResourceMissing::Error,
                _ => WhenResourceMissing::Null,
            },
            language: if self.groovy { Language::Groovy } else { Language::Java },
            page_width,
            page_height: num(&["pageHeight"], 842.0)?,
            landscape: attr(root, &["orientation"]) == Some("Landscape"),
            column_width: num(&["columnWidth"], page_width - left - right)?,
            column_count: num(&["columnCount"], 1.0)? as u32,
            column_spacing: num(&["columnSpacing"], 0.0)?,
            margins: Margins { left, right, top: num(&["topMargin"], 30.0)?, bottom: num(&["bottomMargin"], 30.0)? },
            when_no_data: match attr(root, &["whenNoDataType"]) {
                Some("BlankPage") => WhenNoData::BlankPage,
                Some("AllSectionsNoDetail") => WhenNoData::AllSectionsNoDetail,
                Some("NoDataSection") => WhenNoData::NoDataSection,
                _ => WhenNoData::NoPages,
            },
            title_new_page: attr(root, &["isTitleNewPage", "titleNewPage"]).is_some_and(parse_bool),
            summary_new_page: attr(root, &["isSummaryNewPage", "summaryNewPage"]).is_some_and(parse_bool),
            summary_with_page_header_and_footer: attr(
                root,
                &["isSummaryWithPageHeaderAndFooter", "summaryWithPageHeaderAndFooter"],
            )
            .is_some_and(parse_bool),
            float_column_footer: attr(root, &["isFloatColumnFooter", "floatColumnFooter"]).is_some_and(parse_bool),
            parameters,
            fields,
            variables,
            groups,
            background: band(&mut self, "background")?,
            title: band(&mut self, "title")?,
            page_header: band(&mut self, "pageHeader")?,
            column_header: band(&mut self, "columnHeader")?,
            detail: self.section_bands(root.child("detail"))?,
            column_footer: band(&mut self, "columnFooter")?,
            page_footer: band(&mut self, "pageFooter")?,
            last_page_footer: band(&mut self, "lastPageFooter")?,
            summary: band(&mut self, "summary")?,
            no_data: band(&mut self, "noData")?,
            datasets: Vec::new(),
            query,
            uses_data_source: false,
            horizontal: attr(root, &["printOrder"]) == Some("Horizontal")
                && attr(root, &["columnCount"]).and_then(number::<u32>).unwrap_or(1) > 1,
            pdf: {
                let property = |name: &str| {
                    root.children_named("property")
                        .find(|p| p.attr("name") == Some(&*format!("net.sf.jasperreports.export.pdf.{name}")))
                        .and_then(|p| p.attr("value"))
                };
                let offset = |name: &str| property(name).and_then(number::<f32>).unwrap_or(0.0);
                PdfSettings {
                    page_offsets: [
                        [offset("odd.page.offset.x"), offset("odd.page.offset.y")],
                        [offset("even.page.offset.x"), offset("even.page.offset.y")],
                    ],
                    tagged: property("tagged").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")),
                    language: property("tag.language").map(|v| v.trim().replace('_', "-")).filter(|v| !v.is_empty()),
                    pdfa: root
                        .children_named("property")
                        .find(|p| p.attr("name") == Some("net.sf.jasperreports.export.pdfa.conformance"))
                        .and_then(|p| p.attr("value"))
                        .map(str::trim)
                        .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("none"))
                        .map(|v| {
                            crate::pdf::PdfA::parse(v).ok_or_else(|| {
                                Error::template(format!(
                                    "net.sf.jasperreports.export.pdfa.conformance=\"{v}\": say pdfa2b, pdfa2u, pdfa2a, pdfa3b, pdfa3u or pdfa3a"
                                ))
                            })
                        })
                        .transpose()?,
                    encryption: if property("encrypted").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")) {
                        let permissions = |name: &str| {
                            property(name).map(crate::crypt::Permissions::parse).transpose().map(|p| p.unwrap_or_default().0)
                        };
                        let allowed = permissions("permissions.allowed")?;
                        let denied = permissions("permissions.denied")?;
                        Some(crate::crypt::Encryption {
                            user_password: property("user.password").unwrap_or("").to_string(),
                            owner_password: property("owner.password").map(str::to_string),
                            permissions: crate::crypt::Permissions(allowed & !denied),
                        })
                    } else {
                        None
                    },
                }
            },
            warnings: Vec::new(),
        };

        let mut report = report;
        // Paragraph settings have no place in the model in 1.x: they go in a hidden parameter.
        if !self.paragraphs.is_empty() {
            let table: serde_json::Map<String, serde_json::Value> =
                self.paragraphs.iter().map(|(key, p)| (format!("{key:016x}"), p.to_json())).collect();
            report.parameters.push(Parameter {
                name: crate::text::PARAGRAPHS.to_string(),
                class: JavaType::Object,
                default: Some(Expr::Lit(crate::value::Value::Json(std::sync::Arc::new(serde_json::Value::Object(table))))),
            });
        }
        let mut datasets: Vec<Dataset> = self.datasets.drain().map(|(_, d)| d).collect();
        datasets.sort_by(|a, b| a.name.cmp(&b.name));
        report.datasets = datasets;
        self.warnings.dedup();
        report.warnings = self.warnings;
        Ok(report)
    }

    /// Parameters, fields, variables and groups of the report or of a `<subDataset>`.
    fn dataset(&mut self, node: &Node) -> Result<Dataset> {
        let mut parameters = Vec::new();
        for p in node.children_named("parameter") {
            parameters.push(Parameter {
                name: p.attr("name").unwrap_or_default().to_string(),
                class: JavaType::from_class_name(p.attr("class").unwrap_or("java.lang.String")),
                default: self.child_expr(p, &["defaultValueExpression"])?,
            });
        }
        let fields = node
            .children_named("field")
            .map(|f| Field {
                name: f.attr("name").unwrap_or_default().to_string(),
                class: JavaType::from_class_name(f.attr("class").unwrap_or("java.lang.String")),
            })
            .collect();
        let mut variables = Vec::new();
        for v in node.children_named("variable") {
            variables.push(self.variable(v)?);
        }
        let mut groups = Vec::new();
        for g in node.children_named("group") {
            groups.push(Group {
                name: g.attr("name").unwrap_or_default().to_string(),
                expression: self.child_expr(g, &["groupExpression", "expression"])?,
                start_new_page: attr(g, &["isStartNewPage", "startNewPage"]).is_some_and(parse_bool),
                start_new_column: attr(g, &["isStartNewColumn", "startNewColumn"]).is_some_and(parse_bool),
                reprint_header_on_each_page: attr(g, &["isReprintHeaderOnEachPage", "reprintHeaderOnEachPage"])
                    .is_some_and(parse_bool),
                min_height_to_start_new_page: attr(g, &["minHeightToStartNewPage"]).and_then(number).unwrap_or(0.0),
                header: self.section_bands(g.child("groupHeader"))?,
                footer: self.section_bands(g.child("groupFooter"))?,
            });
        }
        let query = node
            .child("queryString")
            .or_else(|| node.child("query"))
            .map(|q| Query { language: q.attr("language").unwrap_or_default().to_string(), text: q.text.trim().to_string() });
        if let Some(q) = &query {
            if !q.language.is_empty() && !q.language.eq_ignore_ascii_case("sql") && !q.text.is_empty() {
                self.warnings.push(format!("{} query is not run (only SQL is): pass the rows it gives", q.language));
            }
        }
        Ok(Dataset { name: node.attr("name").unwrap_or_default().to_string(), parameters, fields, variables, groups, query })
    }

    /// A `<datasetRun>` (of a table or list).
    fn dataset_run(&mut self, run: Option<&Node>, what: &str) -> Result<DatasetRun> {
        let dataset = match run.and_then(|r| r.attr("subDataset")) {
            Some(name) => match self.datasets.get(name) {
                Some(d) => Some(d.clone()),
                None => {
                    self.warnings.push(format!("{what}: unknown subdataset `{name}`"));
                    None
                }
            },
            None => None,
        };
        let mut parameters = Vec::new();
        let (mut parameters_map, mut data_source, mut connection) = (None, None, false);
        if let Some(r) = run {
            for c in r.children.iter().filter(|c| c.name == "datasetParameter" || c.name == "parameter") {
                if let Some(e) = self.child_expr(c, &["datasetParameterExpression", "expression"])? {
                    parameters.push((c.attr("name").unwrap_or_default().to_string(), e));
                }
            }
            connection = r.child("connectionExpression").is_some() && r.child("dataSourceExpression").is_none();
            parameters_map = self.child_expr(r, &["parametersMapExpression"])?;
            data_source = self.child_expr(r, &["dataSourceExpression"])?;
        }
        Ok(DatasetRun {
            dataset,
            parameters,
            parameters_map,
            data_source,
            connection,
            page_height: self.page_height,
            language: if self.groovy { Language::Groovy } else { Language::Java },
        })
    }

    /// A crosstab (`<crosstab>`, or `<element kind="crosstab">` in JRXML 7).
    fn crosstab(&mut self, n: &Node) -> Result<CrosstabElement> {
        let dataset = n.child("crosstabDataset").and_then(|d| d.child("dataset")).or_else(|| n.child("dataset"));
        let run = match dataset {
            Some(d) => {
                if matches!(attr(d, &["resetType"]), Some(t) if t != "Report") {
                    self.warnings.push("crosstab datasets reset by group or page use all the rows printed so far".into());
                }
                match d.child("datasetRun") {
                    Some(r) => Some(self.dataset_run(Some(r), "crosstab")?),
                    None => None,
                }
            }
            None => None,
        };
        let mut parameters = Vec::new();
        for p in n.children.iter().filter(|c| c.name == "crosstabParameter" || c.name == "parameter") {
            if let Some(e) = self.child_expr(p, &["parameterValueExpression", "expression"])? {
                parameters.push((p.attr("name").unwrap_or_default().to_string(), e));
            }
        }
        if n.child("parametersMapExpression").is_some() {
            self.warnings.push("crosstab parametersMapExpression is ignored".into());
        }
        let mut row_groups = Vec::new();
        let mut column_groups = Vec::new();
        for g in &n.children {
            let row = g.name == "rowGroup";
            if !row && g.name != "columnGroup" {
                continue;
            }
            let size_attr = if row { "width" } else { "height" };
            let size = g.attr(size_attr).map(|v| parse_f32(v, size_attr)).transpose()?.unwrap_or(0.0);
            let total_position = match g.attr("totalPosition") {
                Some("Start") => TotalPosition::Start,
                Some("End") => TotalPosition::End,
                _ => TotalPosition::None,
            };
            let header_position = match g.attr("headerPosition").or_else(|| g.attr("position")) {
                Some("Middle" | "Center") => HeaderPosition::Middle,
                Some("Bottom" | "Right") => HeaderPosition::End,
                Some("Stretch") => HeaderPosition::Stretch,
                _ => HeaderPosition::Start,
            };
            let bucket = g.child("bucket");
            if bucket.is_some_and(|b| b.child("comparatorExpression").is_some()) {
                self.warnings.push("crosstab bucket comparators are ignored (values are sorted naturally)".into());
            }
            let header_node = |names: &[&str]| {
                g.children
                    .iter()
                    .find(|c| names.contains(&c.name.as_str()))
                    .and_then(|h| h.child("cellContents").or_else(|| h.child("contents")).or(Some(h)))
            };
            let (header_names, total_names): (&[&str], &[&str]) = if row {
                (&["crosstabRowHeader", "header"], &["crosstabTotalRowHeader", "totalHeader"])
            } else {
                (&["crosstabColumnHeader", "header"], &["crosstabTotalColumnHeader", "totalHeader"])
            };
            let header = match header_node(header_names) {
                Some(c) => self.cell_contents(c)?,
                None => None,
            };
            let total_header = match header_node(total_names) {
                Some(c) => self.cell_contents(c)?,
                None => None,
            };
            let group = CrosstabGroup {
                name: g.attr("name").unwrap_or_default().to_string(),
                size,
                total_position,
                header_position,
                bucket: match bucket {
                    Some(b) => self.child_expr(b, &["bucketExpression", "expression"])?,
                    None => None,
                },
                descending: bucket.and_then(|b| b.attr("order")) == Some("Descending"),
                order_by: match bucket {
                    Some(b) => self.child_expr(b, &["orderByExpression"])?,
                    None => None,
                },
                class: JavaType::from_class_name(bucket.and_then(|b| b.attr("class")).unwrap_or("java.lang.String")),
                header,
                total_header,
            };
            if row {
                row_groups.push(group);
            } else {
                column_groups.push(group);
            }
        }
        let mut measures = Vec::new();
        for m in n.children_named("measure") {
            let calculation = match m.attr("calculation") {
                Some("Count") => Calculation::Count,
                Some("DistinctCount") => Calculation::DistinctCount,
                Some("Sum") => Calculation::Sum,
                Some("Average") => Calculation::Average,
                Some("Lowest") => Calculation::Lowest,
                Some("Highest") => Calculation::Highest,
                Some("First") => Calculation::First,
                Some("StandardDeviation") => Calculation::StandardDeviation,
                Some("Variance") => Calculation::Variance,
                _ => Calculation::Nothing,
            };
            measures.push(CrosstabMeasure {
                name: m.attr("name").unwrap_or_default().to_string(),
                class: JavaType::from_class_name(m.attr("class").unwrap_or("java.lang.Object")),
                calculation,
                expr: self.child_expr(m, &["measureExpression", "expression"])?,
                percentage: m.attr("percentageOf").or_else(|| m.attr("percentageType")) == Some("GrandTotal"),
            });
        }
        let mut cells = Vec::new();
        for c in n.children_named("crosstabCell").chain(n.children_named("cell")) {
            let num = |a: &str| c.attr(a).map(|v| parse_f32(v, a)).transpose();
            let contents = match c.child("cellContents").or_else(|| c.child("contents")) {
                Some(cc) => self.cell_contents(cc)?,
                None => None,
            };
            cells.push(CrosstabCell {
                row_total: c.attr("rowTotalGroup").map(str::to_string),
                column_total: c.attr("columnTotalGroup").map(str::to_string),
                width: num("width")?,
                height: num("height")?,
                contents,
            });
        }
        let special = |name: &str, alt: &str| {
            // JRXML 6 wraps the contents in <cellContents>; JasperReports 7 writes them in the
            // <headerCell> or <whenNoDataCell> itself.
            n.child(name).or_else(|| n.child(alt)).map(|h| h.child("cellContents").or_else(|| h.child("contents")).unwrap_or(h))
        };
        let header_cell = match special("crosstabHeaderCell", "headerCell") {
            Some(c) => self.cell_contents(c)?,
            None => None,
        };
        let no_data_cell = match special("whenNoDataCell", "whenNoDataCell") {
            Some(c) => self.cell_contents(c)?,
            None => None,
        };
        let flag = |names: &[&str]| attr(n, names).map(parse_bool).unwrap_or(true);
        Ok(CrosstabElement {
            run,
            parameters,
            row_groups,
            column_groups,
            measures,
            cells,
            header_cell,
            no_data_cell,
            column_break_offset: attr(n, &["columnBreakOffset"]).and_then(number).unwrap_or(10.0),
            repeat_column_headers: flag(&["isRepeatColumnHeaders", "repeatColumnHeaders"]),
            repeat_row_headers: flag(&["isRepeatRowHeaders", "repeatRowHeaders"]),
        })
    }

    /// The `<cellContents>` of a crosstab cell as a frame (sized when the cell is placed).
    fn cell_contents(&mut self, c: &Node) -> Result<Option<Element>> {
        let mut re_attrs = vec![
            ("x".to_string(), "0".to_string()),
            ("y".to_string(), "0".to_string()),
            ("width".to_string(), "0".to_string()),
            ("height".to_string(), "0".to_string()),
        ];
        for a in ["style", "mode", "backcolor"] {
            if let Some(v) = c.attr(a) {
                re_attrs.push((a.to_string(), v.to_string()));
            }
        }
        // Cell contents are transparent unless they say otherwise (or their style does).
        if c.attr("mode").is_none() && c.attr("style").is_none() {
            re_attrs.push(("mode".to_string(), "Transparent".to_string()));
        }
        let mut children =
            vec![Node { name: "reportElement".into(), attrs: re_attrs, children: Vec::new(), text: String::new() }];
        children.extend(c.children.iter().cloned());
        let frame_node = Node { name: "frame".into(), attrs: Vec::new(), children, text: String::new() };
        Ok(Some(self.element(&frame_node, "frame")?))
    }

    /// A list component (`jr:list`, or `<component kind="list">` in JRXML 7).
    fn list(&mut self, l: &Node) -> Result<ListElement> {
        let run = self.dataset_run(l.child("datasetRun"), "list")?;
        let node = l.child("listContents").or_else(|| l.child("contents"));
        let contents = match node {
            Some(c) => self.band(c)?,
            None => Band { height: 0.0, split_type: SplitType::Stretch, print_when: None, elements: Vec::new() },
        };
        let across = (attr(l, &["printOrder"]) == Some("Horizontal"))
            .then(|| node.and_then(|c| attr(c, &["width"])).and_then(number::<f32>))
            .flatten()
            .filter(|w| *w > 0.0);
        Ok(ListElement { run, contents, across })
    }

    /// A table component (`jr:table`, or `<component kind="table">` in JRXML 7).
    fn table(&mut self, t: &Node, element_width: f32) -> Result<TableElement> {
        let run = self.dataset_run(t.child("datasetRun"), "table")?;
        let mut columns = Vec::new();
        for c in &t.children {
            if let Some(col) = self.table_column(c)? {
                columns.push(col);
            }
        }
        let no_data = match t.child("noData") {
            Some(n) => Some(self.table_cell(n, element_width)?),
            None => None,
        };
        Ok(TableElement {
            run,
            columns,
            when_no_data: match t.attr("whenNoDataType") {
                Some("AllSectionsNoDetail") => WhenNoData::AllSectionsNoDetail,
                Some("NoDataCell") => WhenNoData::NoDataSection,
                _ => WhenNoData::NoPages,
            },
            no_data,
        })
    }

    /// `<jr:column>` / `<jr:columnGroup>` (JRXML 6) or `<column kind="single|group">` (JRXML 7).
    fn table_column(&mut self, c: &Node) -> Result<Option<TableColumn>> {
        let is_group = match (c.name.as_str(), c.attr("kind")) {
            ("columnGroup", _) | ("column", Some("group")) => true,
            ("column", _) => false,
            _ => return Ok(None),
        };
        let width = c.attr("width").map(|w| parse_f32(w, "column width")).transpose()?.unwrap_or(0.0);
        let mut children = Vec::new();
        if is_group {
            for sub in &c.children {
                if let Some(col) = self.table_column(sub)? {
                    children.push(col);
                }
            }
        }
        let cell = |p: &mut Parser, name: &str| -> Result<Option<TableCell>> {
            match c.child(name) {
                Some(n) => p.table_cell(n, width).map(Some),
                None => Ok(None),
            }
        };
        let table_header = cell(self, "tableHeader")?;
        let table_footer = cell(self, "tableFooter")?;
        let column_header = cell(self, "columnHeader")?;
        let column_footer = cell(self, "columnFooter")?;
        let detail = cell(self, "detailCell")?;
        let group_cells = |p: &mut Parser, name: &str| -> Result<Vec<(String, TableCell)>> {
            let mut out = Vec::new();
            for g in c.children_named(name) {
                // JRXML 6 wraps the cell: <groupHeader groupName><cell>; JRXML 7 may not.
                let node = g.child("cell").unwrap_or(g);
                out.push((g.attr("groupName").unwrap_or_default().to_string(), p.table_cell(node, width)?));
            }
            Ok(out)
        };
        let group_headers = group_cells(self, "groupHeader")?;
        let group_footers = group_cells(self, "groupFooter")?;
        Ok(Some(TableColumn {
            width,
            print_when: self.child_expr(c, &["printWhenExpression"])?,
            children,
            is_group,
            table_header,
            table_footer,
            column_header,
            column_footer,
            detail,
            group_headers,
            group_footers,
        }))
    }

    /// A cell becomes a frame with the cell's style and box, holding the cell's elements, as in
    /// Jasper's `TableReport.createCell`. A cell holding only a text element that fills it,
    /// with no box and no background of its own, is that text element (`createCellElement`).
    fn table_cell(&mut self, c: &Node, width: f32) -> Result<TableCell> {
        let height = c.attr("height").map(|h| parse_f32(h, "cell height")).transpose()?.unwrap_or(0.0);
        let mut re_attrs = vec![
            ("x".to_string(), "0".to_string()),
            ("y".to_string(), "0".to_string()),
            ("width".to_string(), width.to_string()),
            ("height".to_string(), height.to_string()),
            ("positionType".to_string(), "Float".to_string()),
            ("stretchType".to_string(), "RelativeToTallestObject".to_string()),
        ];
        if let Some(style) = c.attr("style") {
            re_attrs.push(("style".to_string(), style.to_string()));
        }
        let mut children =
            vec![Node { name: "reportElement".into(), attrs: re_attrs, children: Vec::new(), text: String::new() }];
        children.extend(c.children.iter().filter(|k| k.name != "printWhenExpression").cloned());
        let frame_node = Node { name: "frame".into(), attrs: Vec::new(), children, text: String::new() };
        let frame = self.element(&frame_node, "frame")?;
        if let ElementKind::Frame { children, boxed } = &frame.kind {
            let no_box = [boxed.top, boxed.left, boxed.bottom, boxed.right].iter().all(|p| p.width <= 0.0)
                && boxed.padding == Padding::default();
            if let [only] = children.as_slice() {
                let fills = only.x == 0.0 && only.y == 0.0 && only.width == width && only.height == height;
                let cell_transparent = frame.mode == Mode::Transparent;
                if fills && no_box && cell_transparent && matches!(only.kind, ElementKind::Text(_)) {
                    let mut el = only.clone();
                    el.stretch_type = StretchType::RelativeToTallestObject;
                    return Ok(TableCell { height, element: el });
                }
            }
        }
        Ok(TableCell { height, element: frame })
    }

    fn variable(&self, v: &Node) -> Result<Variable> {
        let reset = |kind: Option<&str>, group: Option<&str>, default: ResetType| match kind {
            Some("None") => ResetType::None,
            Some("Report") => ResetType::Report,
            Some("Page") => ResetType::Page,
            Some("Column") => ResetType::Column,
            Some("Group") => ResetType::Group(group.unwrap_or_default().to_string()),
            _ => default,
        };
        Ok(Variable {
            name: v.attr("name").unwrap_or_default().to_string(),
            class: JavaType::from_class_name(v.attr("class").unwrap_or("java.lang.String")),
            calculation: calculation(v.attr("calculation")),
            reset: reset(v.attr("resetType"), v.attr("resetGroup"), ResetType::Report),
            increment: reset(v.attr("incrementType"), v.attr("incrementGroup"), ResetType::None),
            expression: self.child_expr(v, &["variableExpression", "expression"])?,
            initial: self.child_expr(v, &["initialValueExpression"])?,
        })
    }
}

/// The barcode4j/ZXing component inside a `componentElement` (JRXML 6) or a
/// A JRXML 7 chart (`<element kind="chart" chartType="bar">`, or the `<chart>` of a multiple
/// axis chart's `<axis>`) in the shape JRXML 6 gives it (`<barChart><chart>...`), which the
/// chart reader understands.
fn v7_chart(e: &Node) -> Node {
    let node = |name: &str, attrs: Vec<(String, String)>, children: Vec<Node>| Node {
        name: name.to_string(),
        attrs,
        children,
        text: String::new(),
    };
    let pick = |n: &Node, map: &[(&str, &str)]| -> Vec<(String, String)> {
        map.iter().filter_map(|(from, to)| n.attr(from).map(|v| (to.to_string(), v.to_string()))).collect()
    };
    let renamed = |n: &Node, name: &str| Node { name: name.to_string(), ..n.clone() };
    let chart_type = e.attr("chartType").unwrap_or("bar");

    // <chart>: what every chart has.
    const CHART_ATTRS: &[&str] = &[
        "kind",
        "chartType",
        "showLegend",
        "evaluationTime",
        "evaluationGroup",
        "customizerClass",
        "theme",
        "renderType",
        "titlePosition",
        "titleColor",
        "subtitleColor",
        "legendColor",
        "legendBackgroundColor",
        "legendPosition",
    ];
    let geometry: Vec<(String, String)> = e.attrs.iter().filter(|(k, _)| !CHART_ATTRS.contains(&k.as_str())).cloned().collect();
    let element_children: Vec<Node> = e
        .children
        .iter()
        .filter(|c| matches!(c.name.as_str(), "property" | "propertyExpression" | "printWhenExpression"))
        .cloned()
        .collect();
    let mut chart_children = vec![node("reportElement", geometry, element_children)];
    if let Some(b) = e.child("box") {
        chart_children.push(b.clone());
    }
    let mut title = Vec::new();
    if let Some(f) = e.child("titleFont") {
        title.push(renamed(f, "font"));
    }
    if let Some(x) = e.child("titleExpression") {
        title.push(x.clone());
    }
    chart_children.push(node("chartTitle", pick(e, &[("titlePosition", "position"), ("titleColor", "color")]), title));
    let mut subtitle = Vec::new();
    if let Some(f) = e.child("subtitleFont") {
        subtitle.push(renamed(f, "font"));
    }
    if let Some(x) = e.child("subtitleExpression") {
        subtitle.push(x.clone());
    }
    chart_children.push(node("chartSubtitle", pick(e, &[("subtitleColor", "color")]), subtitle));
    let legend = e.child("legendFont").map(|f| vec![renamed(f, "font")]).unwrap_or_default();
    chart_children.push(node(
        "chartLegend",
        pick(e, &[("legendColor", "textColor"), ("legendBackgroundColor", "backgroundColor"), ("legendPosition", "position")]),
        legend,
    ));
    let chart = node(
        "chart",
        pick(
            e,
            &[
                ("showLegend", "isShowLegend"),
                ("evaluationTime", "evaluationTime"),
                ("evaluationGroup", "evaluationGroup"),
                ("customizerClass", "customizerClass"),
                ("theme", "theme"),
                ("renderType", "renderType"),
            ],
        ),
        chart_children,
    );
    let mut out = vec![chart];

    // <xxxDataset><dataset .../>series...</xxxDataset>
    if let Some(d) = e.child("dataset") {
        let kind = d.attr("kind").unwrap_or("category");
        let inner_children: Vec<Node> =
            d.children.iter().filter(|c| matches!(c.name.as_str(), "incrementWhenExpression" | "datasetRun")).cloned().collect();
        let inner = node(
            "dataset",
            d.attrs.iter().filter(|(k, _)| !matches!(k.as_str(), "kind" | "timePeriod")).cloned().collect(),
            inner_children,
        );
        let mut children = vec![inner];
        children
            .extend(d.children.iter().filter(|c| !matches!(c.name.as_str(), "incrementWhenExpression" | "datasetRun")).cloned());
        out.push(node(&format!("{kind}Dataset"), pick(d, &[("timePeriod", "timePeriod")]), children));
    }

    // <xxxPlot ...><plot .../>...</xxxPlot>
    if let Some(p) = e.child("plot") {
        let mut children = Vec::new();
        let mut plot_children = Vec::new();
        let mut formats: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for (k, v) in &p.attrs {
            // JasperReports 7 writes the x and y axes in lower case (xaxisTickLabelMask).
            let k = &match k.strip_prefix("xaxis").or_else(|| k.strip_prefix("yaxis")) {
                Some(rest) => format!("{}Axis{rest}", &k[..1]),
                None => k.clone(),
            };
            // valueAxisTickLabelMask="..." -> <valueAxisFormat><axisFormat tickLabelMask="..."/>
            if let Some(i) = k.find("Axis").filter(|_| k != "Axis") {
                let (axis, rest) = (&k[..i], &k[i + 4..]);
                let attr = match rest {
                    "TickLabelMask" => "tickLabelMask",
                    "TickLabelColor" => "tickLabelColor",
                    "LabelColor" => "labelColor",
                    "LineColor" => "axisLineColor",
                    "VerticalTickLabels" => "verticalTickLabels",
                    "TickLabelRotation" => "tickLabelRotation",
                    _ => continue,
                };
                let name = format!("{axis}AxisFormat");
                match formats.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, attrs)) => attrs.push((attr.into(), v.clone())),
                    None => formats.push((name, vec![(attr.into(), v.clone())])),
                }
            }
        }
        for (name, attrs) in formats {
            children.push(node(&name, Vec::new(), vec![node("axisFormat", attrs, Vec::new())]));
        }
        for c in &p.children {
            match c.name.as_str() {
                "seriesColor" => plot_children.push(Node {
                    attrs: c
                        .attrs
                        .iter()
                        .map(|(k, v)| (if k == "order" { "seriesOrder".into() } else { k.clone() }, v.clone()))
                        .collect(),
                    ..c.clone()
                }),
                // JRXML 7 puts a meter's tick label font on <tickLabelFont> itself; JRXML 6 in
                // a <font> inside it.
                "tickLabelFont" => children.push(node("tickLabelFont", Vec::new(), vec![renamed(c, "font")])),
                "axis" => {
                    let position =
                        c.attr("position").or_else(|| c.child("position").map(|p| p.text.trim())).unwrap_or("leftOrTop");
                    let inner = c.child("chart").map(v7_chart).into_iter().collect();
                    children.push(node("axis", vec![("position".into(), position.to_string())], inner));
                }
                _ => children.push(c.clone()),
            }
        }
        children.insert(0, node("plot", p.attrs.clone(), plot_children));
        out.push(node(&format!("{chart_type}Plot"), p.attrs.clone(), children));
    }
    node(&format!("{chart_type}Chart"), Vec::new(), out)
}

/// A meter's plot as JasperReports hands it to JFreeChart's `MeterPlot`: only what the
/// template sets, and `MeterPlot`'s own defaults for the rest. A black dial (`meterColor`,
/// not the plot's backcolor, which the meter does not draw), white ticks, a green needle, the
/// value in yellow SansSerif bold 12 and the tick labels in SansSerif bold 10.
fn meter_defaults(d: &mut crate::chart::DialPlot, pn: &Node, font: &FontSpec) {
    let sans = |size: f32| FontSpec {
        name: "SansSerif".into(),
        size,
        bold: true,
        italic: false,
        underline: false,
        strike: false,
        pdf_font: None,
    };
    let display = pn.child("valueDisplay");
    d.background = pn.attr("meterColor").and_then(parse_color);
    d.needle = pn.attr("needleColor").and_then(parse_color).unwrap_or(Color { r: 0, g: 0xFF, b: 0 });
    d.tick_color = pn.attr("tickColor").and_then(parse_color).unwrap_or(Color::WHITE);
    d.value_color = display.and_then(|d| d.attr("color")).and_then(parse_color).unwrap_or(Color { r: 0xFF, g: 0xFF, b: 0 });
    d.value_font = match display.filter(|d| d.child("font").is_some()) {
        Some(_) => font_from(display, font),
        None => sans(12.0),
    };
    d.tick_font = match pn.child("tickLabelFont").filter(|t| t.child("font").is_some()) {
        Some(t) => font_from(Some(t), font),
        None => sans(10.0),
    };
}

/// `<element kind="component">` (JRXML 7), when this engine can draw it.
fn barcode_node(n: &Node) -> Option<&Node> {
    const SUPPORTED: &[&str] = &[
        "QRCode",
        "Code128",
        "Code39",
        "Interleaved2Of5",
        "Codabar",
        "EAN13",
        "EAN8",
        "UPCA",
        "UPCE",
        "EAN128",
        "DataMatrix",
        "POSTNET",
        "RoyalMailCustomer",
        "USPSIntelligentMail",
        "PDF417",
        "barbecue",
    ];
    n.children.iter().find(|c| {
        SUPPORTED.contains(&c.name.as_str())
            || (c.name == "component" && c.attr("kind").is_some_and(|k| SUPPORTED.contains(&k.rsplit(':').next().unwrap_or(k))))
    })
}

/// The table component inside a `componentElement` (JRXML 6) or a `<element kind="component">`
/// (JRXML 7).
fn table_node(n: &Node) -> Option<&Node> {
    component_node(n, "table")
}

/// The component called `name` inside a component element (JRXML 6 `<jr:name>` or JRXML 7
/// `<component kind="name">`).
fn component_node<'a>(n: &'a Node, name: &str) -> Option<&'a Node> {
    n.children.iter().find(|c| c.name == name || (c.name == "component" && c.attr("kind") == Some(name)))
}

fn font_from(node: Option<&Node>, base: &FontSpec) -> FontSpec {
    let mut f = base.clone();
    if let Some(n) = node.and_then(|n| n.child("font")) {
        if let Some(v) = n.attr("fontName") {
            f.name = v.to_string();
        }
        if let Some(v) = n.attr("pdfFontName") {
            f.pdf_font = Some(v.to_string());
        }
        if let Some(v) = attr(n, &["size", "fontSize"]).and_then(number) {
            f.size = v;
        }
        f.bold = attr(n, &["isBold", "bold"]).map(parse_bool).unwrap_or(f.bold);
        f.italic = attr(n, &["isItalic", "italic"]).map(parse_bool).unwrap_or(f.italic);
    }
    f
}

fn calculation(name: Option<&str>) -> Calculation {
    match name.unwrap_or("Nothing") {
        "Count" => Calculation::Count,
        "DistinctCount" => Calculation::DistinctCount,
        "Sum" => Calculation::Sum,
        "Average" => Calculation::Average,
        "Lowest" => Calculation::Lowest,
        "Highest" => Calculation::Highest,
        "First" => Calculation::First,
        "StandardDeviation" => Calculation::StandardDeviation,
        "Variance" => Calculation::Variance,
        "System" => Calculation::System,
        _ => Calculation::Nothing,
    }
}

impl Parser {
    /// Bands of a section. JRXML 6 nests `<band>` elements; JRXML 7 may put `height` on the
    /// section itself.
    fn section_bands(&mut self, section: Option<&Node>) -> Result<Vec<Band>> {
        let Some(section) = section else { return Ok(Vec::new()) };
        let bands: Vec<&Node> = section.children_named("band").collect();
        if bands.is_empty() {
            if section.attr("height").is_some() {
                return Ok(vec![self.band(section)?]);
            }
            return Ok(Vec::new());
        }
        bands.into_iter().map(|b| self.band(b)).collect()
    }

    fn band(&mut self, b: &Node) -> Result<Band> {
        Ok(Band {
            height: b.attr("height").map(|h| parse_f32(h, "band height")).transpose()?.unwrap_or(0.0),
            split_type: match b.attr("splitType") {
                Some("Prevent") => SplitType::Prevent,
                Some("Immediate") => SplitType::Immediate,
                _ => SplitType::Stretch,
            },
            print_when: self.child_expr(b, &["printWhenExpression"])?,
            elements: self.elements(b)?,
        })
    }

    fn chart(&mut self, n: &Node, tag: &str, font: FontSpec, boxed: BoxStyle) -> Result<crate::chart::ChartElement> {
        use crate::chart::*;
        let kind = ChartKind::from_tag(tag).expect("checked by the caller");
        let chart = n.child("chart");
        let flag = |node: Option<&Node>, names: &[&str], default: bool| {
            node.and_then(|c| attr(c, names)).map(parse_bool).unwrap_or(default)
        };
        let color = |node: Option<&Node>, name: &str| node.and_then(|c| c.attr(name)).and_then(parse_color);
        let title = chart.and_then(|c| c.child("chartTitle"));
        let subtitle = chart.and_then(|c| c.child("chartSubtitle"));
        let legend = chart.and_then(|c| c.child("chartLegend"));
        let evaluation_time = match chart.and_then(|c| c.attr("evaluationTime")) {
            Some("Report") | Some("Auto") => EvaluationTime::Report,
            Some("Page") => EvaluationTime::Page,
            Some("Column") => EvaluationTime::Column,
            Some("Group") => EvaluationTime::Group,
            _ => EvaluationTime::Now,
        };
        if chart.and_then(|c| c.attr("customizerClass")).is_some() {
            self.warnings.push("chart customizers (customizerClass) are ignored".into());
        }

        // Dataset.
        let ds_node = n.children.iter().find(|c| c.name.ends_with("Dataset"));
        let ds = ds_node.and_then(|d| d.child("dataset"));
        let group_attr = |name: &str| ds.and_then(|d| d.attr(name)).unwrap_or_default().to_string();
        let reset = match ds.and_then(|d| d.attr("resetType")) {
            Some("Group") => ResetType::Group(group_attr("resetGroup")),
            Some("Page") => ResetType::Page,
            Some("None") => ResetType::None,
            _ => ResetType::Report,
        };
        let increment = match ds.and_then(|d| d.attr("incrementType")) {
            Some("Group") => Increment::Group(group_attr("incrementGroup")),
            Some("Page") => Increment::Page,
            Some("Report") => Increment::Report,
            _ => Increment::Record,
        };
        let run = match ds.and_then(|d| d.child("datasetRun")) {
            Some(r) => Some(Box::new(self.dataset_run(Some(r), "chart")?)),
            None => None,
        };
        let data_kind = ds_node.map(|d| DataKind::from_tag(&d.name)).unwrap_or(match kind {
            ChartKind::Pie | ChartKind::Pie3D => DataKind::Pie,
            ChartKind::XyLine | ChartKind::XyArea | ChartKind::Scatter | ChartKind::XyBar => DataKind::Xy,
            ChartKind::TimeSeries => DataKind::TimeSeries,
            ChartKind::Bubble => DataKind::Xyz,
            ChartKind::Candlestick | ChartKind::HighLow => DataKind::HighLow,
            ChartKind::Meter | ChartKind::Thermometer => DataKind::Value,
            ChartKind::Gantt => DataKind::Gantt,
            _ => DataKind::Category,
        });
        let time_period = TimePeriod::parse(ds_node.and_then(|d| d.attr("timePeriod")).unwrap_or("Day"));
        let mut series = Vec::new();
        if let Some(d) = ds_node {
            let read = |p: &mut Parser, s: &Node| -> Result<SeriesDef> {
                let e = |p: &mut Parser, names: &[&str]| p.child_expr(s, names);
                Ok(SeriesDef {
                    series: e(p, &["seriesExpression"])?,
                    key: e(p, &["keyExpression"])?,
                    category: e(p, &["categoryExpression"])?,
                    value: e(p, &["valueExpression"])?,
                    x: e(p, &["xValueExpression", "xvalueExpression"])?,
                    y: e(p, &["yValueExpression", "yvalueExpression"])?,
                    z: e(p, &["zValueExpression", "zvalueExpression"])?,
                    period: e(p, &["timePeriodExpression"])?,
                    date: e(p, &["dateExpression"])?,
                    high: e(p, &["highExpression"])?,
                    low: e(p, &["lowExpression"])?,
                    open: e(p, &["openExpression"])?,
                    close: e(p, &["closeExpression"])?,
                    volume: e(p, &["volumeExpression"])?,
                    task: e(p, &["taskExpression"])?,
                    subtask: e(p, &["subtaskExpression"])?,
                    start: e(p, &["startDateExpression"])?,
                    end: e(p, &["endDateExpression"])?,
                    percent: e(p, &["percentExpression"])?,
                })
            };
            let nested: Vec<&Node> = d
                .children
                .iter()
                .filter(|c| {
                    matches!(
                        c.name.as_str(),
                        "pieSeries"
                            | "categorySeries"
                            | "xySeries"
                            | "timeSeries"
                            | "timePeriodSeries"
                            | "xyzSeries"
                            | "ganttSeries"
                            | "series"
                    )
                })
                .collect();
            if nested.is_empty() {
                series.push(read(self, d)?);
            } else {
                for s in nested {
                    series.push(read(self, s)?);
                }
            }
        }

        // Plot.
        let plot_node = n.children.iter().find(|c| c.name.ends_with("Plot"));
        let plot = plot_node.and_then(|p| p.child("plot"));
        let mut series_colors: Vec<(i32, Color)> = plot
            .map(|p| {
                p.children
                    .iter()
                    .filter(|c| c.name == "seriesColor")
                    .filter_map(|c| Some((c.attr("seriesOrder")?.parse().ok()?, parse_color(c.attr("color")?)?)))
                    .collect()
            })
            .unwrap_or_default();
        series_colors.sort_by_key(|(o, _)| *o);
        let value_format = plot_node
            .and_then(|p| p.child("valueAxisFormat").or_else(|| p.child("rangeAxisFormat")))
            .and_then(|f| f.child("axisFormat"))
            .and_then(|f| f.attr("tickLabelMask"))
            .map(str::to_string);
        let mask = |names: &[&str]| {
            plot_node
                .and_then(|p| names.iter().find_map(|n| p.child(n)))
                .and_then(|f| f.child("axisFormat"))
                .and_then(|f| f.attr("tickLabelMask"))
                .map(str::to_string)
        };
        let time_format = mask(&["timeAxisFormat"]);
        let x_format = mask(&["xAxisFormat", "categoryAxisFormat"]);
        let axis_color = plot_node
            .and_then(|p| p.child("valueAxisFormat").or_else(|| p.child("rangeAxisFormat")))
            .and_then(|f| f.child("axisFormat"))
            .and_then(|f| f.attr("tickLabelColor").or_else(|| f.attr("labelColor")))
            .and_then(parse_color);
        let dial = match kind {
            ChartKind::Meter | ChartKind::Thermometer => match plot_node {
                Some(pn) => Some(Box::new(self.dial_plot(pn, &font, kind == ChartKind::Thermometer)?)),
                None => None,
            },
            _ => None,
        };
        let dial = match (dial, plot_node) {
            (Some(mut d), Some(pn)) if kind == ChartKind::Meter => {
                meter_defaults(&mut d, pn, &font);
                Some(d)
            }
            (d, _) => d,
        };
        let mut axes = Vec::new();
        if kind == ChartKind::MultiAxis {
            for a in plot_node.into_iter().flat_map(|p| p.children_named("axis")) {
                let right = a.attr("position").or_else(|| a.child("position").map(|p| p.text.trim())) == Some("rightOrBottom");
                if let Some(inner) = a.children.iter().find(|c| ChartKind::from_tag(&c.name).is_some()) {
                    axes.push((right, self.chart(inner, &inner.name, font.clone(), boxed)?));
                }
            }
        }
        let label = |p: &mut Parser, names: &[&str]| match plot_node {
            Some(pn) => p.child_expr(pn, names),
            None => Ok(None),
        };
        self.chart_count += 1;
        let settings = crate::chart::Settings {
            show_open_ticks: flag(plot_node, &["isShowOpenTicks", "showOpenTicks"], false),
            show_close_ticks: flag(plot_node, &["isShowCloseTicks", "showCloseTicks"], false),
            mercury_by_range: kind == ChartKind::Thermometer && plot_node.and_then(|p| p.attr("mercuryColor")).is_none(),
            foreground_alpha: plot.and_then(|p| p.attr("foregroundAlpha")).and_then(number::<f32>).unwrap_or(1.0),
            svg: chart
                .and_then(|c| c.attr("renderType"))
                .map(str::to_string)
                .or_else(|| self.chart_render_type.clone())
                .as_deref()
                == Some("svg"),
            meter_shape: match plot_node.and_then(|p| p.attr("shape")) {
                Some("chord") => crate::chart::MeterShape::Chord,
                Some("circle") => crate::chart::MeterShape::Circle,
                _ => crate::chart::MeterShape::Pie,
            },
        };
        let element = ChartElement {
            dial,
            axes,
            axis_color,
            time_format,
            x_format,
            show_volume: flag(plot_node, &["isShowVolume", "showVolume"], true),
            label_format: plot_node.and_then(|p| p.attr("labelFormat")).map(str::to_string),
            legend_label_format: plot_node.and_then(|p| p.attr("legendLabelFormat")).map(str::to_string),
            id: self.chart_count,
            kind,
            horizontal: plot.and_then(|p| p.attr("orientation")) == Some("Horizontal"),
            title: match title {
                Some(t) => self.child_expr(t, &["titleExpression"])?,
                None => None,
            },
            title_font: font_from(title, &font),
            title_color: color(title, "color").unwrap_or(Color::BLACK),
            subtitle: match subtitle {
                Some(t) => self.child_expr(t, &["subtitleExpression"])?,
                None => None,
            },
            subtitle_font: font_from(subtitle, &font),
            subtitle_color: color(subtitle, "color").unwrap_or(Color::BLACK),
            show_legend: flag(chart, &["isShowLegend", "showLegend"], true),
            legend_font: font_from(legend, &font),
            legend_color: color(legend, "textColor").unwrap_or(Color::BLACK),
            legend_back: color(legend, "backgroundColor"),
            legend_position: match legend.and_then(|l| l.attr("position")) {
                Some("Top") => Edge::Top,
                Some("Left") => Edge::Left,
                Some("Right") => Edge::Right,
                _ => Edge::Bottom,
            },
            font,
            evaluation_time,
            evaluation_group: chart.and_then(|c| c.attr("evaluationGroup")).map(str::to_string),
            dataset: DatasetDef {
                reset,
                increment,
                increment_when: match ds {
                    Some(d) => self.child_expr(d, &["incrementWhenExpression"])?,
                    None => None,
                },
                series,
                kind: data_kind,
                time_period,
                run,
            },
            category_axis_label: label(
                self,
                &["categoryAxisLabelExpression", "xAxisLabelExpression", "xaxisLabelExpression", "timeAxisLabelExpression"],
            )?,
            value_axis_label: label(self, &["valueAxisLabelExpression", "yAxisLabelExpression", "yaxisLabelExpression"])?,
            value_min: label(self, &["rangeAxisMinValueExpression"])?,
            value_max: label(self, &["rangeAxisMaxValueExpression"])?,
            series_colors: series_colors.into_iter().map(|(_, c)| c).collect(),
            show_labels: flag(plot_node, &["isShowLabels", "showLabels"], true),
            show_lines: flag(plot_node, &["isShowLines", "showLines"], true),
            show_shapes: flag(plot_node, &["isShowShapes", "showShapes"], true),
            backcolor: color(chart.and_then(|c| c.child("reportElement")), "backcolor")
                .filter(|_| chart.and_then(|c| c.child("reportElement")).and_then(|r| r.attr("mode")) == Some("Opaque")),
            plot_backcolor: color(plot, "backcolor"),
            value_format,
            boxed,
        };
        crate::chart::set_settings(&element, settings);
        Ok(element)
    }

    /// The plot of a meter or thermometer (`<meterPlot>`, `<thermometerPlot>`).
    fn dial_plot(&mut self, pn: &Node, font: &FontSpec, thermometer: bool) -> Result<crate::chart::DialPlot> {
        use crate::chart::*;
        let range = |p: &mut Parser, n: Option<&Node>| -> Result<(Option<Expr>, Option<Expr>)> {
            match n.and_then(|n| n.child("dataRange")) {
                Some(r) => Ok((p.child_expr(r, &["lowExpression"])?, p.child_expr(r, &["highExpression"])?)),
                None => Ok((None, None)),
            }
        };
        let (low, high) = range(self, Some(pn))?;
        let mut intervals = Vec::new();
        for i in pn.children.iter().filter(|c| c.name == "meterInterval" || c.name == "interval") {
            let (l, h) = range(self, Some(i))?;
            intervals.push(Interval {
                label: i.attr("label").unwrap_or_default().to_string(),
                color: i.attr("color").or_else(|| i.attr("backgroundColor")).and_then(parse_color),
                alpha: i.attr("alpha").and_then(number).unwrap_or(1.0),
                low: l,
                high: h,
            });
        }
        // Thermometers: always the low, medium and high ranges, in that order (without
        // expressions when the template leaves one out). JasperReports makes them JFreeChart's
        // critical, warning and normal subranges, whose colors are fixed: red, orange
        // (Color.ORANGE) and green.
        // JRXML 6 wraps a range's expressions in a <dataRange>; JRXML 7 does not.
        for (name, color) in [("lowRange", 0xFF0000), ("mediumRange", 0xFFC800), ("highRange", 0x00FF00)] {
            let r = pn.child(name);
            if r.is_some() || thermometer {
                let (l, h) = match r.filter(|r| r.child("dataRange").is_none()) {
                    Some(r) => (self.child_expr(r, &["lowExpression"])?, self.child_expr(r, &["highExpression"])?),
                    None => range(self, r)?,
                };
                let c = Color { r: (color >> 16) as u8, g: (color >> 8) as u8, b: color as u8 };
                intervals.push(Interval { label: String::new(), color: Some(c), alpha: 1.0, low: l, high: h });
            }
        }
        let display = pn.child("valueDisplay");
        // A thermometer's value is white in SansSerif bold 16 (JFreeChart's ThermometerPlot)
        // unless the template says otherwise; JasperReports' mask defaults to the locale's
        // number format.
        let value_color = display.and_then(|d| d.attr("color")).and_then(parse_color);
        let value_color = value_color.unwrap_or(if thermometer { Color::WHITE } else { Color::BLACK });
        let value_font = match display.and_then(|d| d.child("font")) {
            None if thermometer => FontSpec {
                name: "SansSerif".into(),
                size: 16.0,
                bold: true,
                italic: false,
                underline: false,
                strike: false,
                ..font.clone()
            },
            _ => font_from(display, font),
        };
        let plot = pn.child("plot").unwrap_or(pn);
        Ok(DialPlot {
            low,
            high,
            intervals,
            units: pn.attr("units").unwrap_or_default().to_string(),
            tick_interval: pn.attr("tickInterval").and_then(number).unwrap_or(10.0),
            needle: pn.attr("needleColor").and_then(parse_color).unwrap_or(Color { r: 0x80, g: 0x80, b: 0x80 }),
            tick_color: pn.attr("tickColor").and_then(parse_color).unwrap_or(Color::BLACK),
            background: pn.attr("meterColor").or_else(|| plot.attr("backcolor")).and_then(parse_color),
            angle: pn.attr("meterAngle").or_else(|| pn.attr("angle")).and_then(number).unwrap_or(180.0),
            value_color,
            value_mask: display.and_then(|d| d.attr("mask")).map(str::to_string),
            value_font,
            // JFreeChart's meter labels its ticks in a bold 10-point font unless told otherwise.
            tick_font: match pn.child("tickLabelFont") {
                Some(t) => font_from(Some(t), font),
                None => FontSpec { size: 10.0, bold: true, ..font.clone() },
            },
            // Without a mercuryColor a thermometer's mercury takes its range's color, and
            // JFreeChart's light gray outside the ranges (`Settings::mercury_by_range`).
            mercury: pn.attr("mercuryColor").and_then(parse_color).unwrap_or(if thermometer {
                Color { r: 0xC0, g: 0xC0, b: 0xC0 }
            } else {
                Color { r: 0xFF, g: 0, b: 0 }
            }),
            value_location: match pn.attr("valueLocation") {
                Some("none") => ValueLocation::None,
                Some("left") => ValueLocation::Left,
                Some("right") => ValueLocation::Right,
                _ => ValueLocation::Bulb,
            },
        })
    }

    fn elements(&mut self, container: &Node) -> Result<Vec<Element>> {
        const ELEMENT_TAGS: &[&str] = &[
            "staticText",
            "textField",
            "line",
            "rectangle",
            "ellipse",
            "frame",
            "image",
            "subreport",
            "break",
            "crosstab",
            "componentElement",
            "genericElement",
        ];
        let mut out = Vec::new();
        for c in &container.children {
            let kind = if c.name == "element" {
                c.attr("kind").unwrap_or("unknown")
            } else if ELEMENT_TAGS.contains(&c.name.as_str()) || c.name.ends_with("Chart") || c.name.ends_with("Chart3D") {
                c.name.as_str()
            } else if c.name == "elementGroup" {
                // Element groups only affect stretch dependencies; flatten them.
                out.extend(self.elements(c)?);
                continue;
            } else {
                continue;
            };
            if kind == "elementGroup" {
                // JRXML 7 element groups, flattened like JRXML 6 ones.
                out.extend(self.elements(c)?);
                continue;
            }
            if kind == "chart" {
                // JRXML 7 charts are read through their JRXML 6 shape.
                let old = v7_chart(c);
                out.push(self.element(&old, &old.name.clone())?);
                continue;
            }
            out.push(self.element(c, kind)?);
        }
        Ok(out)
    }

    fn element(&mut self, n: &Node, kind: &str) -> Result<Element> {
        // JRXML 6 keeps geometry in <reportElement>; JRXML 7 puts it on <element>.
        // Charts keep their geometry in <chart><reportElement>.
        let re = n.child("reportElement").or_else(|| n.child("chart").and_then(|c| c.child("reportElement"))).unwrap_or(n);
        let num = |name: &str| -> Result<f32> { re.attr(name).map(|v| parse_f32(v, name)).transpose().map(|v| v.unwrap_or(0.0)) };

        // Style chain: named style, else the report default style.
        let base = match re.attr("style") {
            Some(name) => match self.styles.get(name) {
                Some(s) => s.clone(),
                None => {
                    self.warnings.push(format!("unknown style `{name}`"));
                    StyleProps::default()
                }
            },
            None => self.default_style.clone(),
        };
        let mut own = StyleProps::default();
        own.read_colors(re);
        if let Some(te) = n.child("textElement") {
            own.read_alignment(te);
            if let Some(f) = te.child("font") {
                own.read_font_attrs(f);
            }
            if let Some(p) = te.child("paragraph") {
                own.read_paragraph(p);
            }
        }
        if n.name == "element" {
            own.read_font_attrs(n);
            own.read_alignment(n);
            if let Some(p) = n.child("paragraph") {
                own.read_paragraph(p);
            }
        }
        if let Some(b) = n.child("box").or_else(|| n.child("chart").and_then(|c| c.child("box"))) {
            own.read_box(b);
        }
        if let Some(v) = n.attr("pattern") {
            own.pattern = Some(v.to_string());
        }
        if let Some(v) = attr(n, &["isBlankWhenNull", "blankWhenNull"]) {
            own.blank_when_null = Some(parse_bool(v));
        }
        if let Some(v) = n.attr("radius").and_then(number) {
            own.radius = Some(v);
        }
        if let Some(pen) = n.child("graphicElement").and_then(|g| g.child("pen")).or_else(|| n.child("pen")) {
            own.graphic_pen = PenProps::read(pen);
        }
        let parts = self.styled_parts(n, kind, &own.over(&base))?;

        // Conditional styles of the element's style (or of the default style): one variant per
        // combination of conditions, chosen at fill time.
        let style_name = re.attr("style").map(str::to_string).or_else(|| self.default_name.clone());
        let conds: Vec<(Expr, StyleProps)> = style_name
            .and_then(|name| self.conditionals.get(&name).cloned())
            .unwrap_or_default()
            .into_iter()
            .take(MAX_CONDITIONAL_STYLES)
            .collect();
        let conditional = if conds.is_empty() {
            None
        } else {
            let mut variants = Vec::with_capacity(1 << conds.len());
            for mask in 0..(1usize << conds.len()) {
                let mut props = base.clone();
                for (i, (_, cprops)) in conds.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        props = cprops.over(&props);
                    }
                }
                variants.push(self.styled_parts(n, kind, &own.over(&props))?);
            }
            Some(Box::new(ConditionalStyles { conditions: conds.into_iter().map(|(e, _)| e).collect(), variants }))
        };

        Ok(Element {
            tag: re
                .children_named("property")
                .find(|p| p.attr("name") == Some("net.sf.jasperreports.export.accessibility.tag"))
                .and_then(|p| p.attr("value").map(str::to_string).or_else(|| Some(p.text.clone())))
                .and_then(|v| Tag::parse(&v)),
            key: re.attr("key").map(str::to_string),
            x: num("x")?,
            y: num("y")?,
            width: num("width")?,
            height: num("height")?,
            position_type: match re.attr("positionType") {
                Some("Float") => PositionType::Float,
                Some("FixRelativeToBottom") => PositionType::FixRelativeToBottom,
                Some("FixRelativeToTop") => PositionType::FixRelativeToTop,
                _ => PositionType::Fix,
            },
            stretch_type: match re.attr("stretchType") {
                // JasperReports 6.10 renamed RelativeToBandHeight and RelativeToTallestObject
                // (ContainerHeight, ElementGroupHeight); both names mean the same.
                Some("RelativeToBandHeight" | "ContainerHeight") => StretchType::ContainerHeight,
                Some("RelativeToTallestObject" | "ElementGroupHeight") => StretchType::RelativeToTallestObject,
                Some("ContainerBottom" | "ElementGroupBottom") => StretchType::ContainerBottom,
                _ => StretchType::NoStretch,
            },
            mode: parts.mode,
            forecolor: parts.forecolor,
            backcolor: parts.backcolor,
            print_when: self.child_expr(re, &["printWhenExpression"])?,
            remove_line_when_blank: attr(re, &["isRemoveLineWhenBlank", "removeLineWhenBlank"]).is_some_and(parse_bool),
            print_when_detail_overflows: attr(re, &["isPrintWhenDetailOverflows", "printWhenDetailOverflows"])
                .is_some_and(parse_bool),
            print_repeated_values: attr(re, &["isPrintRepeatedValues", "printRepeatedValues"]).is_none_or(parse_bool),
            print_in_first_whole_band: attr(re, &["isPrintInFirstWholeBand", "printInFirstWholeBand"]).is_some_and(parse_bool),
            print_when_group_changes: re.attr("printWhenGroupChanges").map(str::to_string),
            conditional,
            link: self.hyperlink(n)?,
            kind: parts.kind,
        })
    }

    /// The link, anchor and bookmark of an element: attributes and expressions of the element
    /// itself in JRXML 6 (`hyperlinkType`) and 7 (`linkType`).
    fn hyperlink(&self, n: &Node) -> Result<Option<Box<Hyperlink>>> {
        let kind = match attr(n, &["hyperlinkType", "linkType"]) {
            Some("Reference") => LinkKind::Reference,
            Some("LocalAnchor") => LinkKind::LocalAnchor,
            Some("LocalPage") => LinkKind::LocalPage,
            Some("RemoteAnchor") => LinkKind::RemoteAnchor,
            Some("RemotePage") => LinkKind::RemotePage,
            _ => LinkKind::None,
        };
        let anchor_name = self.child_expr(n, &["anchorNameExpression"])?;
        if kind == LinkKind::None && anchor_name.is_none() {
            return Ok(None);
        }
        Ok(Some(Box::new(Hyperlink {
            kind,
            reference: self.child_expr(n, &["hyperlinkReferenceExpression"])?,
            anchor: self.child_expr(n, &["hyperlinkAnchorExpression"])?,
            page: self.child_expr(n, &["hyperlinkPageExpression"])?,
            tooltip: self.child_expr(n, &["hyperlinkTooltipExpression"])?,
            bookmark_level: attr(n, &["bookmarkLevel"]).and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            bookmark_level_expr: self.child_expr(n, &["bookmarkLevelExpression"])?,
            anchor_name,
        })))
    }

    /// Builds the style-dependent parts of an element from its resolved properties.
    fn styled_parts(&mut self, n: &Node, kind: &str, props: &StyleProps) -> Result<StyledParts> {
        let forecolor = props.forecolor.unwrap_or(Color::BLACK);
        let is_graphic = matches!(kind, "rectangle" | "ellipse");
        // Images are transparent unless a mode is given (Jasper 6 default).
        let mode = props.mode.unwrap_or(if is_graphic { Mode::Opaque } else { Mode::Transparent });

        let element_kind = match kind {
            "staticText" | "textField" => {
                let font = FontSpec {
                    name: props.font_name.clone().unwrap_or_else(|| "SansSerif".into()),
                    size: props.font_size.unwrap_or(10.0),
                    bold: props.bold.unwrap_or(false),
                    italic: props.italic.unwrap_or(false),
                    underline: props.underline.unwrap_or(false),
                    strike: props.strike.unwrap_or(false),
                    pdf_font: props.pdf_font_name.clone(),
                };
                let rotation = match n.child("textElement").and_then(|t| t.attr("rotation")).or_else(|| n.attr("rotation")) {
                    Some("Left") => Rotation::Left,
                    Some("Right") => Rotation::Right,
                    Some("UpsideDown") => Rotation::UpsideDown,
                    _ => Rotation::None,
                };
                let style = TextStyle {
                    font,
                    halign: props.halign.unwrap_or(HAlign::Left),
                    valign: props.valign.unwrap_or(VAlign::Top),
                    line_spacing: props.line_spacing(),
                    rotation,
                };
                let te_node = n.child("textElement");
                let markup = match attr(n, &["markup"]).or_else(|| te_node.and_then(|t| t.attr("markup"))) {
                    Some("styled") => crate::rich::Markup::Styled,
                    Some("html") => crate::rich::Markup::Html,
                    Some("rtf") => crate::rich::Markup::Rtf,
                    Some(m) if m != "none" && !m.is_empty() => {
                        self.warnings.push(format!("markup=\"{m}\" is rendered as plain text"));
                        crate::rich::Markup::None
                    }
                    _ if te_node.and_then(|t| t.attr("isStyledText")).is_some_and(parse_bool) => crate::rich::Markup::Styled,
                    _ => crate::rich::Markup::None,
                };
                let stretch = attr(n, &["isStretchWithOverflow"]).is_some_and(parse_bool)
                    || attr(n, &["textAdjust"]).is_some_and(|v| v == "StretchHeight");
                let content = if kind == "staticText" {
                    TextContent::Static(n.child("text").map(|t| t.text.clone()).unwrap_or_default())
                } else {
                    let master = n.attr("evaluationTime") == Some("Master");
                    let evaluation_time = match n.attr("evaluationTime").unwrap_or("Now") {
                        "Report" | "Master" => EvaluationTime::Report,
                        "Page" => EvaluationTime::Page,
                        "Column" => EvaluationTime::Column,
                        "Group" => EvaluationTime::Group,
                        "Band" => EvaluationTime::Band,
                        "Auto" => EvaluationTime::Auto,
                        _ => EvaluationTime::Now,
                    };
                    TextContent::Expression {
                        expr: self.child_expr(n, &["textFieldExpression", "expression"])?,
                        pattern: props.pattern.clone(),
                        pattern_expr: self.child_expr(n, &["patternExpression"])?,
                        blank_when_null: props.blank_when_null.unwrap_or(false),
                        evaluation_time,
                        evaluation_group: if master {
                            Some(MASTER_EVALUATION.to_string())
                        } else {
                            n.attr("evaluationGroup").map(str::to_string)
                        },
                    }
                };
                let property = |name: &str| {
                    let holder = n.child("reportElement").unwrap_or(n);
                    holder
                        .children_named("property")
                        .find(|p| p.attr("name") == Some(name))
                        .map(|p| p.attr("value").map(str::to_string).unwrap_or_else(|| p.text.clone()))
                        .or_else(|| self.report_properties.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
                };
                let truncate = crate::model::Truncate {
                    at_char: property("net.sf.jasperreports.text.truncate.at.char").is_some_and(|v| parse_bool(&v)),
                    suffix: property("net.sf.jasperreports.text.truncate.suffix").filter(|v| !v.is_empty()),
                    keep_full: property("net.sf.jasperreports.print.keep.full.text").is_some_and(|v| parse_bool(&v)),
                };
                let field = property("net.sf.jasperreports.export.pdf.field.type").and_then(|kind| {
                    let p = |k: &str| property(&format!("net.sf.jasperreports.export.pdf.field.{k}"));
                    let kind = match kind.as_str() {
                        "Text" => FieldKind::Text,
                        "Combo" => FieldKind::Combo,
                        "List" => FieldKind::List,
                        "Check" => FieldKind::Check,
                        "Radio" => FieldKind::Radio,
                        _ => return None,
                    };
                    let separators = p("choice.separators").unwrap_or_else(|| "|".into());
                    let choices = p("choices")
                        .map(|c| {
                            c.split(|ch: char| separators.contains(ch))
                                .map(|v| v.trim().to_string())
                                .filter(|v| !v.is_empty())
                                .collect()
                        })
                        .unwrap_or_default();
                    let flag = |k: &str| p(k).is_some_and(|v| parse_bool(&v));
                    Some(Box::new(FormField {
                        kind,
                        name: p("name"),
                        value: p("value"),
                        choices,
                        checked: flag("checked"),
                        check_char: match p("check.type").as_deref() {
                            Some("Circle") => 'l',
                            Some("Cross") => '8',
                            Some("Diamond") => 'u',
                            Some("Square") => 'n',
                            Some("Star") => 'H',
                            _ => '4',
                        },
                        read_only: flag("read.only"),
                        multiline: flag("text.multiline"),
                        editable: flag("combo.edit"),
                        border: match p("border.style").as_deref() {
                            Some("Beveled") => 'B',
                            Some("Inset") => 'I',
                            Some("Underline") => 'U',
                            Some("Dashed") => 'D',
                            _ => 'S',
                        },
                    }))
                });
                // Hyphenation is set for a whole report (see `Parser::report`), not per element.
                let holder = n.child("reportElement").unwrap_or(n);
                if holder.children_named("property").any(|p| p.attr("name") == Some(crate::text::HYPHENATE)) {
                    let warning = format!("{} is read on the report only, not on an element", crate::text::HYPHENATE);
                    if !self.warnings.contains(&warning) {
                        self.warnings.push(warning);
                    }
                }
                let te = TextElement { content, style, boxed: props.box_style(forecolor), stretch, markup, truncate, field };
                let paragraph = props.paragraph();
                if !paragraph.is_default() {
                    let key = crate::paragraph::fingerprint(&te);
                    self.paragraphs.retain(|(k, _)| *k != key);
                    self.paragraphs.push((key, paragraph));
                }
                ElementKind::Text(te)
            }
            "line" => ElementKind::Line {
                direction: if n.attr("direction") == Some("BottomUp") { LineDirection::BottomUp } else { LineDirection::TopDown },
                pen: props.graphic_pen.resolve(1.0, forecolor),
            },
            "rectangle" => {
                ElementKind::Rectangle { radius: props.radius.unwrap_or(0.0), pen: props.graphic_pen.resolve(1.0, forecolor) }
            }
            "ellipse" => ElementKind::Ellipse { pen: props.graphic_pen.resolve(1.0, forecolor) },
            "frame" => ElementKind::Frame { children: self.elements(n)?, boxed: props.box_style(forecolor) },
            "componentElement" | "component" if component_node(n, "iconLabel").is_some() => {
                // A label and an icon, drawn as a frame of their two text fields (see
                // `crate::iconlabel`): the filler measures them and places them.
                let c = component_node(n, "iconLabel").unwrap_or(n);
                // The alignment is the component's own (`hAlign`, or `horizontalAlignment` in
                // JRXML 6), else its element style's image alignment.
                let settings = crate::iconlabel::Settings::read(&|a| {
                    let (names, style): (&[&str], _) = match a {
                        "hAlign" => (&["hAlign", "horizontalAlignment"], &props.himage),
                        "vAlign" => (&["vAlign", "verticalAlignment"], &props.vimage),
                        _ => (&[a], &None),
                    };
                    attr(c, names).map(str::to_string).or_else(|| style.clone())
                });
                let mut children = Vec::new();
                for part in ["label", "icon"] {
                    // JRXML 6: <label> and <icon>, each holding a text field. JRXML 7: the text
                    // fields themselves, <labelTextField> and <iconTextField>, read as the
                    // `<element kind="textField">` they are.
                    let v7 = c.child(&format!("{part}TextField")).map(|t| {
                        let mut t = t.clone();
                        t.name = "element".to_string();
                        t.attrs.retain(|(k, _)| k != "kind");
                        t.attrs.push(("kind".to_string(), "textField".to_string()));
                        t
                    });
                    let node = c
                        .child(part)
                        .and_then(|p| {
                            p.children.iter().find(|e| e.name == "textField" || e.name == "staticText" || e.name == "element")
                        })
                        .or(v7.as_ref());
                    if let Some(node) = node {
                        let kind = node.attr("kind").unwrap_or(node.name.as_str()).to_string();
                        children.push(self.element(node, &kind)?);
                    }
                }
                if children.len() == 2 {
                    children.push(crate::iconlabel::marker(settings));
                }
                // The frame's box is the component's own `<box>`, over the box of the
                // element's style (IconLabelComponent is a box container of that style).
                let mut own = StyleProps::default();
                if let Some(b) = c.child("box") {
                    own.read_box(b);
                }
                ElementKind::Frame { children, boxed: own.over(props).box_style(forecolor) }
            }
            "image" => {
                // RealHeight and RealSize print as RetainShape in an element sized from the
                // picture (not for lazy images, which JasperReports does not load to fill).
                let lazy = attr(n, &["isLazy", "lazy"]).is_some_and(|v| v == "true");
                let real = match attr(n, &["scaleImage"]) {
                    Some("RealHeight") if !lazy => Some(crate::model::RealImage::Height),
                    Some("RealSize") if !lazy => Some(crate::model::RealImage::Size),
                    _ => None,
                };
                let scale = match attr(n, &["scaleImage"]) {
                    Some("Clip") => ScaleImage::Clip,
                    Some("FillFrame") => ScaleImage::FillFrame,
                    Some("RetainShape" | "RealHeight" | "RealSize") | None => ScaleImage::RetainShape,
                    Some(other) => {
                        self.warnings.push(format!("scaleImage=\"{other}\" is rendered as RetainShape"));
                        ScaleImage::RetainShape
                    }
                };
                let halign = match attr(n, &["hImageAlign", "hAlign", "horizontalImageAlign", "horizontalAlignment"]) {
                    Some("Center") => HAlign::Center,
                    Some("Right") => HAlign::Right,
                    _ => HAlign::Left,
                };
                let valign = match attr(n, &["vImageAlign", "vAlign", "verticalImageAlign", "verticalAlignment"]) {
                    Some("Middle") => VAlign::Middle,
                    Some("Bottom") => VAlign::Bottom,
                    _ => VAlign::Top,
                };
                let on_error = match attr(n, &["onErrorType"]) {
                    Some("Blank") => OnErrorType::Blank,
                    Some("Icon") => OnErrorType::Icon,
                    _ => OnErrorType::Error,
                };
                ElementKind::Image(ImageElement {
                    expr: self.child_expr(n, &["imageExpression", "expression"])?,
                    real,
                    tooltip: self.child_expr(n, &["hyperlinkTooltipExpression"])?,
                    scale,
                    halign,
                    valign,
                    boxed: props.box_style(forecolor),
                    on_error,
                })
            }
            "subreport" => {
                // JRXML 6: <subreportParameter name><subreportParameterExpression>;
                // JRXML 7: <parameter name><expression>.
                let mut parameters = Vec::new();
                for c in n.children.iter().filter(|c| c.name == "subreportParameter" || c.name == "parameter") {
                    let name = c.attr("name").unwrap_or_default().to_string();
                    if let Some(e) = self.child_expr(c, &["subreportParameterExpression", "expression"])? {
                        parameters.push((name, e));
                    }
                }
                let returns = n
                    .children
                    .iter()
                    .filter(|c| c.name == "returnValue")
                    .filter_map(|c| {
                        Some(ReturnValue {
                            from: c.attr("subreportVariable").or(c.attr("fromVariable"))?.to_string(),
                            to: c.attr("toVariable")?.to_string(),
                            calculation: calculation(c.attr("calculation")),
                        })
                    })
                    .collect();
                let connection = n.child("connectionExpression").is_some();
                ElementKind::Subreport(Box::new(SubreportElement {
                    expr: self.child_expr(n, &["subreportExpression", "expression"])?,
                    parameters,
                    parameters_map: self.child_expr(n, &["parametersMapExpression"])?,
                    data_source: self.child_expr(n, &["dataSourceExpression"])?,
                    connection,
                    returns,
                }))
            }
            tag if crate::chart::ChartKind::from_tag(tag).is_some() => {
                let base_font = FontSpec {
                    name: props.font_name.clone().unwrap_or_else(|| "SansSerif".into()),
                    size: props.font_size.unwrap_or(10.0),
                    bold: false,
                    italic: false,
                    underline: false,
                    strike: false,
                    pdf_font: props.pdf_font_name.clone(),
                };
                ElementKind::Chart(Box::new(self.chart(n, tag, base_font, props.box_style(forecolor))?))
            }
            "componentElement" | "component" if component_node(n, "list").is_some() => {
                let l = component_node(n, "list").unwrap_or(n);
                ElementKind::List(Box::new(self.list(l)?))
            }
            "componentElement" | "component" if table_node(n).is_some() => {
                let t = table_node(n).unwrap_or(n);
                let re = n.child("reportElement").unwrap_or(n);
                let width = re.attr("width").and_then(number).unwrap_or(0.0);
                ElementKind::Table(Box::new(self.table(t, width)?))
            }
            "componentElement" | "component" if barcode_node(n).is_some() => {
                let c = barcode_node(n).unwrap_or(n);
                let name = c.attr("kind").map(|k| k.rsplit(':').next().unwrap_or(k)).filter(|_| c.name == "component");
                let name = name.unwrap_or(c.name.as_str());
                let num = |a: &str| c.attr(a).and_then(number::<f64>);
                let checksum = |w: &mut Vec<String>| match c.attr("checksumMode") {
                    Some(m) => crate::barcode::linear::Checksum::from_name(m).unwrap_or_else(|| {
                        w.push(format!("unknown barcode checksumMode `{m}`, using auto"));
                        crate::barcode::linear::Checksum::Auto
                    }),
                    None => crate::barcode::linear::Checksum::Auto,
                };
                let position = match c.attr("textPosition") {
                    Some("none") => crate::barcode::TextPosition::None,
                    Some("top") => crate::barcode::TextPosition::Top,
                    _ => crate::barcode::TextPosition::Bottom,
                };
                let symbology = if name == "barbecue" {
                    let kind = c.attr("type").unwrap_or("Code128").to_string();
                    if kind == "PostNet" {
                        self.warnings.push(
                            "barbecue type `PostNet` is skipped: Barbecue draws it as bars of one height that no reader \
                             takes; the barcode4j POSTNET component draws a real one"
                                .into(),
                        );
                    } else if !crate::barcode::barbecue::TYPES.contains(&kind.as_str()) {
                        self.warnings.push(format!("barbecue type `{kind}` is not supported yet and is skipped"));
                    }
                    let whole = |a: &str| c.attr(a).and_then(number::<f64>).filter(|v| *v > 0.0).map(|v| v as u32);
                    Symbology::Barbecue {
                        spec: crate::barcode::barbecue::Spec {
                            kind,
                            draw_text: !c.attr("drawText").is_some_and(|v| v.eq_ignore_ascii_case("false")),
                            checksum: c.attr("checksumRequired").is_some_and(|v| v.eq_ignore_ascii_case("true")),
                            bar_width: whole("barWidth"),
                            bar_height: whole("barHeight"),
                            application_identifier: None,
                        },
                        application_identifier: self.child_expr(c, &["applicationIdentifierExpression"])?,
                        rotation: match c.attr("rotation") {
                            Some("Left") => Rotation::Left,
                            Some("Right") => Rotation::Right,
                            Some("UpsideDown") => Rotation::UpsideDown,
                            _ => Rotation::None,
                        },
                    }
                } else if name == "EAN128" {
                    Symbology::Ean128 {
                        module_width: num("moduleWidth"),
                        text_position: position,
                        quiet_zone: num("quietZone"),
                        vertical_quiet_zone: num("verticalQuietZone"),
                        checksum: checksum(&mut self.warnings),
                    }
                } else if name == "PDF417" {
                    let int = |a: &str| c.attr(a).and_then(number::<f64>).map(|v| v.max(0.0) as usize);
                    Symbology::Pdf417 {
                        spec: crate::barcode::Pdf417Spec {
                            module_width: num("moduleWidth"),
                            quiet_zone: num("quietZone"),
                            vertical_quiet_zone: num("verticalQuietZone"),
                            min_columns: int("minColumns"),
                            max_columns: int("maxColumns"),
                            min_rows: int("minRows"),
                            max_rows: int("maxRows"),
                            ratio: num("widthToHeightRatio"),
                            level: int("errorCorrectionLevel").map(|l| l.min(8) as u8),
                        },
                        text_position: position,
                    }
                } else if name == "DataMatrix" {
                    Symbology::DataMatrix {
                        module_width: num("moduleWidth"),
                        shape: c.attr("shape").map(crate::barcode::datamatrix::Shape::from_name).unwrap_or_default(),
                    }
                } else if name == "POSTNET" {
                    Symbology::Postnet {
                        module_width: num("moduleWidth"),
                        short_bar: num("shortBarHeight"),
                        gap: num("intercharGapWidth"),
                        add_checksum: c.attr("checksumMode") == Some("add"),
                    }
                } else if name == "RoyalMailCustomer" || name == "USPSIntelligentMail" {
                    Symbology::FourState {
                        usps: name == "USPSIntelligentMail",
                        module_width: num("moduleWidth"),
                        ascender: num("ascenderHeight"),
                        track: num("trackHeight"),
                        gap: num("intercharGapWidth"),
                        checksum: checksum(&mut self.warnings),
                    }
                } else if name == "QRCode" {
                    let level = c.attr("errorCorrectionLevel").and_then(crate::barcode::qr::Ecl::from_name);
                    Symbology::Qr {
                        level: level.unwrap_or(crate::barcode::qr::Ecl::L),
                        margin: c.attr("margin").and_then(number),
                        version: c.attr("qrVersion").and_then(number),
                    }
                } else {
                    if c.attr("orientation").is_some_and(|o| !matches!(o, "0" | "up")) {
                        self.warnings.push("rotated barcodes are drawn unrotated".into());
                    }
                    let text_position = match c.attr("textPosition") {
                        Some("none") => crate::barcode::TextPosition::None,
                        Some("top") => crate::barcode::TextPosition::Top,
                        _ => crate::barcode::TextPosition::Bottom,
                    };
                    match crate::barcode::linear::Kind::from_component(name) {
                        Some(kind) => {
                            use crate::barcode::linear::{Checksum, Spec};
                            let flag = |a: &str| c.attr(a).map(parse_bool).unwrap_or(false);
                            let checksum = match c.attr("checksumMode") {
                                Some(m) => Checksum::from_name(m).unwrap_or_else(|| {
                                    self.warnings.push(format!("unknown barcode checksumMode `{m}`, using auto"));
                                    Checksum::Auto
                                }),
                                None => Checksum::Auto,
                            };
                            let display_start_stop = flag("displayStartStop");
                            Symbology::Linear(Spec {
                                module_width: num("moduleWidth"),
                                text_position,
                                quiet_zone: num("quietZone"),
                                vertical_quiet_zone: num("verticalQuietZone"),
                                checksum,
                                wide_factor: num("wideFactor"),
                                // barcode4j shows the check character whenever it shows start/stop.
                                display_checksum: flag("displayChecksum") || display_start_stop,
                                display_start_stop,
                                extended: flag("extendedCharSetEnabled"),
                                intercharacter_gap: num("intercharGapWidth"),
                                ..Spec::new(kind)
                            })
                        }
                        None => Symbology::Code128 {
                            module_width: num("moduleWidth"),
                            text_position,
                            quiet_zone: num("quietZone"),
                            vertical_quiet_zone: num("verticalQuietZone"),
                        },
                    }
                };
                ElementKind::Barcode(Box::new(BarcodeElement {
                    symbology,
                    expr: self.child_expr(c, &["codeExpression", "expression"])?,
                    boxed: props.box_style(forecolor),
                    font_name: props.font_name.clone(),
                    font_size: props.font_size,
                }))
            }
            "crosstab" => ElementKind::Crosstab(Box::new(self.crosstab(n)?)),
            "break" => {
                self.warnings.push("page breaks (<break>) are applied after the band that contains them".into());
                if attr(n, &["type"]) == Some("Column") {
                    ElementKind::ColumnBreak
                } else {
                    ElementKind::Break
                }
            }
            other => {
                let label = if other == "componentElement" || other == "component" {
                    n.children
                        .iter()
                        .find(|c| c.name != "reportElement")
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| other.to_string())
                } else {
                    other.to_string()
                };
                self.warnings.push(format!("<{label}> elements are not supported yet and are skipped"));
                ElementKind::Unsupported(label)
            }
        };

        Ok(StyledParts { mode, forecolor, backcolor: props.backcolor.unwrap_or(Color::WHITE), kind: element_kind })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V6: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<jasperReport xmlns="http://jasperreports.sourceforge.net/jasperreports" name="t" pageWidth="595" pageHeight="842" columnWidth="555" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
  <style name="Base" isDefault="true" fontName="DejaVu Sans" fontSize="9"/>
  <style name="Head" style="Base" isBold="true" forecolor="#336699"/>
  <field name="name" class="java.lang.String"/>
  <variable name="total" class="java.lang.Double" calculation="Sum"><variableExpression><![CDATA[$F{amount}]]></variableExpression></variable>
  <title><band height="30">
    <staticText><reportElement style="Head" x="0" y="0" width="200" height="20"/><textElement textAlignment="Center"/><text><![CDATA[Hi]]></text></staticText>
    <textField isStretchWithOverflow="true" pattern="#,##0.00"><reportElement x="0" y="0" width="100" height="10"/><textFieldExpression><![CDATA[$V{total}]]></textFieldExpression></textField>
    <genericElement><reportElement x="0" y="0" width="10" height="10"/><genericElementType namespace="x" name="y"/></genericElement>
  </band></title>
</jasperReport>"##;

    #[test]
    fn static_fields_of_the_templates_own_classes_are_enum_constants() {
        // JasperReports' java1.5 sample: a parameter of the sample's enum Greeting.
        let r = parse_report(
            r#"<jasperReport name="t" pageWidth="595" pageHeight="842" columnWidth="555" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
            <parameter name="greeting" class="Greeting"/>
            <parameter name="status" class="com.example.Order$Status"/>
            <title height="30">
                <element kind="staticText" x="0" y="0" width="100" height="10"><printWhenExpression><![CDATA[$P{greeting} == Greeting.bye]]></printWhenExpression><text>Bye</text></element>
                <element kind="textField" x="0" y="10" width="100" height="10"><expression><![CDATA[Status.OPEN + " " + Integer.MAX_VALUE]]></expression></element>
                <element kind="textField" x="0" y="20" width="100" height="10"><expression><![CDATA[Other.CONSTANT]]></expression></element>
            </title></jasperReport>"#,
        )
        .unwrap();
        let elements = &r.title.as_ref().unwrap().elements;
        let when = format!("{:?}", elements[0].print_when);
        assert!(when.contains("Lit(Str(\"bye\"))") && !when.contains("StaticField"), "{when}");
        let text = |i: usize| match &elements[i].kind {
            ElementKind::Text(TextElement { content: TextContent::Expression { expr: Some(e), .. }, .. }) => format!("{e:?}"),
            other => panic!("{other:?}"),
        };
        assert!(text(1).contains("Lit(Str(\"OPEN\"))") && text(1).contains("StaticField"), "{}", text(1));
        assert!(text(2).contains("StaticField"), "a class the template does not declare: {}", text(2));
    }

    /// An icon label, written the way JasperReports 6 and 7 write it: in 6 the label and the
    /// icon are text fields inside <label> and <icon>; in 7 they are <labelTextField> and
    /// <iconTextField>, with the element's geometry on the element itself.
    #[test]
    fn icon_labels_of_both_versions() {
        let v6 = r#"<jasperReport xmlns:c="http://jasperreports.sourceforge.net/jasperreports/components" name="t" pageWidth="595" pageHeight="842" columnWidth="555" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
            <style name="s" vImageAlign="Middle"/>
            <title><band height="30"><componentElement><reportElement x="0" y="5" width="200" height="10" style="s"/>
            <c:iconLabel iconPosition="Start" horizontalAlignment="Right">
              <c:label><textField textAdjust="StretchHeight"><reportElement x="0" y="0" width="1" height="1"/><textElement><font size="8.0"/></textElement>
                <textFieldExpression><![CDATA["This is it!"]]></textFieldExpression></textField></c:label>
              <c:icon><textField><reportElement x="0" y="0" width="1" height="1"/><textFieldExpression><![CDATA[""]]></textFieldExpression></textField></c:icon>
            </c:iconLabel></componentElement></band></title></jasperReport>"#;
        let v7 = r#"<jasperReport name="t" pageWidth="595" pageHeight="842" columnWidth="555" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
            <style name="s" vImageAlign="Middle"/>
            <title height="30"><element kind="component" x="0" y="5" width="200" height="10" style="s">
            <component kind="iconLabel" iconPosition="Start" hAlign="Right">
              <labelTextField x="0" y="0" width="1" height="1" fontSize="8.0" textAdjust="StretchHeight">
                <expression><![CDATA["This is it!"]]></expression><box padding="5"><pen lineWidth="1.0"/></box></labelTextField>
              <iconTextField x="0" y="0" width="1" height="1"><expression><![CDATA[""]]></expression></iconTextField>
            </component></element></title></jasperReport>"#;
        for (version, source) in [("6", v6), ("7", v7)] {
            let r = parse_report(source).unwrap();
            let el = &r.title.as_ref().unwrap().elements[0];
            assert_eq!((el.x, el.y, el.width, el.height), (0.0, 5.0, 200.0, 10.0), "{version}");
            let ElementKind::Frame { children, .. } = &el.kind else { panic!("{version}: {:?}", el.kind) };
            // The label and the icon as designed (the filler places them), and the settings: the
            // vertical alignment from the vImageAlign of the element's style.
            let Some((settings, label, icon)) = crate::iconlabel::of(children) else { panic!("{version}: {children:?}") };
            let written = |a: &str| match a {
                "iconPosition" => Some("Start".to_string()),
                "hAlign" => Some("Right".to_string()),
                "vAlign" => Some("Middle".to_string()),
                _ => None,
            };
            assert_eq!(settings, crate::iconlabel::Settings::read(&written), "{version}");
            assert_eq!((label.x, label.width, label.height), (0.0, 1.0, 1.0), "{version}");
            let ElementKind::Text(t) = &label.kind else { panic!("{version}") };
            assert_eq!(t.style.font.size, 8.0, "{version}");
            assert!(t.stretch, "{version}");
            assert!(format!("{:?}", t.content).contains("This is it!"), "{version}");
            let ElementKind::Text(t) = &icon.kind else { panic!("{version}") };
            assert!(format!("{:?}", t.content).contains("e001"), "{version}");
        }
        // The padding and pen of the label's own box (written in the 7 form above).
        let r = parse_report(v7).unwrap();
        let ElementKind::Frame { children, .. } = &r.title.as_ref().unwrap().elements[0].kind else { panic!() };
        let ElementKind::Text(t) = &children[0].kind else { panic!() };
        assert!(format!("{:?}", t.boxed).contains("5.0"), "{:?}", t.boxed);
    }

    #[test]
    fn parses_v6_with_style_inheritance() {
        let r = parse_report(V6).unwrap();
        let band = r.title.as_ref().unwrap();
        let ElementKind::Text(t) = &band.elements[0].kind else { panic!() };
        assert_eq!(t.style.font.name, "DejaVu Sans");
        assert_eq!(t.style.font.size, 9.0);
        assert!(t.style.font.bold);
        assert_eq!(t.style.halign, HAlign::Center);
        assert_eq!(band.elements[0].forecolor, Color { r: 0x33, g: 0x66, b: 0x99 });
        let ElementKind::Text(tf) = &band.elements[1].kind else { panic!() };
        assert!(tf.stretch);
        assert!(matches!(&tf.content, TextContent::Expression { pattern: Some(p), .. } if p == "#,##0.00"));
        assert!(matches!(band.elements[2].kind, ElementKind::Unsupported(_)));
        assert!(r.warnings.iter().any(|w| w.contains("genericElement")));
    }

    const V7: &str = r##"<jasperReport name="t7" language="java" pageWidth="595" pageHeight="842" columnWidth="555" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
  <style name="Base" default="true" fontName="DejaVu Sans" fontSize="11.0"/>
  <field name="name" class="java.lang.String"/>
  <title height="40">
    <element kind="textField" x="0" y="0" width="300" height="20" bold="true" hTextAlign="Right" textAdjust="StretchHeight">
      <expression><![CDATA["Hello " + $F{name}]]></expression>
    </element>
  </title>
  <detail><band height="20"><element kind="staticText" x="0" y="0" width="100" height="20"><text><![CDATA[x]]></text></element></band></detail>
</jasperReport>"##;

    #[test]
    fn parses_v7_layout() {
        let r = parse_report(V7).unwrap();
        let title = r.title.as_ref().unwrap();
        assert_eq!(title.height, 40.0);
        let ElementKind::Text(t) = &title.elements[0].kind else { panic!() };
        assert!(t.style.font.bold && t.stretch);
        assert_eq!(t.style.font.size, 11.0);
        assert_eq!(t.style.halign, HAlign::Right);
        assert_eq!(r.detail.len(), 1);
        assert_eq!(title.elements[0].width, 300.0);
    }
}
