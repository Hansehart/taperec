//! The bar along the top of the video, drawn in Rust with bundled fonts, the same on every machine.
//!
//! Text is set in Inter and shaped with `HarfRust`, so it kerns as the font asks; icons come from the Lucide
//! icon font. Both fonts ship inside the binary.

use std::fmt;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::sync::OnceLock;

use ab_glyph_rasterizer::{point, Point, Rasterizer};
use harfrust::{Buffer, Direction, ShapeOptions, ShaperFont};
use skrifa::font::glyph::outline::ControlBoundsSink;
use skrifa::{DrawMemory, Font, GlyphId, PathSink, ScalerFont};

/// Inter Bold, under the SIL Open Font License 1.1 (`assets/fonts/LICENSE-inter`).
const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
/// The icons taperec draws from the Lucide font, cut by `assets/fonts/subset.sh`, under the ISC license
/// (`assets/fonts/LICENSE-lucide`).
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");

/// The bar's size in pixels: the video's full width.
const BAR: (u32, u32) = (1920, 64);
/// Space between the bar's content and its ends, in pixels.
const PADDING: f32 = 24.0;
/// Space between two parts in one slot of the bar, in pixels.
const PART_GAP: f32 = 32.0;
/// The bar's text size, in pixels per em.
const BAR_SIZE: f32 = 28.0;
/// How opaque the black behind the text is.
const BACKDROP: f32 = 0.6;
/// A keycap label's size, in pixels per em.
const KEY_SIZE: f32 = 22.0;
/// A keycap's height, and its least width, in pixels.
const KEY_HEIGHT: f32 = 36.0;
/// Space between a keycap's label and its sides, in pixels.
const KEY_PADDING: f32 = 10.0;
/// Space between two keycaps, in pixels.
const KEY_GAP: f32 = 6.0;
/// A keycap's corner radius, in pixels.
const KEY_RADIUS: f32 = 6.0;
/// How opaque a keycap's white is.
const KEY_FILL: f32 = 0.18;

/// An icon of the Lucide font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Two triangles pointing right, for a fast-forwarded stretch.
    FastForward,
    /// A magnifying glass with a plus, for a zoomed view.
    ZoomIn,
}

impl Icon {
    /// Its codepoint in the Lucide font; `assets/fonts/subset.sh` keeps each one an icon uses.
    fn codepoint(self) -> char {
        match self {
            Self::FastForward => '\u{e0bd}',
            Self::ZoomIn => '\u{e1b6}',
        }
    }
}

/// A piece of a part of the bar: text, an icon, or keycaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Text set in Inter.
    Text(String),
    /// An icon.
    Icon(Icon),
    /// One keycap per label, e.g. `⇧`, `⌘`, `P`.
    Keys(Vec<String>),
}

/// What the bar shows: the parts in its left, centre and right slots, each part a line of runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bar {
    /// Parts from the left edge, in order.
    pub left: Vec<Vec<Run>>,
    /// Parts in the middle, in order.
    pub center: Vec<Vec<Run>>,
    /// Parts up to the right edge, in order.
    pub right: Vec<Vec<Run>>,
}

impl Bar {
    /// Whether the bar shows nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.left.is_empty() && self.center.is_empty() && self.right.is_empty()
    }
}

/// An RGBA image, not premultiplied, row by row from the top-left corner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Four bytes per pixel.
    pub rgba: Vec<u8>,
}

/// Why drawing an overlay failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A bundled font that cannot be read.
    Font(&'static str),
    /// Shaping text failed.
    Shape(String),
    /// Writing the PNG failed.
    Write(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(name) => write!(f, "the bundled font {name} cannot be read"),
            Self::Shape(detail) => write!(f, "shaping overlay text failed: {detail}"),
            Self::Write(detail) => write!(f, "writing an overlay failed: {detail}"),
        }
    }
}

impl std::error::Error for Error {}

/// The bundled fonts, read once.
struct Fonts {
    text: Font,
    icons: Font,
}

fn fonts() -> Result<&'static Fonts, Error> {
    static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            Some(Fonts {
                text: Font::new(INTER, 0)?,
                icons: Font::new(LUCIDE, 0)?,
            })
        })
        .as_ref()
        .ok_or(Error::Font("Inter or Lucide"))
}

