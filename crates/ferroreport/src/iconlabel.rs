//! The icon label component (`iconLabel`): a label and an icon, two text fields, side by side
//! in a frame, laid out the way JasperReports 7 lays them out
//! (`net.sf.jasperreports.components.iconlabel.IconLabelComponentFill`).
//!
//! The parser reads the component as a frame of its two text fields, the label then the icon,
//! with a hidden third element that carries the component's settings ([`marker`]); the public
//! model has no place for them in 1.x. The filler measures both texts ([`prepare`]) and places
//! them ([`place`]) where JasperReports does:
//!
//! - The frame is the component element, its box the component's: its padding is the room
//!   around the label and the icon, which are drawn inside it (exporters offset a frame's
//!   elements by its left and top padding).
//! - Between the label and the icon goes a "middle padding" of half the icon's font size, in
//!   whole points.
//! - The icon is as wide as its text and its padding; the label gets the rest of the width.
//!   When the icon does not fit beside the label, they go one above the other ("vertical"),
//!   both as wide as the element.
//! - `labelFill` (`None` by default): with `Horizontal` or `Both` the label keeps all the
//!   width it got, otherwise it shrinks to its text (its widest line, padding and 3 points
//!   more). With `Vertical` or `Both` it is as tall as the element inside the padding. The
//!   label and the icon then take the taller of their two heights.
//! - `iconPosition`: `Start` puts the icon before the label (left of it, or above), anything
//!   else (`End`, the default) after it.
//! - `hAlign` and `vAlign` (`Left` and `Top` by default, or the `hImageAlign` and
//!   `vImageAlign` of the element's style) place the label and the icon inside the padding: side
//!   by side, the two as one block across and each on its own up and down; one above the other,
//!   each on its own across and the two as one block up and down. Centering drops the fraction
//!   of a point (Java's integer division). Up and down count from the component's final height,
//!   so they only show in an element stretched past what it asked for (by its container), or in
//!   one above the other.

use crate::expr::Expr;
use crate::model::{BoxStyle, Color, Element, ElementKind, HAlign, Mode, Padding, PositionType, StretchType, VAlign};
use crate::value::Value;

/// Where the icon goes (`iconPosition`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IconPosition {
    Start,
    /// Also when not given: JasperReports tests for `Start` only.
    End,
}

/// How the label fills the element (`labelFill`, `ContainerFillEnum`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabelFill {
    None,
    Horizontal,
    Vertical,
    Both,
}

impl LabelFill {
    fn horizontal(self) -> bool {
        matches!(self, LabelFill::Horizontal | LabelFill::Both)
    }
    fn vertical(self) -> bool {
        matches!(self, LabelFill::Vertical | LabelFill::Both)
    }
}

/// The settings of an icon label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Settings {
    pub icon_position: IconPosition,
    pub label_fill: LabelFill,
    /// Where the label and the icon go inside the padding (`hAlign`, `vAlign`): left, center or
    /// right; top, middle or bottom.
    pub halign: HAlign,
    pub valign: VAlign,
}

impl Settings {
    /// From the component's attributes.
    pub fn read(attr: &dyn Fn(&str) -> Option<String>) -> Settings {
        Settings {
            icon_position: match attr("iconPosition").as_deref() {
                Some("Start") => IconPosition::Start,
                _ => IconPosition::End,
            },
            label_fill: match attr("labelFill").as_deref() {
                Some("Horizontal") => LabelFill::Horizontal,
                Some("Vertical") => LabelFill::Vertical,
                Some("Both") => LabelFill::Both,
                _ => LabelFill::None,
            },
            halign: match attr("hAlign").as_deref() {
                Some("Center") => HAlign::Center,
                Some("Right") => HAlign::Right,
                _ => HAlign::Left,
            },
            valign: match attr("vAlign").as_deref() {
                Some("Middle") => VAlign::Middle,
                Some("Bottom") => VAlign::Bottom,
                _ => VAlign::Top,
            },
        }
    }

    fn write(self) -> String {
        format!(
            "iconPosition={:?} labelFill={:?} hAlign={:?} vAlign={:?}",
            self.icon_position, self.label_fill, self.halign, self.valign
        )
    }

