//! Band layout rules checked against JasperReports 7.1.1 on small templates: blank elements
//! that give their space back, elements printed again where a band continues, and how icon
//! labels lay out their label and icon.

use ferroreport::print::{PrintItem, PrintText};
use ferroreport::Renderer;

/// Every line of text in the document: (page, baseline, text).
fn lines(template: &str, rows: usize) -> Vec<(usize, f32, String)> {
    let report = ferroreport::parse(template).unwrap();
    let data = serde_json::Value::Array((1..=rows).map(|n| serde_json::json!({ "n": n })).collect());
    let records = ferroreport::data::records_from_json(&report, &data).unwrap();
    let doc = Renderer::new().fill(&report, &records, Default::default()).unwrap();
    fn walk(item: &PrintItem, page: usize, out: &mut Vec<(usize, f32, String)>) {
        match item {
            PrintItem::Text(PrintText { lines, .. }) => out.extend(lines.iter().map(|l| (page, l.baseline, l.text.clone()))),
            PrintItem::Cell(_, inner) => walk(inner, page, out),
            PrintItem::Group(items) | PrintItem::Paragraphs(_, items) => items.iter().for_each(|i| walk(i, page, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for (page, p) in doc.pages.iter().enumerate() {
        p.items.iter().for_each(|i| walk(i, page + 1, &mut out));
    }
    out
}

fn first_lines(all: &[(usize, f32, String)], prefix: &str) -> Vec<(usize, f32)> {
    all.iter().filter(|l| l.2.starts_with(prefix)).map(|l| (l.0, l.1)).collect()
}

#[test]
fn a_blank_element_keeps_its_space_where_a_stretched_neighbour_overlaps_it() {
    // The static text never prints (isPrintRepeatedValues="false") and has
    // isRemoveLineWhenBlank. Nothing overlaps it in the design, but once the text field above
    // it has stretched and it has floated down, the frame beside it, which stretched too, does:
    // JasperReports keeps its 47 points, and the bands are 130 points apart (three to a page).
    let template = r#"<jasperReport name="keep" pageWidth="300" pageHeight="400" columnWidth="260" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
        <field name="n" class="java.lang.Integer"/>
        <detail><band height="60">
            <element kind="textField" x="0" y="5" width="120" height="0" fontName="DejaVu Sans" fontSize="10.0" textAdjust="StretchHeight"><expression><![CDATA["Row " + $F{n} + ": a long text that stretches its field over several lines and moves the elements below it down."]]></expression></element>
            <element kind="line" positionType="Float" x="0" y="8" width="120" height="1"/>
            <element kind="frame" positionType="Float" x="130" y="4" width="130" height="4">
                <element kind="textField" x="5" y="1" width="120" height="0" fontName="DejaVu Sans" fontSize="10.0" textAdjust="StretchHeight"><expression><![CDATA["Framed " + $F{n} + ": a long text that stretches the frame around it over several lines, and splits with it at the end of a page."]]></expression></element>
            </element>
            <element kind="staticText" positionType="Float" x="0" y="9" width="120" height="47" printRepeatedValues="false" removeLineWhenBlank="true"><text><![CDATA[Space remover]]></text></element>
        </band></detail>
    </jasperReport>"#;
    let rows = first_lines(&lines(template, 6), "Row ");
    assert_eq!(rows.iter().filter(|r| r.0 == 1).count(), 3, "{rows:?}");
    assert!((rows[1].1 - rows[0].1 - 130.0).abs() < 0.01, "{rows:?}");
}

#[test]
fn a_blank_element_gives_its_space_back_where_it_has_floated_to() {
    // The field beside the static text overlaps it in the design, but not where the static text
    // has floated to, under the stretched field: its 47 points go, and the bands are 25 points
    // apart, as in JasperReports 7.1.1. (JasperReports' stretch sample uses such a "space
    // remover"; keeping its space made that sample 21 pages long instead of 14.)
    let template = r#"<jasperReport name="remove" pageWidth="300" pageHeight="400" columnWidth="260" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
        <field name="n" class="java.lang.Integer"/>
        <detail><band height="60">
            <element kind="textField" x="0" y="5" width="120" height="0" fontName="DejaVu Sans" fontSize="10.0" textAdjust="StretchHeight"><expression><![CDATA["Row " + $F{n} + ": a text."]]></expression></element>
            <element kind="line" positionType="Float" x="0" y="8" width="120" height="1"/>
            <element kind="staticText" positionType="Float" x="0" y="9" width="120" height="47" printRepeatedValues="false" removeLineWhenBlank="true"><text><![CDATA[Space remover]]></text></element>
            <element kind="textField" x="130" y="5" width="120" height="16" fontName="DejaVu Sans" fontSize="10.0"><expression><![CDATA["Beside " + $F{n}]]></expression></element>
        </band></detail>
    </jasperReport>"#;
    let rows = first_lines(&lines(template, 2), "Row ");
    assert!((rows[1].1 - rows[0].1 - 25.0).abs() < 0.01, "{rows:?}");
}

#[test]
fn an_element_that_continues_is_not_printed_again_in_full() {
    // A frame with isPrintWhenDetailOverflows whose text goes on to the next page continues
    // there; it is not printed again from its start on top of its continuation (which made
    // JasperReports' stretch sample 21 pages long instead of 14). The line printed again keeps
    // the band's top: the continuation starts where it does in JasperReports 7.1.1, a point
    // above the text's first line on the first page.
    let words: Vec<String> = (1..140).map(|i| format!("w{i}")).collect();
    let template = format!(
        r#"<jasperReport name="reprint" pageWidth="300" pageHeight="400" columnWidth="260" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
        <field name="n" class="java.lang.Integer"/>
        <detail><band height="30">
            <element kind="line" positionType="Float" x="0" y="4" width="120" height="1" printWhenDetailOverflows="true"/>
            <element kind="frame" positionType="Float" x="130" y="4" width="130" height="4" printWhenDetailOverflows="true">
                <element kind="textField" x="5" y="1" width="120" height="0" fontName="DejaVu Sans" fontSize="10.0" textAdjust="StretchHeight"><expression><![CDATA["Framed " + $F{{n}} + " {} end"]]></expression></element>
                <box><pen lineWidth="1.0"/></box>
            </element>
        </band></detail>
    </jasperReport>"#,
        words.join(" ")
    );
    let all = lines(&template, 1);
    let framed = first_lines(&all, "Framed");
    assert_eq!(framed.len(), 1, "{framed:?}");
    let next = all.iter().filter(|l| l.0 == 2).map(|l| l.1).fold(f32::INFINITY, f32::min);
    assert!(all.iter().any(|l| l.0 == 2 && l.2.ends_with("end")), "{all:?}");
    assert!((next - framed[0].1 - (25.7 - 26.7)).abs() < 0.05, "{next} {framed:?}");
}

/// Every line of text on the first page: (x, baseline, text).
fn placed_lines(template: &str) -> Vec<(f32, f32, String)> {
    let report = ferroreport::parse(template).unwrap();
    let records = ferroreport::data::records_from_json(&report, &serde_json::json!([{}])).unwrap();
    let doc = Renderer::new().fill(&report, &records, Default::default()).unwrap();
    fn walk(item: &PrintItem, out: &mut Vec<(f32, f32, String)>) {
        match item {
            PrintItem::Text(PrintText { lines, .. }) => out.extend(lines.iter().map(|l| (l.x, l.baseline, l.text.clone()))),
            PrintItem::Cell(_, inner) => walk(inner, out),
            PrintItem::Group(items) | PrintItem::Paragraphs(_, items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    doc.pages[0].items.iter().for_each(|i| walk(i, &mut out));
    out
}

/// An icon label (JRXML 7) at `x`, its label and its icon in DejaVu Sans, 8 and 10 points, with
/// 2 points of padding, in a box of 4.
fn icon_label(x: f32, width: f32, height: f32, attrs: &str, label: &str) -> String {
    let field = |kind: &str, size: f32, text: &str| {
        format!(
            r#"<{kind}TextField x="0" y="0" width="1" height="1" fontName="DejaVu Sans" fontSize="{size}" textAdjust="StretchHeight">
                <expression><![CDATA["{text}"]]></expression><box padding="2"/></{kind}TextField>"#
        )
    };
    format!(
        r#"<element kind="component" x="{x}" y="0" width="{width}" height="{height}">
            <component kind="iconLabel" {attrs}>{}{}<box padding="4"/></component></element>"#,
        field("label", 8.0, label),
        field("icon", 10.0, "»«")
    )
}

#[test]
fn icon_labels_are_sized_as_jasperreports_measures_them() {
    // Where JasperReports 7.1.1 draws each text (pdftotext -bbox). The label shrinks to its
    // text (labelFill None), or keeps the width the icon leaves it (Horizontal); the icon is as
    // wide as its text, half its font size (5) away from the label. 20 wide, no glyph of the
    // icon fits beside the label: the icon, wrapped, goes above it.
    let template = format!(
        r#"<jasperReport name="t" pageWidth="400" pageHeight="200" columnWidth="360" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
        <title height="60">{}{}{}</title></jasperReport>"#,
        icon_label(0.0, 160.0, 40.0, r#"iconPosition="End" labelFill="None""#, "Label"),
        icon_label(170.0, 160.0, 40.0, r#"iconPosition="Start" labelFill="Horizontal""#, "Wide"),
        icon_label(340.0, 20.0, 10.0, r#"iconPosition="Start""#, "Ab"),
    );
    let all = placed_lines(&template);
    let at = |text: &str| all.iter().find(|l| l.2 == text).unwrap_or_else(|| panic!("{text}: {all:?}"));
    let icons: Vec<f32> = all.iter().filter(|l| l.2 == "»«").map(|l| l.0).collect();
    assert_eq!(icons, [59.0, 196.0], "{all:?}");
    assert_eq!((at("Label").0, at("Wide").0), (26.0, 218.0), "{all:?}");
    let narrow: Vec<f32> = ["»", "«", "A", "b"].iter().map(|t| at(t).0).collect();
    assert_eq!(narrow, [366.0; 4], "{all:?}");
    // One above the other: », «, then the label 5 points under the icon. JasperReports' "A"
    // ends 19.03 points under its "«"; with DejaVu's descent (0.2358 em) at 8 and 10 points,
    // that puts their baselines 19.50 apart.
    let ys: Vec<f32> = ["»", "«", "A", "b"].iter().map(|t| at(t).1).collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
    assert!((ys[2] - ys[1] - 19.50).abs() < 0.05, "{ys:?}");
}

#[test]
fn icon_labels_align_their_label_and_icon() {
    // Stretched with the band by the text field at the end, so that up and down shows. Where
    // JasperReports 7.1.1 draws each text (pdftotext -bbox, x and the bottom of the glyphs less
    // DejaVu's descent): centered and in the middle, at the right and the bottom, and one above
    // the other at the bottom (the icon, last, wrapped).
    let stretched = |x, width, height, attrs, label| {
        icon_label(x, width, height, attrs, label).replacen(
            r#"<element kind="component""#,
            r#"<element kind="component" stretchType="ContainerHeight""#,
            1,
        )
    };
    let template = format!(
        r#"<jasperReport name="t" pageWidth="400" pageHeight="200" columnWidth="360" leftMargin="20" rightMargin="20" topMargin="20" bottomMargin="20">
        <title height="20">{}{}{}<element kind="textField" x="345" y="0" width="15" height="20" fontName="DejaVu Sans" fontSize="8.0" textAdjust="StretchHeight">
            <expression><![CDATA["1\n2\n3\n4\n5\n6\n7\n8"]]></expression></element></title></jasperReport>"#,
        stretched(0.0, 150.0, 20.0, r#"iconPosition="End" labelFill="None" hAlign="Center" vAlign="Middle""#, "Label"),
        stretched(160.0, 150.0, 20.0, r#"iconPosition="Start" labelFill="None" hAlign="Right" vAlign="Bottom""#, "Right"),
        stretched(320.0, 20.0, 10.0, r#"iconPosition="End" hAlign="Center" vAlign="Bottom""#, "Ab"),
    );
    let all = placed_lines(&template);
    // DejaVu's descent (0.2358 em) at 8 and 10 points.
    let (small, big) = (1.887, 2.358);
    let jasper = [
        ("Label", 72.0, 60.35 - small),
        ("Right", 300.0, 86.35 - small),
        ("A", 346.0, 36.35 - small),
        ("b", 346.0, 45.66 - small),
        ("»", 346.0, 66.68 - big),
        ("«", 346.0, 78.32 - big),
    ];
    for (text, x, baseline) in jasper {
        let l = all.iter().find(|l| l.2 == text).unwrap_or_else(|| panic!("{text}: {all:?}"));
        assert!((l.0 - x).abs() < 0.01 && (l.1 - baseline).abs() < 0.05, "{text}: {l:?}, not ({x}, {baseline})");
    }
    let icons: Vec<(f32, f32)> = all.iter().filter(|l| l.2 == "»«").map(|l| (l.0, l.1)).collect();
    assert_eq!(icons.len(), 2, "{all:?}");
    assert!((icons[0].0 - 105.0).abs() < 0.01 && (icons[0].1 - (62.68 - big)).abs() < 0.05, "{icons:?}");
    assert!((icons[1].0 - 278.0).abs() < 0.01 && (icons[1].1 - (88.68 - big)).abs() < 0.05, "{icons:?}");
}