/// The bar: its parts on 60 % black across the video's full width, centred vertically; text too wide for
/// the bar is cut short with "…".
///
/// # Errors
///
/// A bundled font that cannot be read, or text that cannot be shaped.
pub fn bar(content: &Bar) -> Result<Image, Error> {
    let mut slots = [
        content.left.clone(),
        content.center.clone(),
        content.right.clone(),
    ];
    let mut lines = shape_slots(&slots)?;
    let room = f32_of(BAR.0) - 2.0 * PADDING;
    while width_of(&lines) > room {
        if !shorten_longest_text(&mut slots) {
            break;
        }
        lines = shape_slots(&slots)?;
    }
    let mut image = Image::new(BAR.0, BAR.1);
    let (width, height) = (f32_of(BAR.0), f32_of(BAR.1));
    image.fill(&rectangle(width, height, 0.0), [0, 0, 0], BACKDROP);
    let metrics = TextMetrics::at(BAR_SIZE)?;
    let baseline = (height - (metrics.ascent - metrics.descent)) / 2.0 + metrics.ascent;
    let [left, center, right] = &lines;
    let starts = [
        PADDING,
        (width - slot_width(center)) / 2.0,
        width - PADDING - slot_width(right),
    ];
    let mut keycaps = Shape::default();
    let mut glyphs = Shape::default();
    for (slot, start) in [left, center, right].into_iter().zip(starts) {
        let mut x = start;
        for line in slot {
            keycaps
                .0
                .extend(line.keycaps(x, baseline - metrics.cap / 2.0).0);
            glyphs.0.extend(line.glyphs(x, baseline)?.0);
            x += line.advance + PART_GAP;
        }
    }
    image.fill(&keycaps, [255, 255, 255], KEY_FILL);
    image.fill(&glyphs, [255, 255, 255], 1.0);
    Ok(image)
}

/// Each slot's parts, shaped.
fn shape_slots(slots: &[Vec<Vec<Run>>; 3]) -> Result<[Vec<Line>; 3], Error> {
    let shape = |parts: &Vec<Vec<Run>>| -> Result<Vec<Line>, Error> {
        parts.iter().map(|runs| Line::shape(runs)).collect()
    };
    Ok([shape(&slots[0])?, shape(&slots[1])?, shape(&slots[2])?])
}

/// A slot's width: its parts and the gaps between them.
fn slot_width(lines: &[Line]) -> f32 {
    let gaps = f32_of(u32::try_from(lines.len().saturating_sub(1)).unwrap_or(u32::MAX));
    lines.iter().map(|line| line.advance).sum::<f32>() + gaps * PART_GAP
}

/// The bar's content width: its slots and a gap between each two that show something.
fn width_of(slots: &[Vec<Line>; 3]) -> f32 {
    let filled = slots.iter().filter(|slot| !slot.is_empty()).count();
    let gaps = f32_of(u32::try_from(filled.saturating_sub(1)).unwrap_or(u32::MAX));
    slots.iter().map(|slot| slot_width(slot)).sum::<f32>() + gaps * PART_GAP
}

/// Cuts the longest text by one character, ending it in "…"; false when no text is left to cut.
fn shorten_longest_text(slots: &mut [Vec<Vec<Run>>; 3]) -> bool {
    let longest = slots
        .iter_mut()
        .flatten()
        .flatten()
        .filter_map(|run| match run {
            Run::Text(text) => Some(text),
            _ => None,
        })
        .filter(|text| text.trim_end_matches('…').chars().count() > 1)
        .max_by_key(|text| text.chars().count());
    let Some(text) = longest else {
        return false;
    };
    let mut kept: String = text.trim_end_matches('…').to_owned();
    kept.pop();
    *text = format!("{}…", kept.trim_end());
    true
}

/// The bar's size, fully transparent: what the bar's stream shows while the bar is away.
#[must_use]
pub fn gap() -> Image {
    Image::new(BAR.0, BAR.1)
}

/// Writes `image` as a PNG to `path`.
///
/// # Errors
///
/// The file cannot be created or written.
pub fn write_png(image: &Image, path: &Path) -> Result<(), Error> {
    let error = |detail: String| Error::Write(format!("{}: {detail}", path.display()));
    let file = File::create(path).map_err(|e| error(e.to_string()))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| error(e.to_string()))?;
    writer
        .write_image_data(&image.rgba)
        .map_err(|e| error(e.to_string()))?;
    writer.finish().map_err(|e| error(e.to_string()))
}