    fn parse(s: &str) -> Settings {
        let words: Vec<(&str, &str)> = s.split(' ').filter_map(|w| w.split_once('=')).collect();
        let attr = |name: &str| words.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string());
        Settings::read(&attr)
    }
}

/// The key of the hidden element that carries an icon label's settings.
const MARKER_KEY: &str = "net.sf.jasperreports.components.iconlabel";

/// The hidden last element of an icon label's frame: never printed, its key names the
/// component and its settings.
pub(crate) fn marker(settings: Settings) -> Element {
    Element {
        key: Some(format!("{MARKER_KEY} {}", settings.write())),
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
        position_type: PositionType::Fix,
        stretch_type: StretchType::NoStretch,
        mode: Mode::Transparent,
        forecolor: Color::BLACK,
        backcolor: Color { r: 255, g: 255, b: 255 },
        print_when: Some(Expr::Lit(Value::Bool(false))),
        remove_line_when_blank: false,
        print_when_detail_overflows: false,
        print_repeated_values: true,
        print_in_first_whole_band: false,
        print_when_group_changes: None,
        conditional: None,
        tag: None,
        link: None,
        kind: ElementKind::Frame { children: Vec::new(), boxed: BoxStyle::plain(Color::BLACK) },
    }
}

/// The settings of the icon label whose frame has `children`, with its label and icon (`None`:
/// not an icon label).
pub(crate) fn of(children: &[Element]) -> Option<(Settings, &Element, &Element)> {
    let [label, icon, marker] = children else { return None };
    let rest = marker.key.as_deref()?.strip_prefix(MARKER_KEY)?;
    let text = |e: &Element| matches!(e.kind, ElementKind::Text(_));
    (text(label) && text(icon)).then(|| (Settings::parse(rest.trim()), label, icon))
}

/// What measuring a text field gave: the width of its widest line (`getTextWidth`) and its
/// height once stretched (`getStretchHeight`), padding included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Measured {
    pub text_width: f32,
    pub height: f32,
}

/// What [`prepare`] works out before the element's final height is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Prepared {
    /// The component's left and right padding (narrowed when the element is narrower).
    pub left: f32,
    pub right: f32,
    /// The label above or below the icon rather than beside it.
    pub vertical: bool,
    pub middle: f32,
    pub icon_visible: bool,
    /// The widths the two text fields were measured in.
    pub label_width: f32,
    pub icon_width: f32,
    pub label: Measured,
    pub icon: Measured,
    /// Left plus right padding of the label's and the icon's text field.
    pub label_padding: f32,
    pub icon_padding: f32,
    /// The height the component asks for, padding included.
    pub height: f32,
}

/// What [`prepare`] needs to know of the two text fields, besides measuring them.
pub(crate) struct Fields {
    /// Left plus right padding of each.
    pub label_padding: f32,
    pub icon_padding: f32,
    pub icon_font_size: f32,
    /// The label stretches with its text (`textAdjust="StretchHeight"`).
    pub label_stretches: bool,
}

