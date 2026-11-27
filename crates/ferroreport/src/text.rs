//! Line breaking and vertical text metrics, following Jasper's AWT-based text measurer.

use crate::fonts::FaceData;
use crate::model::LineSpacing;

/// One laid-out line of text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    /// Advance width without trailing whitespace.
    pub width: f32,
    /// Byte offset of the line in the [`normalize`]d text, used to continue a text that is
    /// split across pages.
    pub start: usize,
}

/// Line endings normalised to `\n` and tabs to spaces (same byte length).
pub fn normalize(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\r', '\t']) {
        text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', " ").into()
    } else {
        text.into()
    }
}

/// Which of JasperReports' two text measurements to use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measure {
    /// Filling: decides stretch height, truncation and page splits.
    Fill,
    /// Exporting: decides the drawn line breaks and alignment.
    Render,
    /// Exporting text in a PDF standard font: iText breaks it with that font's widths.
    Standard(u8),
}

/// The text of the first `k` lines of [`break_lines`] (the part that fits in a box).
pub fn text_before_line(text: &str, lines: &[Line], k: usize) -> String {
    let normalized = normalize(text);
    match lines.get(k) {
        Some(l) => {
            let head = &normalized[..l.start];
            head.strip_suffix('\n').unwrap_or(head).trim_end_matches(' ').to_string()
        }
        None => normalized.into_owned(),
    }
}

/// ferroreport's own report property (and parameter) that turns hyphenation on: `true` for the
/// language of the render's locale, a language code (`es`, `en_US`), or `false`.
pub const HYPHENATE: &str = "ferroreport.text.hyphenate";

/// The hidden parameter in which a parsed report keeps the paragraph settings (indents, tab
/// stops) of its text elements, which `TextStyle` has no room for in 1.x. Tools that list a
/// report's parameters leave it out, as they leave out [`HYPHENATE`].
pub const PARAGRAPHS: &str = "ferroreport.text.paragraphs";

/// Hyphenation of words that do not fit at the end of a line, in one language. JasperReports
/// does not hyphenate; ferroreport does when a report asks for it ([`HYPHENATE`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hyphenator {
    #[cfg(feature = "hyphenation")]
    lang: hypher::Lang,
}

impl Hyphenator {
    /// The hyphenator of a language, from its code or a locale (`es`, `en_US`, `pt-BR`), if
    /// its patterns are built in.
    pub fn for_language(code: &str) -> Option<Hyphenator> {
        #[cfg(feature = "hyphenation")]
        {
            let code = code.trim().as_bytes();
            let iso = [code.first()?.to_ascii_lowercase(), code.get(1)?.to_ascii_lowercase()];
            if code.get(2).is_some_and(|c| c.is_ascii_alphabetic()) {
                return None;
            }
            hypher::Lang::from_iso(iso).map(|lang| Hyphenator { lang })
        }
        #[cfg(not(feature = "hyphenation"))]
        {
            let _ = code;
            None
        }
    }

    /// Where `unit` (a break unit: a word, maybe with punctuation and trailing spaces) may be
    /// hyphenated, as byte offsets into it. Only a word made of letters is: anything with
    /// digits or other signs inside is left whole, and so is a word shorter than the
    /// language's minimum (two letters before the hyphen, two or three after it).
    pub fn breaks(&self, unit: &str) -> Vec<usize> {
        let word = unit.trim_end_matches([' ', '\t']);
        let start = word.find(char::is_alphabetic).unwrap_or(word.len());
        let end = word.rfind(char::is_alphabetic).map(|i| i + word[i..].chars().next().map_or(1, char::len_utf8));
        let Some(end) = end.filter(|&e| e > start) else { return Vec::new() };
        let core = &word[start..end];
        if !core.chars().all(char::is_alphabetic)
            || !word[..start].chars().chain(word[end..].chars()).all(|c| !c.is_alphanumeric())
        {
            return Vec::new();
        }
        self.syllables(core).into_iter().map(|b| start + b).collect()
    }

    #[cfg(feature = "hyphenation")]
    fn syllables(&self, word: &str) -> Vec<usize> {
        let mut at = 0;
        let mut out = Vec::new();
        for s in hypher::hyphenate(word, self.lang) {
            at += s.len();
            if at < word.len() {
                out.push(at);
            }
        }
        out
    }

    #[cfg(not(feature = "hyphenation"))]
    fn syllables(&self, _word: &str) -> Vec<usize> {
        Vec::new()
    }
}