impl Image {
    /// A transparent image.
    fn new(width: u32, height: u32) -> Self {
        let size = usize::try_from(u64::from(width) * u64::from(height) * 4).unwrap_or(0);
        Self {
            width,
            height,
            rgba: vec![0; size],
        }
    }

    /// Paints `colour` at `opacity` through the coverage of `shape`, over what lies beneath.
    fn fill(&mut self, shape: &Shape, colour: [u8; 3], opacity: f32) {
        let mut raster = Rasterizer::new(as_index(self.width), as_index(self.height));
        for curve in &shape.0 {
            match *curve {
                Curve::Line(a, b) => raster.draw_line(a, b),
                Curve::Cubic(a, b, c, d) => raster.draw_cubic(a, b, c, d),
                Curve::Quad(a, b, c) => raster.draw_quad(a, b, c),
            }
        }
        raster.for_each_pixel(|index, coverage| {
            let alpha = coverage.min(1.0) * opacity;
            if let Some(pixel) = self.rgba.get_mut(index * 4..index * 4 + 4) {
                blend(pixel, colour, alpha);
            }
        });
    }
}

/// Lays `colour` at `alpha` over `pixel`, as source-over compositing does.
fn blend(pixel: &mut [u8], colour: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let below = f32::from(pixel.get(3).copied().unwrap_or(0)) / 255.0;
    let out = alpha + below * (1.0 - alpha);
    for (channel, source) in pixel.iter_mut().zip(colour) {
        let mixed = (f32::from(source) * alpha + f32::from(*channel) * below * (1.0 - alpha)) / out;
        *channel = byte(mixed);
    }
    if let Some(a) = pixel.get_mut(3) {
        *a = byte(out * 255.0);
    }
}

/// One segment of an outline, in image pixels with y pointing down.
#[derive(Debug, Clone, Copy)]
enum Curve {
    Line(Point, Point),
    Quad(Point, Point, Point),
    Cubic(Point, Point, Point, Point),
}

/// Closed outlines to fill.
#[derive(Debug, Default)]
struct Shape(Vec<Curve>);

impl Shape {
    /// Moves every outline by (`dx`, `dy`).
    fn translate(&mut self, dx: f32, dy: f32) {
        let shift = |p: &mut Point| *p = point(p.x + dx, p.y + dy);
        for curve in &mut self.0 {
            match curve {
                Curve::Line(a, b) => [a, b].into_iter().for_each(shift),
                Curve::Quad(a, b, c) => [a, b, c].into_iter().for_each(shift),
                Curve::Cubic(a, b, c, d) => [a, b, c, d].into_iter().for_each(shift),
            }
        }
    }
}

/// A `width`×`height` rectangle at the image's top-left corner, its corners rounded by `radius`.
fn rectangle(width: f32, height: f32, radius: f32) -> Shape {
    // Traced in font coordinates, y up from the image's bottom edge.
    let mut path = Path2d {
        origin: (0.0, height),
        ..Path2d::default()
    };
    // A cubic's handles at this fraction of the radius trace a quarter circle closely.
    let k = radius * 0.552_284_8;
    let (w, h, r) = (width, height, radius);
    path.move_to(r, 0.0);
    path.line_to(w - r, 0.0);
    path.curve_to(w - r + k, 0.0, w, r - k, w, r);
    path.line_to(w, h - r);
    path.curve_to(w, h - r + k, w - r + k, h, w - r, h);
    path.line_to(r, h);
    path.curve_to(r - k, h, 0.0, h - r + k, 0.0, h - r);
    path.line_to(0.0, r);
    path.curve_to(0.0, r - k, r - k, 0.0, r, 0.0);
    path.close();
    path.shape
}

/// Collects closed outlines given in font coordinates (y up) from `origin`, a point in the image (y down).
#[derive(Debug, Default)]
struct Path2d {
    shape: Shape,
    origin: (f32, f32),
    start: Option<Point>,
    at: Point,
}

impl Path2d {
    fn place(&self, x: f32, y: f32) -> Point {
        point(self.origin.0 + x, self.origin.1 - y)
    }
}