/// Measures an icon label `width` wide and `height` tall (as designed) with the `padding` of
/// its box, with `available` points of height left for it: `IconLabelComponentFill.prepare`.
/// `measure(icon, width)` measures the icon's text field (`true`) or the label's `width` wide.
pub(crate) fn prepare(
    width: f32,
    height: f32,
    padding: Padding,
    available: f32,
    f: &Fields,
    measure: &mut dyn FnMut(bool, f32) -> Measured,
) -> Prepared {
    // An element narrower than its padding (and a point) narrows its left and right padding,
    // each by half of what is missing.
    let (mut left, mut right) = (padding.left, padding.right);
    let missing = width - left - right - 1.0;
    if missing < 0.0 {
        left = (left + (missing / 2.0).floor()).max(0.0);
        right = (right + (missing / 2.0).ceil()).max(0.0);
    }
    let inner = width - left - right;
    // Half the icon's font size between them, if the width leaves room for it.
    let half = (f.icon_font_size / 2.0).trunc();
    let mut middle = if inner <= half { 0.0 } else { (inner - half).min(half) };
    let available_width = inner - middle;
    let text_height = available - padding.top - padding.bottom;

    let mut icon = measure(true, available_width);
    let mut icon_width = available_width;
    // Beside each other when the icon's text fits on its lines and leaves room for the label.
    let mut used = 0.0;
    let mut vertical = icon.text_width > available_width - f.icon_padding;
    if !vertical {
        used = icon.text_width.trunc() + f.icon_padding;
        vertical = available_width <= used;
    }
    let label_width = if vertical { available_width + middle } else { available_width - used };
    let label = measure(false, label_width);

    let mut icon_visible = true;
    if vertical {
        // One above the other: the icon is printed if there is room for it under the label,
        // as wide as the label.
        middle = half;
        let room = if f.label_stretches { text_height - label.height } else { height - label.height } - middle;
        icon_visible = room > 0.0;
        if icon_visible {
            icon_width = label_width;
            icon = measure(true, icon_width);
            icon_visible = icon.height <= room;
        }
    }
    let icon_height = if icon_visible { icon.height } else { 0.0 };
    let content = if vertical {
        label.height + if icon_visible { icon_height + middle } else { 0.0 }
    } else {
        label.height.max(icon_height)
    };
    Prepared {
        left,
        right,
        vertical,
        middle,
        icon_visible,
        label_width,
        icon_width,
        label,
        icon,
        label_padding: f.label_padding,
        icon_padding: f.icon_padding,
        height: content + padding.top + padding.bottom,
    }
}

/// Where the label and the icon go: x, y, width and height in the frame, inside its padding
/// (`None`: the icon is not printed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Placed {
    pub label: [f32; 4],
    pub icon: Option<[f32; 4]>,
}