fn measure(face: &FaceData, size: f32, mode: Measure, s: &str) -> f32 {
    match mode {
        Measure::Fill => face.fill_width(s, size),
        Measure::Render => face.text_width(s, size),
        Measure::Standard(font) => crate::standard::width(font, s, size),
    }
}

/// Breaks `text` into lines no wider than `max_width` at `size` points.
///
/// Paragraphs are split on newlines. Within a paragraph, breaks happen after whitespace and
/// hyphens; trailing whitespace hangs past the edge. A word longer than the line is broken
/// between characters, keeping at least one character per line. A paragraph with
/// right-to-left or complex-script text is shaped and broken by [`crate::shape`] instead.
pub fn break_lines(text: &str, face: &FaceData, size: f32, max_width: f32, mode: Measure) -> Vec<Line> {
    break_lines_hyphenated(text, face, size, max_width, mode, None)
}

/// [`break_lines`], hyphenating words that do not fit at the end of a line when `hyphenator`
/// is given: a word that does not fit is cut after the last of its syllables that fits with a
/// hyphen, and the line ends there with the hyphen drawn. Right-to-left and complex-script
/// paragraphs are not hyphenated.
pub fn break_lines_hyphenated(
    text: &str,
    face: &FaceData,
    size: f32,
    max_width: f32,
    mode: Measure,
    hyphenator: Option<Hyphenator>,
) -> Vec<Line> {
    let mut lines = Vec::new();
    let normalized = normalize(text);
    let mut base = 0;
    // Right-to-left and complex scripts take their own path; the rest is decided once.
    let complex = !matches!(mode, Measure::Standard(_)) && crate::shape::needs_shaping(&normalized);
    for paragraph in normalized.split('\n') {
        if complex && crate::shape::needs_shaping(paragraph) {
            // Right-to-left and complex scripts: shaped advances, Unicode break rules.
            for (from, to, width) in crate::shape::break_paragraph(face, paragraph, size, max_width, mode == Measure::Fill) {
                let text = paragraph[from..to].trim_end_matches(' ');
                lines.push(Line { text: text.to_string(), width, start: base + from });
            }
        } else {
            break_paragraph(paragraph, base, face, size, max_width, mode, hyphenator, &mut lines);
        }
        base += paragraph.len() + 1;
    }
    lines
}

/// The text left after the first `k` lines of [`break_lines`], for continuing on a new page.
pub fn text_after_lines(text: &str, lines: &[Line], k: usize) -> String {
    let normalized = normalize(text);
    match lines.get(k) {
        Some(l) => normalized[l.start..].to_string(),
        None => String::new(),
    }
}

/// Splits a paragraph into break units: each unit ends at a break opportunity.
fn units(paragraph: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = paragraph.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let next = chars.get(k + 1).map(|x| x.1);
        let end = i + c.len_utf8();
        let is_break = match c {
            ' ' => next.is_some_and(|n| n != ' '),
            '-' | '\u{2010}' | '\u{2013}' => {
                // Break after a hyphen only when it is preceded by a letter/digit and followed by one.
                let prev_alnum = k > 0 && chars[k - 1].1.is_alphanumeric();
                prev_alnum && next.is_some_and(|n| n.is_alphanumeric())
            }
            _ => false,
        };
        if is_break {
            out.push(&paragraph[start..end]);
            start = end;
        }
    }
    if start < paragraph.len() || out.is_empty() {
        out.push(&paragraph[start..]);
    }
    out
}