impl PathSink for Path2d {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.at = self.place(x, y);
        self.start = Some(self.at);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.place(x, y);
        self.shape.0.push(Curve::Line(self.at, to));
        self.at = to;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (control, to) = (self.place(cx0, cy0), self.place(x, y));
        self.shape.0.push(Curve::Quad(self.at, control, to));
        self.at = to;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (c0, c1, to) = (self.place(cx0, cy0), self.place(cx1, cy1), self.place(x, y));
        self.shape.0.push(Curve::Cubic(self.at, c0, c1, to));
        self.at = to;
    }

    fn close(&mut self) {
        if let Some(start) = self.start.take() {
            if start != self.at {
                self.shape.0.push(Curve::Line(self.at, start));
            }
            self.at = start;
        }
    }
}

/// One glyph placed on the line: its font, size, id and pen position, in pixels from the line's start.
struct Placed {
    font: &'static Font,
    size: f32,
    glyph: GlyphId,
    x: f32,
    /// How far above the baseline it sits, in pixels; icons are lifted onto the text's middle.
    rise: f32,
}

/// The text font's vertical measures at one size, in pixels.
struct TextMetrics {
    /// Ascent above the baseline.
    ascent: f32,
    /// Descent below the baseline, as a negative value.
    descent: f32,
    /// The height of a capital letter.
    cap: f32,
}

impl TextMetrics {
    fn at(size: f32) -> Result<Self, Error> {
        let text = ScalerFont::unhinted(fonts()?.text.clone(), Some(size));
        let (ascent, descent) = text
            .metrics()
            .h_line()
            .map_or((size, 0.0), |line| (line.ascender, line.descender));
        let cap = text.metrics().cap_height().unwrap_or(ascent * 0.7);
        Ok(Self {
            ascent,
            descent,
            cap,
        })
    }
}

/// A shaped line of runs in the bar's text size.
struct Line {
    placed: Vec<Placed>,
    /// The keycaps' boxes, as (left edge, width) from the line's start.
    keycaps: Vec<(f32, f32)>,
    /// The line's width in pixels.
    advance: f32,
}

impl Line {
    /// Shapes `runs`: text in Inter with its kerning, icons from Lucide, keycaps with smaller labels; every
    /// icon and label sits on the middle of a capital letter.
    fn shape(runs: &[Run]) -> Result<Self, Error> {
        let fonts = fonts()?;
        let cap = TextMetrics::at(BAR_SIZE)?.cap;
        let mut line = Self {
            placed: Vec::new(),
            keycaps: Vec::new(),
            advance: 0.0,
        };
        for run in runs {
            match run {
                Run::Text(text) => {
                    line.advance += line.push(&fonts.text, BAR_SIZE, text, line.advance, 0.0)?;
                }
                Run::Icon(icon) => {
                    let icons = ScalerFont::unhinted(fonts.icons.clone(), Some(BAR_SIZE));
                    let rise = icon_bounds(&icons, &fonts.icons, *icon)
                        .map_or(0.0, |(low, high)| cap / 2.0 - f32::midpoint(low, high));
                    let content = icon.codepoint().to_string();
                    line.advance +=
                        line.push(&fonts.icons, BAR_SIZE, &content, line.advance, rise)?;
                }
                Run::Keys(labels) => {
                    let rise = (cap - TextMetrics::at(KEY_SIZE)?.cap) / 2.0;
                    for (index, label) in labels.iter().enumerate() {
                        if index > 0 {
                            line.advance += KEY_GAP;
                        }
                        let width = Line::shape_width(&fonts.text, KEY_SIZE, label)?;
                        let box_width = (width + 2.0 * KEY_PADDING).max(KEY_HEIGHT);
                        let x = line.advance + (box_width - width) / 2.0;
                        line.push(&fonts.text, KEY_SIZE, label, x, rise)?;
                        line.keycaps.push((line.advance, box_width));
                        line.advance += box_width;
                    }
                }
            }
        }
        Ok(line)
    }

    /// The width `content` takes in `font` at `size`.
    fn shape_width(font: &'static Font, size: f32, content: &str) -> Result<f32, Error> {
        let mut scratch = Self {
            placed: Vec::new(),
            keycaps: Vec::new(),
            advance: 0.0,
        };
        scratch.push(font, size, content, 0.0, 0.0)
    }