/// Places the label and the icon of an icon label `width` wide and finally `height` tall (its
/// height after stretching, which JasperReports gives the component before filling it), with
/// the `padding` of its box: `IconLabelComponentFill.fillHorizontal` and `fillVertical`.
pub(crate) fn place(s: Settings, p: &Prepared, width: f32, padding: Padding, height: f32) -> Placed {
    let start = s.icon_position == IconPosition::Start;
    let inner_width = width - p.left - p.right;
    let inner_height = height - padding.top - padding.bottom;
    // What is left around a block `size` long in `room`, halved and rounded down as Java's
    // integer division does (every length here is a whole number of points).
    let half = |room: f32, size: f32| ((room - size) / 2.0).trunc();
    let (label, icon) = if !p.vertical {
        let mut lw = p.label_width;
        if !s.label_fill.horizontal() {
            // The label shrinks to its text, never wider than it was measured.
            lw = lw.min(p.label.text_width.trunc() + p.label_padding + 3.0);
        }
        let mut lh = p.label.height;
        if s.label_fill.vertical() {
            lh = lh.max(inner_height);
        }
        let iw = (p.icon.text_width + p.icon_padding).ceil();
        let common = lh.max(p.icon.height);
        // Across, the label, the middle padding and the icon as one block.
        let block = lw + p.middle + iw;
        let first = match s.halign {
            HAlign::Center => half(inner_width, block),
            HAlign::Right => inner_width - block,
            _ => 0.0,
        };
        let (lx, ix) = if start { (first + iw + p.middle, first) } else { (first, first + lw + p.middle) };
        // Up and down, each on its own (both are as tall).
        let y = match s.valign {
            VAlign::Middle => half(inner_height, common),
            VAlign::Bottom => inner_height - common,
            VAlign::Top => 0.0,
        };
        ([lx, y, lw, common], [ix, y, iw, common])
    } else {
        let (lw, iw) = (p.label_width, p.icon_width);
        let mut lh = p.label.height;
        if s.label_fill.vertical() {
            lh = lh.max(inner_height - (p.icon.height + p.middle));
        }
        let ih = p.icon.height;
        // Across, each on its own.
        let x = |w: f32| match s.halign {
            HAlign::Center => half(inner_width, w),
            HAlign::Right => inner_width - w,
            _ => 0.0,
        };
        // Up and down, the label, the middle padding and the icon as one block.
        let block = lh + p.middle + ih;
        let first = match s.valign {
            VAlign::Middle => half(inner_height, block),
            VAlign::Bottom => inner_height - block,
            VAlign::Top => 0.0,
        };
        let (ly, iy) = if start { (first + ih + p.middle, first) } else { (first, first + lh + p.middle) };
        ([x(lw), ly, lw, lh], [x(iw), iy, iw, ih])
    };
    Placed { label, icon: p.icon_visible.then_some(icon) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PADDING: Padding = Padding { top: 10.0, left: 10.0, bottom: 10.0, right: 10.0 };
    const FIELDS: Fields = Fields { label_padding: 10.0, icon_padding: 20.0, icon_font_size: 12.0, label_stretches: true };

    /// The icon's text 36.4 wide on one line, the label's 40.2: the fields of JasperReports'
    /// iconlabel sample, with its padding.
    fn measure(icon: bool, width: f32) -> Measured {
        if icon {
            Measured { text_width: 36.4_f32.min(width - 20.0), height: 34.0 }
        } else {
            Measured { text_width: 40.2_f32.min(width - 10.0), height: 20.0 }
        }
    }

    fn settings(icon_position: IconPosition, label_fill: LabelFill) -> Settings {
        aligned(icon_position, label_fill, HAlign::Left, VAlign::Top)
    }

    fn aligned(icon_position: IconPosition, label_fill: LabelFill, halign: HAlign, valign: VAlign) -> Settings {
        Settings { icon_position, label_fill, halign, valign }
    }

    #[test]
    fn the_settings_go_with_the_frame() {
        let s = aligned(IconPosition::Start, LabelFill::Both, HAlign::Center, VAlign::Bottom);
        assert_eq!(Settings::parse(&s.write()), s);
        let defaults = Settings::read(&|_| None);
        assert_eq!(defaults, settings(IconPosition::End, LabelFill::None));
    }

    #[test]
    fn side_by_side_the_label_gets_the_width_the_icon_leaves() {
        // 180 inside the padding, 6 between them (half the icon's 12 points), the icon as wide as
        // its text and padding (36 + 20 = 56 when deciding, 57 when printed).
        let p = prepare(200.0, 10.0, PADDING, f32::INFINITY, &FIELDS, &mut measure);
        assert!(!p.vertical && p.icon_visible);
        assert_eq!((p.middle, p.label_width), (6.0, 118.0));
        // As tall as the taller field, and the padding.
        assert_eq!(p.height, 54.0);

        // labelFill="None": the label shrinks to its text (40 + 10 + 3), the icon follows it.
        let placed = place(settings(IconPosition::End, LabelFill::None), &p, 200.0, PADDING, 54.0);
        assert_eq!(placed.label, [0.0, 0.0, 53.0, 34.0]);
        assert_eq!(placed.icon, Some([59.0, 0.0, 57.0, 34.0]));
        // labelFill="Horizontal": the label keeps its 118, after the icon at the start.
        let placed = place(settings(IconPosition::Start, LabelFill::Horizontal), &p, 200.0, PADDING, 54.0);
        assert_eq!(placed.label, [63.0, 0.0, 118.0, 34.0]);
        assert_eq!(placed.icon, Some([0.0, 0.0, 57.0, 34.0]));
        // labelFill="Vertical" in an element stretched to 100: both as tall as its inside.
        let placed = place(settings(IconPosition::End, LabelFill::Vertical), &p, 200.0, PADDING, 100.0);
        assert_eq!(placed.label, [0.0, 0.0, 53.0, 80.0]);
        assert_eq!(placed.icon, Some([59.0, 0.0, 57.0, 80.0]));
    }

    #[test]
    fn without_room_beside_the_label_the_icon_goes_under_it() {
        // 40 inside the padding: 34 for the icon, which needs 56.
        let p = prepare(60.0, 10.0, PADDING, f32::INFINITY, &FIELDS, &mut measure);
        assert!(p.vertical && p.icon_visible);
        assert_eq!((p.label_width, p.icon_width, p.middle), (40.0, 40.0, 6.0));
        assert_eq!(p.height, 20.0 + 6.0 + 34.0 + 20.0);
        let placed = place(settings(IconPosition::End, LabelFill::None), &p, 60.0, PADDING, p.height);
        assert_eq!(placed.label, [0.0, 0.0, 40.0, 20.0]);
        assert_eq!(placed.icon, Some([0.0, 26.0, 40.0, 34.0]));
        let placed = place(settings(IconPosition::Start, LabelFill::None), &p, 60.0, PADDING, p.height);
        assert_eq!(placed.label, [0.0, 40.0, 40.0, 20.0]);
        assert_eq!(placed.icon, Some([0.0, 0.0, 40.0, 34.0]));
        // A label that does not stretch leaves the icon the element's own height: none here.
        let fixed = Fields { label_stretches: false, ..FIELDS };
        let p = prepare(60.0, 10.0, PADDING, f32::INFINITY, &fixed, &mut measure);
        assert!(p.vertical && !p.icon_visible);
        assert_eq!(p.height, 40.0);
        assert_eq!(place(settings(IconPosition::End, LabelFill::None), &p, 60.0, PADDING, 40.0).icon, None);
    }

    #[test]
    fn side_by_side_they_align_as_one_block_across_and_each_up_and_down() {
        let p = prepare(200.0, 10.0, PADDING, f32::INFINITY, &FIELDS, &mut measure);
        let at = |pos, fill, h, v, height| {
            let placed = place(aligned(pos, fill, h, v), &p, 200.0, PADDING, height);
            (placed.label[0], placed.label[1], placed.icon.map(|i| (i[0], i[1])))
        };
        use IconPosition::*;
        // The label (53), the middle (6) and the icon (57): 116 of the 180 inside the padding.
        assert_eq!(at(End, LabelFill::None, HAlign::Center, VAlign::Top, 54.0), (32.0, 0.0, Some((91.0, 0.0))));
        assert_eq!(at(End, LabelFill::None, HAlign::Right, VAlign::Top, 54.0), (64.0, 0.0, Some((123.0, 0.0))));
        assert_eq!(at(Start, LabelFill::None, HAlign::Right, VAlign::Top, 54.0), (127.0, 0.0, Some((64.0, 0.0))));
        // A block a point wider than the room: centering rounds -0.5 to 0, as Java does.
        assert_eq!(at(End, LabelFill::Horizontal, HAlign::Center, VAlign::Top, 54.0), (0.0, 0.0, Some((124.0, 0.0))));
        // 34 tall in 80: 23 down in the middle, 46 at the bottom.
        assert_eq!(at(End, LabelFill::None, HAlign::Left, VAlign::Middle, 100.0), (0.0, 23.0, Some((59.0, 23.0))));
        assert_eq!(at(End, LabelFill::None, HAlign::Left, VAlign::Bottom, 100.0), (0.0, 46.0, Some((59.0, 46.0))));
    }

    #[test]
    fn one_above_the_other_they_align_as_one_block_up_and_down() {
        // The label (20), the middle (6) and the icon (34): 60 of the 80 inside the padding.
        let p = prepare(60.0, 10.0, PADDING, f32::INFINITY, &FIELDS, &mut measure);
        let at = |pos, v| {
            let placed = place(aligned(pos, LabelFill::None, HAlign::Center, v), &p, 60.0, PADDING, 100.0);
            (placed.label[1], placed.icon.map(|i| i[1]))
        };
        assert_eq!(at(IconPosition::End, VAlign::Middle), (10.0, Some(36.0)));
        assert_eq!(at(IconPosition::Start, VAlign::Middle), (50.0, Some(10.0)));
        assert_eq!(at(IconPosition::End, VAlign::Bottom), (20.0, Some(46.0)));
        assert_eq!(at(IconPosition::Start, VAlign::Bottom), (60.0, Some(20.0)));
    }

    #[test]
    fn an_element_narrower_than_its_padding_narrows_it() {
        // 15 wide with 10 on each side: 6 points short, 3 off each side.
        let p = prepare(15.0, 10.0, PADDING, f32::INFINITY, &FIELDS, &mut measure);
        assert_eq!((p.left, p.right), (7.0, 7.0));
    }
}