/// Splits a paragraph where Java's line `BreakIterator` allows a break, as JasperReports
/// does when it fills: after spaces; after closing punctuation (`! " % ) , - . : ; ? ] }`)
/// that follows a word, unless a `.` or `,` sits between the digits of a number; and before
/// opening brackets (`( [ {`).
pub(crate) fn java_units(paragraph: &str) -> Vec<&str> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Space,
        Pre,
        Post,
        Other,
    }
    fn class(c: char) -> Class {
        match c {
            ' ' | '\t' => Class::Space,
            '(' | '[' | '{' | '\u{2018}' | '\u{201C}' | '\u{00AB}' => Class::Pre,
            '!' | '"' | '%' | ')' | ',' | '-' | '.' | ':' | ';' | '?' | ']' | '}' | '\u{2019}' | '\u{201D}' | '\u{00BB}'
            | '\u{2010}' | '\u{2013}' | '\u{2014}' => Class::Post,
            _ => Class::Other,
        }
    }
    let chars: Vec<(usize, char)> = paragraph.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0;
    // Whether the current word has had a letter or digit (or other) character yet, and
    // whether it is a number (begins with a digit).
    let mut seen_other = chars.first().is_some_and(|&(_, c)| class(c) == Class::Other);
    let mut number = chars.first().is_some_and(|&(_, c)| c.is_ascii_digit());
    for k in 1..chars.len() {
        let (i, c) = chars[k];
        let prev = chars[k - 1].1;
        let (pc, cc) = (class(prev), class(c));
        let brk = match (pc, cc) {
            (Class::Space, Class::Space) => false,
            (Class::Space, _) => true,
            (_, Class::Space) => false,
            (Class::Post, Class::Other | Class::Pre) => {
                let numeric =
                    number && matches!(prev, '.' | ',') && c.is_ascii_digit() && k >= 2 && chars[k - 2].1.is_ascii_digit();
                // A quote that opens a word does not end it.
                let opening_quote = prev == '"' && !seen_other;
                seen_other && !numeric && !opening_quote || (prev != '"' && !seen_other && cc == Class::Other)
            }
            (Class::Other, Class::Pre) => true,
            _ => false,
        };
        if brk {
            out.push(&paragraph[start..i]);
            start = i;
            seen_other = false;
            number = false;
        }
        if class(c) == Class::Other {
            if !seen_other {
                number = c.is_ascii_digit();
            } else if !c.is_ascii_digit() {
                number = false;
            }
            seen_other = true;
        }
    }
    if start < paragraph.len() || out.is_empty() {
        out.push(&paragraph[start..]);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn break_paragraph(
    paragraph: &str,
    base: usize,
    face: &FaceData,
    size: f32,
    max_width: f32,
    mode: Measure,
    hyphenator: Option<Hyphenator>,
    lines: &mut Vec<Line>,
) {
    let fits = |s: &str| measure(face, size, mode, s.trim_end_matches(' ')) <= max_width + 1e-3;
    let push = |lines: &mut Vec<Line>, from: usize, to: usize| {
        let text = paragraph[from..to].trim_end_matches(' ');
        let width = match mode {
            Measure::Standard(font) => crate::standard::width(font, text, size),
            _ => face.text_width(text, size),
        };
        lines.push(Line { text: text.to_string(), width, start: base + from });
    };
    // A line that ends inside a word, at one of its syllables, with a hyphen.
    let hyphenated = |from: usize, to: usize| format!("{}-", &paragraph[from..to]);
    let push_hyphenated = |lines: &mut Vec<Line>, from: usize, to: usize| {
        let text = hyphenated(from, to);
        let width = match mode {
            Measure::Standard(font) => crate::standard::width(font, &text, size),
            _ => face.text_width(&text, size),
        };
        lines.push(Line { text, width, start: base + from });
    };
    // Cuts the unit `unit_start..unit_end` at the last of its syllables that fits on the line
    // from `line_start` with a hyphen, again on the next line while the rest does not fit.
    let hyphenate = |lines: &mut Vec<Line>, line_start: &mut usize, unit_start: usize, unit_end: usize| {
        let Some(h) = hyphenator else { return };
        let breaks: Vec<usize> = h.breaks(&paragraph[unit_start..unit_end]).into_iter().map(|b| unit_start + b).collect();
        while !fits(&paragraph[*line_start..unit_end]) {
            let fit = breaks
                .iter()
                .rev()
                .find(|&&b| b > *line_start && measure(face, size, mode, &hyphenated(*line_start, b)) <= max_width + 1e-3);
            let Some(&b) = fit else { break };
            push_hyphenated(lines, *line_start, b);
            *line_start = b;
        }
    };
    // Most texts are one line that fits: widths only grow with length, so when the whole
    // paragraph fits there is nothing to break.
    if fits(paragraph) {
        push(lines, 0, paragraph.len());
        return;
    }
    // `line_start..pos` is the current line, as byte offsets into `paragraph`.
    let mut line_start = 0;
    let mut pos = 0;
    let pieces = if mode == Measure::Fill { java_units(paragraph) } else { units(paragraph) };
    for unit in pieces {
        let unit_end = pos + unit.len();
        if fits(&paragraph[line_start..unit_end]) {
            pos = unit_end;
            continue;
        }
        // Hyphenation: as much of the word as fits stays on this line.
        hyphenate(lines, &mut line_start, pos, unit_end);
        if line_start > pos {
            if fits(&paragraph[line_start..unit_end]) {
                pos = unit_end;
                continue;
            }
        } else if pos > line_start {
            push(lines, line_start, pos);
            line_start = pos;
            // The word alone on its new line may need hyphenating too.
            hyphenate(lines, &mut line_start, pos, unit_end);
        }
        // The unit alone may still be too wide: break it between characters.
        while !fits(&paragraph[line_start..unit_end]) {
            let mut w = 0.0;
            let mut cut = line_start;
            for (i, c) in paragraph[line_start..unit_end].char_indices() {
                let cw = measure(face, size, mode, c.encode_utf8(&mut [0; 4]));
                if i > 0 && w + cw > max_width + 1e-3 {
                    break;
                }
                w += cw;
                cut = line_start + i + c.len_utf8();
            }
            push(lines, line_start, cut);
            line_start = cut;
            if line_start >= unit_end {
                break;
            }
        }
        pos = unit_end;
    }
    // What is left is the last line (an empty paragraph is one empty line). A last word broken
    // between characters to its end, each one wider than the line, leaves nothing.
    if line_start < paragraph.len() || line_start == 0 {
        push(lines, line_start, paragraph.len());
    }
}

/// Vertical metrics of a block of text in a given font and line spacing.
///
/// JasperReports 6 gives the first line its natural height (ascent + descent + leading) and
/// every following line `descent + leading + h`, where `h` is `factor * ascent` for
/// proportional spacings (Single = 1, 1½ = 1.5, Double = 2, Proportional = size), the given
/// size for Fixed, and `max(size, ascent)` for AtLeast. When drawing, lines are spaced
/// evenly by the average of those heights, with the first baseline one ascent below the top.
/// These rules were derived from Jasper's PDF output and reproduce it exactly.
#[derive(Debug, Clone, Copy)]
pub struct VerticalMetrics {
    pub ascent: f32,
    pub first_line_height: f32,
    pub other_line_height: f32,
}

pub fn vertical_metrics(face: &FaceData, size: f32, spacing: LineSpacing) -> VerticalMetrics {
    let natural = face.line_height(size);
    let ascent = face.ascent(size);
    let below = natural - ascent;
    let h = match spacing {
        LineSpacing::Single => ascent,
        LineSpacing::OneAndHalf => 1.5 * ascent,
        LineSpacing::Double => 2.0 * ascent,
        LineSpacing::Proportional(f) => f * ascent,
        LineSpacing::AtLeast(s) => s.max(ascent),
        LineSpacing::Fixed(s) => s,
    };
    VerticalMetrics { ascent, first_line_height: natural, other_line_height: below + h }
}

impl VerticalMetrics {
    /// Height of a block with `n` lines.
    pub fn block_height(&self, n: usize) -> f32 {
        if n == 0 {
            0.0
        } else {
            self.first_line_height + self.other_line_height * (n - 1) as f32
        }
    }

    /// Distance between consecutive baselines in a block of `n` lines.
    pub fn advance(&self, n: usize) -> f32 {
        if n == 0 {
            0.0
        } else {
            self.block_height(n) / n as f32
        }
    }

    /// How many lines fit in `available` points.
    pub fn lines_that_fit(&self, available: f32) -> usize {
        if self.first_line_height > available + 1e-3 {
            return 0;
        }
        // Lines of no height (a font of size 0) all fit: the count saturates.
        1usize.saturating_add(((available - self.first_line_height + 1e-3) / self.other_line_height).floor().max(0.0) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_break_opportunities() {
        // What java.text.BreakIterator.getLineInstance() gives for these.
        let cases = [
            (
                r#"<line direction="BottomUp"><reportElement x="0" y="150"/>"#,
                vec![
                    r#"<line "#,
                    r#"direction=""#,
                    r#"BottomUp""#,
                    r#"><reportElement "#,
                    r#"x=""#,
                    r#"0" "#,
                    r#"y=""#,
                    r#"150""#,
                    "/>",
                ],
            ),
            (
                r#"a-b 12.5 x/y (p) q,r "s" #FFF a+b"#,
                vec!["a-", "b ", "12.5 ", "x/y ", "(p) ", "q,", "r ", "\"s\" ", "#FFF ", "a+b"],
            ),
            (
                "a.5 5.a 5,a x(5) a--b a -b 100%x",
                vec!["a.", "5 ", "5.", "a ", "5,", "a ", "x", "(5) ", "a--", "b ", "a ", "-", "b ", "100%", "x"],
            ),
            ("1,5 1.5 1:5 10,000.50 v1.2 \"q\"r", vec!["1,5 ", "1.5 ", "1:", "5 ", "10,000.50 ", "v1.", "2 ", "\"q\"", "r"]),
        ];
        for (text, want) in cases {
            assert_eq!(java_units(text), want, "{text}");
        }
    }
    use crate::fonts::FontSet;
    use crate::model::FontSpec;

    fn face() -> (FontSet, usize) {
        let mut fonts = FontSet::new();
        let id = fonts.resolve(&FontSpec {
            name: "DejaVu Sans".into(),
            size: 10.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            pdf_font: None,
        });
        (fonts, id)
    }

    #[test]
    fn wraps_on_spaces_and_splits_long_words() {
        let (fonts, id) = face();
        let f = fonts.face(id);
        let lines = break_lines("hello world foo", f, 10.0, f.text_width("hello world", 10.0) + 0.5, Measure::Render);
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["hello world", "foo"]);
        let lines = break_lines("abcdefghij", f, 10.0, f.text_width("abcd", 10.0) + 0.1, Measure::Render);
        assert_eq!(lines[0].text, "abcd");
        assert_eq!(lines.len(), 3);
        let lines = break_lines("a\n\nb", f, 10.0, 100.0, Measure::Render);
        assert_eq!(lines.len(), 3);
        // Narrower than any character: one a line, as Java's LineBreakMeasurer gives them, and
        // no empty line after them.
        let lines = break_lines("Ab", f, 10.0, 2.0, Measure::Fill);
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["A", "b"]);
    }

    #[test]
    fn hyphenates_words_that_do_not_fit() {
        let (fonts, id) = face();
        let f = fonts.face(id);
        let text = "Internationalization requires responsibility";
        let width = f.text_width("Internationaliza-", 10.0).max(f.fill_width("Internationaliza-", 10.0)) + 0.1;
        // Without hyphenation, the long word is cut between characters.
        let plain = break_lines(text, f, 10.0, width, Measure::Render);
        assert!(!plain.iter().any(|l| l.text.ends_with('-')));
        let en = Hyphenator::for_language("en_US");
        assert!(en.is_some());
        for mode in [Measure::Fill, Measure::Render] {
            let lines = break_lines_hyphenated(text, f, 10.0, width, mode, en);
            let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
            assert_eq!(texts, ["Internationaliza-", "tion requires re-", "sponsibility"], "{mode:?}");
            // Each line starts where its text starts in the source.
            assert_eq!(&text[lines[1].start..lines[1].start + 4], "tion");
            assert!(lines[0].width <= width);
        }
        // Words with digits or other signs inside, and short words, are left whole.
        let h = en.unwrap();
        assert!(h.breaks("abc123def").is_empty());
        assert!(h.breaks("e-mail").is_empty());
        assert!(h.breaks("cat").is_empty());
        assert_eq!(h.breaks("(hyphenation), "), [3, 7]);
        assert!(Hyphenator::for_language("es").unwrap().breaks("responsabilidad").contains(&"responsabi".len()));
        assert!(Hyphenator::for_language("xx").is_none());
        assert!(Hyphenator::for_language("eng").is_none());
    }

    #[test]
    fn fit_counts() {
        let (fonts, id) = face();
        let m = vertical_metrics(fonts.face(id), 10.0, LineSpacing::Single);
        assert_eq!(m.lines_that_fit(18.0), 1);
        assert_eq!(m.lines_that_fit(23.3), 2);
        assert_eq!(m.lines_that_fit(5.0), 0);
    }

    #[test]
    fn spacing_matches_jasper() {
        let (fonts, id) = face();
        // Baseline advances measured in JasperReports 6.20 PDFs (DejaVu Sans 10pt).
        let m = vertical_metrics(fonts.face(id), 10.0, LineSpacing::OneAndHalf);
        assert!((m.advance(2) - 13.96).abs() < 0.01);
        assert!((m.advance(3) - 14.74).abs() < 0.01);
        assert!((m.advance(4) - 15.12).abs() < 0.01);
        let m = vertical_metrics(fonts.face(id), 10.0, LineSpacing::Fixed(20.0));
        assert!((m.advance(2) - 17.0).abs() < 0.01);
        let m = vertical_metrics(fonts.face(id), 10.0, LineSpacing::Proportional(0.8));
        assert!((m.advance(4) - 10.25).abs() < 0.01);
    }
}