    /// Places `content` shaped in `font` at `size` from `x`, `rise` above the baseline; returns its width.
    fn push(
        &mut self,
        font: &'static Font,
        size: f32,
        content: &str,
        x: f32,
        rise: f32,
    ) -> Result<f32, Error> {
        let scale = size / f32::from(font.units_per_em().max(1));
        let mut buffer = Buffer::new();
        buffer.push_str(content);
        buffer.set_direction(Direction::LeftToRight);
        buffer.guess_segment_properties();
        harfrust::shape(&ShaperFont::new(font), &mut buffer, ShapeOptions::new())
            .map_err(|e| Error::Shape(format!("{e:?}")))?;
        let mut pen = 0.0;
        for (info, position) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
            self.placed.push(Placed {
                font,
                size,
                glyph: GlyphId::new(info.glyph_id),
                x: x + pen + units(position.x_offset) * scale,
                rise: rise + units(position.y_offset) * scale,
            });
            pen += units(position.x_advance) * scale;
        }
        Ok(pen)
    }

    /// The outlines of the line's glyphs, starting at `x` on a baseline at `baseline`, in image pixels.
    fn glyphs(&self, x: f32, baseline: f32) -> Result<Shape, Error> {
        let mut path = Path2d::default();
        for glyph in &self.placed {
            let scaler = ScalerFont::unhinted(glyph.font.clone(), Some(glyph.size));
            path.origin = (x + glyph.x, baseline - glyph.rise);
            if let Some(outline) = scaler.outline(glyph.glyph) {
                outline
                    .draw(DrawMemory::Scratch, &mut path)
                    .map_err(|e| Error::Shape(format!("{e:?}")))?;
            }
        }
        path.close();
        Ok(path.shape)
    }

    /// The keycaps' boxes, starting at `x`, centred on the height `middle`, in image pixels.
    fn keycaps(&self, x: f32, middle: f32) -> Shape {
        let mut shape = Shape::default();
        for (left, width) in &self.keycaps {
            let mut cap = rectangle(*width, KEY_HEIGHT, KEY_RADIUS);
            cap.translate(x + left, middle - KEY_HEIGHT / 2.0);
            shape.0.extend(cap.0);
        }
        shape
    }
}

/// The lowest and highest point of `icon` drawn by `scaler`, in pixels above the baseline.
fn icon_bounds(scaler: &ScalerFont, font: &Font, icon: Icon) -> Option<(f32, f32)> {
    let glyph = font.charmap().map_unicode(icon.codepoint())?;
    let mut bounds = ControlBoundsSink::new();
    scaler
        .outline(glyph)?
        .draw(DrawMemory::Scratch, &mut bounds)
        .ok()?;
    bounds.bounding_box().map(|b| (b.y_min, b.y_max))
}

/// Font units as a float; shaping positions stay far within f32's exact range.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "glyph positions are small integers"
)]
fn units(value: i32) -> f32 {
    value as f32
}

/// Pixels as a float.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "image sizes are small integers"
)]
fn f32_of(value: u32) -> f32 {
    value as f32
}

/// A channel value from 0 to 255, rounded.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to a byte's range first"
)]
fn byte(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// A size as an index into pixel rows.
fn as_index(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Vec<Run> {
        vec![Run::Text(text.into())]
    }

    fn left(parts: Vec<Vec<Run>>) -> Bar {
        Bar {
            left: parts,
            ..Bar::default()
        }
    }

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let at = as_index((y * image.width + x) * 4);
        let mut rgba = [0; 4];
        rgba.copy_from_slice(&image.rgba[at..at + 4]);
        rgba
    }

    /// The columns with a bright pixel, as (first, last).
    fn lit_columns(image: &Image) -> Option<(u32, u32)> {
        let lit: Vec<u32> = (0..image.width)
            .filter(|&x| (0..image.height).any(|y| pixel(image, x, y)[0] > 128))
            .collect();
        Some((*lit.first()?, *lit.last()?))
    }

    #[test]
    fn the_bar_spans_the_video_in_sixty_percent_black() {
        let image = bar(&left(vec![text("Part 1 · Docker")])).unwrap();
        assert_eq!((image.width, image.height), BAR);
        assert_eq!(pixel(&image, 1900, 2), [0, 0, 0, 153]);
        let (first, last) = lit_columns(&image).unwrap();
        assert!((24..30).contains(&first), "text starts at {first}");
        assert!(last < 400, "text ends at {last}");
    }

    #[test]
    fn bar_text_sits_in_the_middle() {
        let image = bar(&left(vec![text("HHHH")])).unwrap();
        let rows: Vec<u32> = (0..image.height)
            .filter(|&y| (0..200).any(|x| pixel(&image, x, y)[0] > 128))
            .collect();
        let (top, bottom) = (rows[0], rows[rows.len() - 1]);
        let middle = u32::midpoint(top, bottom);
        assert!((30..=34).contains(&middle), "capitals span {top}..{bottom}");
    }

    #[test]
    fn slots_align_left_centre_and_right() {
        let centre = bar(&Bar {
            center: vec![text("HHHH")],
            ..Bar::default()
        })
        .unwrap();
        let (first, last) = lit_columns(&centre).unwrap();
        assert!(
            u32::midpoint(first, last).abs_diff(960) <= 2,
            "{first}..{last}"
        );
        let right = bar(&Bar {
            right: vec![text("HHHH")],
            ..Bar::default()
        })
        .unwrap();
        let (_, last) = lit_columns(&right).unwrap();
        assert!((1890..1897).contains(&last), "text ends at {last}");
    }

    #[test]
    fn parts_in_a_slot_keep_a_gap() {
        let one = lit_columns(&bar(&left(vec![text("H")])).unwrap()).unwrap();
        let two = lit_columns(&bar(&left(vec![text("H"), text("H")])).unwrap()).unwrap();
        let width = one.1 - one.0;
        assert!(two.1 - two.0 > 2 * width + 30, "{one:?} {two:?}");
    }

    #[test]
    fn keycaps_box_each_label() {
        let image = bar(&left(vec![vec![Run::Keys(vec![
            "⇧".into(),
            "⌘".into(),
            "P".into(),
        ])]]))
        .unwrap();
        // A keycap's white lifts the black above its label; the bar's ends stay plain.
        let lifted = |x| pixel(&image, x, 16)[0] > 20;
        assert!(lifted(30), "{:?}", pixel(&image, 30, 16));
        assert!(!lifted(5));
        // Three boxes with two gaps between them: the row crosses plain black twice.
        let row: Vec<bool> = (24..200).map(lifted).collect();
        let edges = row.windows(2).filter(|w| w[0] && !w[1]).count();
        assert_eq!(edges, 3, "{row:?}");
    }

    #[test]
    fn kerning_pulls_pairs_together() {
        let width = |s: &str| Line::shape(&text(s)).unwrap().advance;
        assert!(width("AV") < width("A") + width("V") - 1.0);
    }

    #[test]
    fn icons_come_from_lucide() {
        let fonts = fonts().unwrap();
        for icon in [Icon::FastForward, Icon::ZoomIn] {
            assert!(
                fonts
                    .icons
                    .charmap()
                    .map_unicode(icon.codepoint())
                    .is_some(),
                "{icon:?}"
            );
            let image = bar(&left(vec![vec![Run::Icon(icon)]])).unwrap();
            let (first, last) = lit_columns(&image).unwrap();
            assert!(last - first > 14, "{icon:?} spans {first}..{last}");
        }
    }

    #[test]
    fn text_too_wide_ends_in_an_ellipsis() {
        let long = "word ".repeat(200);
        let mut slots = [vec![text(&long)], Vec::new(), vec![text("right")]];
        let image = bar(&Bar {
            left: slots[0].clone(),
            right: slots[2].clone(),
            ..Bar::default()
        })
        .unwrap();
        let (first, last) = lit_columns(&image).unwrap();
        assert!(first >= 24 && last <= 1896, "{first}..{last}");
        assert!(shorten_longest_text(&mut slots));
        assert_eq!(slots[2], [text("right")]);
        let mut short = [vec![text("x")], Vec::new(), Vec::new()];
        assert!(!shorten_longest_text(&mut short));
    }

    #[test]
    fn pngs_round_trip() {
        let image = bar(&left(vec![text("x")])).unwrap();
        let path = std::env::temp_dir().join(format!("taperec-overlay-{}.png", std::process::id()));
        write_png(&image, &path).unwrap();
        let decoder = png::Decoder::new(std::io::BufReader::new(File::open(&path).unwrap()));
        let reader = decoder.read_info().unwrap();
        assert_eq!(
            (reader.info().width, reader.info().height),
            (image.width, image.height)
        );
        let _ = std::fs::remove_file(path);
    }
}
