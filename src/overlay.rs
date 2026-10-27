//! The overlays laid over the video, drawn in Rust with bundled fonts, the same on every machine: one bar with
//! a zone each for the chapter, the comment, and the effects and shortcuts, and the card a new chapter opens with.
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

use crate::edit::{BarSettings, Edge};
use crate::scene::BarState;

/// Inter Bold, under the SIL Open Font License 1.1 (`assets/fonts/LICENSE-inter`).
const INTER: &[u8] = include_bytes!("../assets/fonts/Inter-Bold.ttf");
/// The icons taperec draws from the Lucide font, cut by `assets/fonts/subset.sh`, under the ISC license
/// (`assets/fonts/LICENSE-lucide`).
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");

/// The video's size in pixels.
const SCREEN: (u32, u32) = (1920, 1080);
/// The bar's height, in pixels.
pub const BAR_HEIGHT: u32 = 64;
/// The chapter zone's width, at the bar's left.
const CHAPTER_WIDTH: f32 = 440.0;
/// The effects and shortcuts zone's width, at the bar's right.
const STATUS_WIDTH: f32 = 620.0;
/// Space between a zone's content and its sides, in pixels.
const ZONE_PADDING: f32 = 20.0;
/// The chapter's text size, in pixels per em.
const CHAPTER_SIZE: f32 = 24.0;
/// The comment's text size, the bar's largest, in pixels per em.
const COMMENT_SIZE: f32 = 28.0;
/// The effects' and keycaps' text size, in pixels per em.
const STATUS_SIZE: f32 = 22.0;
/// How dark the bar's tint over the frosted screen is.
const BAR_TINT: f32 = 0.45;
/// How bright the lines between zones are.
const DIVIDER: f32 = 0.2;
/// A card's title size, in pixels per em.
const CARD_SIZE: f32 = 56.0;
/// How far a card dims the screen.
const CARD_DIM: f32 = 0.55;
/// The size keycaps are designed at; they scale with the text around them.
const KEY_DESIGN: f32 = 28.0;
/// A keycap label's size at [`KEY_DESIGN`], in pixels per em.
const KEY_SIZE: f32 = 22.0;
/// A keycap's height, and its least width, at [`KEY_DESIGN`].
const KEY_HEIGHT: f32 = 36.0;
/// Space between a keycap's label and its sides at [`KEY_DESIGN`].
const KEY_PADDING: f32 = 10.0;
/// Space between two keycaps at [`KEY_DESIGN`].
const KEY_GAP: f32 = 6.0;
/// A keycap's corner radius at [`KEY_DESIGN`].
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

/// A piece of an overlay's line: text, an icon, or keycaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Text set in Inter.
    Text(String),
    /// An icon.
    Icon(Icon),
    /// One keycap per label, e.g. `⇧`, `⌘`, `P`.
    Keys(Vec<String>),
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

/// Where the bar's band starts from the top, by `settings`.
#[must_use]
pub fn bar_top(settings: &BarSettings) -> u32 {
    match settings.at {
        Edge::Top => settings.offset,
        Edge::Bottom => SCREEN.1.saturating_sub(BAR_HEIGHT + settings.offset),
    }
}

/// The bar over a transparent screen, placed by `settings`: the tint and the lines between zones over the band
/// the graph frosts, then the chapter at the left, the comment in the middle and the effects and shortcuts at
/// the right; text too wide for its zone ends in "…".
///
/// # Errors
///
/// A bundled font that cannot be read, or text that cannot be shaped.
pub fn bar(state: &BarState, settings: &BarSettings) -> Result<Image, Error> {
    let mut image = gap();
    let width = f32_of(SCREEN.0);
    let (top, height) = (f32_of(bar_top(settings)), f32_of(BAR_HEIGHT));
    let middle = top + height / 2.0;
    let mut band = rectangle(width, height, 0.0);
    band.translate(0.0, top);
    image.fill(&band, [0, 0, 0], BAR_TINT);
    for x in [CHAPTER_WIDTH, width - STATUS_WIDTH] {
        let mut line = rectangle(1.0, height * 0.6, 0.0);
        line.translate(x, top + height * 0.2);
        image.fill(&line, [255, 255, 255], DIVIDER);
    }
    if let Some(chapter) = &state.chapter {
        let line = fitted(
            &[Run::Text(chapter.clone())],
            CHAPTER_SIZE,
            CHAPTER_WIDTH - 2.0 * ZONE_PADDING,
        )?;
        draw_line(&mut image, &line, ZONE_PADDING, middle, CHAPTER_SIZE)?;
    }
    if let Some(comment) = &state.comment {
        let room = width - CHAPTER_WIDTH - STATUS_WIDTH - 2.0 * ZONE_PADDING;
        let line = fitted(&[Run::Text(comment.clone())], COMMENT_SIZE, room)?;
        let left = CHAPTER_WIDTH + ZONE_PADDING + (room - line.advance) / 2.0;
        draw_line(&mut image, &line, left, middle, COMMENT_SIZE)?;
    }
    let mut status: Vec<Run> = Vec::new();
    for effect in &state.effects {
        if !status.is_empty() {
            status.push(Run::Text("   ".to_owned()));
        }
        status.extend(effect.iter().cloned());
    }
    for (index, keys) in state.keys.iter().enumerate() {
        status.push(Run::Text(
            if index == 0 && !state.effects.is_empty() {
                "   "
            } else {
                "  "
            }
            .to_owned(),
        ));
        status.push(Run::Keys(keys.clone()));
    }
    if !status.is_empty() {
        let room = STATUS_WIDTH - 2.0 * ZONE_PADDING;
        let line = fitted(&status, STATUS_SIZE, room)?;
        draw_line(
            &mut image,
            &line,
            width - ZONE_PADDING - line.advance,
            middle,
            STATUS_SIZE,
        )?;
    }
    Ok(image)
}

/// A new chapter's card: the screen dimmed and `title` large in its middle.
///
/// # Errors
///
/// A bundled font that cannot be read, or text that cannot be shaped.
pub fn card(title: &str) -> Result<Image, Error> {
    let mut image = gap();
    let (width, height) = (f32_of(SCREEN.0), f32_of(SCREEN.1));
    image.fill(&rectangle(width, height, 0.0), [0, 0, 0], CARD_DIM);
    let line = fitted(
        &[Run::Text(title.to_owned())],
        CARD_SIZE,
        width - 4.0 * ZONE_PADDING,
    )?;
    draw_line(
        &mut image,
        &line,
        (width - line.advance) / 2.0,
        height / 2.0,
        CARD_SIZE,
    )?;
    Ok(image)
}

/// `runs` shaped at `size`, cut short with "…" until they fit `room`.
fn fitted(runs: &[Run], size: f32, room: f32) -> Result<Line, Error> {
    let mut runs = runs.to_vec();
    let mut line = Line::shape(&runs, size)?;
    while line.advance > room && shorten_longest_text(&mut runs) {
        line = Line::shape(&runs, size)?;
    }
    Ok(line)
}

/// Draws `line` from `left`, its capitals centred on `middle`: keycaps, then glyphs in white.
fn draw_line(
    image: &mut Image,
    line: &Line,
    left: f32,
    middle: f32,
    size: f32,
) -> Result<(), Error> {
    let baseline = middle + TextMetrics::at(size)?.cap / 2.0;
    image.fill(&line.keycaps(left, middle), [255, 255, 255], KEY_FILL);
    image.fill(&line.glyphs(left, baseline)?, [255, 255, 255], 1.0);
    Ok(())
}

/// Cuts the longest text by one character, ending it in "…"; false when no text is left to cut.
fn shorten_longest_text(runs: &mut [Run]) -> bool {
    let longest = runs
        .iter_mut()
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

/// The screen, fully transparent: what the overlays' stream shows while nothing is up.
#[must_use]
pub fn gap() -> Image {
    Image::new(SCREEN.0, SCREEN.1)
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

    /// Paints `colour` at `opacity` through the coverage of `shape`, over what lies beneath; only the
    /// shape's bounds on the image are rasterized.
    fn fill(&mut self, shape: &Shape, colour: [u8; 3], opacity: f32) {
        let Some((left, top, right, bottom)) = shape.bounds() else {
            return;
        };
        let left = left.floor().max(0.0);
        let top = top.floor().max(0.0);
        let right = right.ceil().min(f32_of(self.width));
        let bottom = bottom.ceil().min(f32_of(self.height));
        if right <= left || bottom <= top {
            return;
        }
        let (x0, y0) = (whole(left), whole(top));
        let (columns, rows) = (whole(right) - x0, whole(bottom) - y0);
        let mut raster = Rasterizer::new(as_index(columns), as_index(rows));
        let shift = |p: Point| point(p.x - left, p.y - top);
        for curve in &shape.0 {
            match *curve {
                Curve::Line(from, to) => raster.draw_line(shift(from), shift(to)),
                Curve::Cubic(from, c1, c2, to) => {
                    raster.draw_cubic(shift(from), shift(c1), shift(c2), shift(to));
                }
                Curve::Quad(from, control, to) => {
                    raster.draw_quad(shift(from), shift(control), shift(to));
                }
            }
        }
        let width = self.width;
        raster.for_each_pixel_2d(|x, y, coverage| {
            let alpha = coverage.min(1.0) * opacity;
            let index = as_index((y0 + y) * width + x0 + x) * 4;
            if let Some(pixel) = self.rgba.get_mut(index..index + 4) {
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
    /// The smallest box holding every point, as (left, top, right, bottom).
    fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let points = self.0.iter().flat_map(|curve| match *curve {
            Curve::Line(a, b) => vec![a, b],
            Curve::Quad(a, b, c) => vec![a, b, c],
            Curve::Cubic(a, b, c, d) => vec![a, b, c, d],
        });
        points.fold(None, |bounds, p| {
            Some(bounds.map_or(
                (p.x, p.y, p.x, p.y),
                |(l, t, r, b): (f32, f32, f32, f32)| {
                    (l.min(p.x), t.min(p.y), r.max(p.x), b.max(p.y))
                },
            ))
        })
    }

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

/// The text font's measures at one size, in pixels.
struct TextMetrics {
    /// The height of a capital letter.
    cap: f32,
}

impl TextMetrics {
    fn at(size: f32) -> Result<Self, Error> {
        let text = ScalerFont::unhinted(fonts()?.text.clone(), Some(size));
        let cap = text.metrics().cap_height().unwrap_or(size * 0.7);
        Ok(Self { cap })
    }
}

/// A shaped line of runs at one text size.
struct Line {
    placed: Vec<Placed>,
    /// The keycaps' boxes, as (left edge, width) from the line's start.
    keycaps: Vec<(f32, f32)>,
    /// How much the keycaps are scaled from [`KEY_DESIGN`].
    key_scale: f32,
    /// The line's width in pixels.
    advance: f32,
}

impl Line {
    /// Shapes `runs`: text in Inter with its kerning, icons from Lucide, keycaps with smaller labels; every
    /// icon and label sits on the middle of a capital letter.
    fn shape(runs: &[Run], size: f32) -> Result<Self, Error> {
        let fonts = fonts()?;
        let cap = TextMetrics::at(size)?.cap;
        let k = size / KEY_DESIGN;
        let mut line = Self {
            placed: Vec::new(),
            keycaps: Vec::new(),
            key_scale: k,
            advance: 0.0,
        };
        for run in runs {
            match run {
                Run::Text(text) => {
                    line.advance += line.push(&fonts.text, size, text, line.advance, 0.0)?;
                }
                Run::Icon(icon) => {
                    let icons = ScalerFont::unhinted(fonts.icons.clone(), Some(size));
                    let rise = icon_bounds(&icons, &fonts.icons, *icon)
                        .map_or(0.0, |(low, high)| cap / 2.0 - f32::midpoint(low, high));
                    let content = icon.codepoint().to_string();
                    line.advance += line.push(&fonts.icons, size, &content, line.advance, rise)?;
                }
                Run::Keys(labels) => {
                    let label_size = KEY_SIZE * k;
                    let rise = (cap - TextMetrics::at(label_size)?.cap) / 2.0;
                    for (index, label) in labels.iter().enumerate() {
                        if index > 0 {
                            line.advance += KEY_GAP * k;
                        }
                        let width = Line::shape_width(&fonts.text, label_size, label)?;
                        let box_width = (width + 2.0 * KEY_PADDING * k).max(KEY_HEIGHT * k);
                        let x = line.advance + (box_width - width) / 2.0;
                        line.push(&fonts.text, label_size, label, x, rise)?;
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
            key_scale: 1.0,
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
        let (height, radius) = (KEY_HEIGHT * self.key_scale, KEY_RADIUS * self.key_scale);
        for (left, width) in &self.keycaps {
            let mut cap = rectangle(*width, height, radius);
            cap.translate(x + left, middle - height / 2.0);
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

/// A whole, non-negative pixel coordinate, already rounded.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "callers pass floored or ceiled coordinates within the image"
)]
fn whole(value: f32) -> u32 {
    value.max(0.0) as u32
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
    use std::time::Duration;

    fn settings(at: Edge, offset: u32) -> BarSettings {
        BarSettings {
            at,
            offset,
            card: Duration::from_secs(2),
        }
    }

    fn alpha(image: &Image, x: u32, y: u32) -> u8 {
        image.rgba[as_index((y * image.width + x) * 4 + 3)]
    }

    fn red(image: &Image, x: u32, y: u32) -> u8 {
        image.rgba[as_index((y * image.width + x) * 4)]
    }

    /// The columns between `from` and `to` with a bright pixel in rows `rows`, as (first, last).
    fn lit(image: &Image, from: u32, to: u32, rows: std::ops::Range<u32>) -> Option<(u32, u32)> {
        let columns: Vec<u32> = (from..to)
            .filter(|&x| rows.clone().any(|y| red(image, x, y) > 128))
            .collect();
        Some((*columns.first()?, *columns.last()?))
    }

    fn full() -> BarState {
        BarState {
            chapter: Some("3/3 · Reopen o3s".into()),
            comment: Some("Only allowlisted hosts answer".into()),
            effects: vec![vec![Run::Icon(Icon::ZoomIn), Run::Text("  1.75×".into())]],
            keys: vec![vec!["⌘".into(), "V".into()]],
        }
    }

    #[test]
    fn the_gap_is_a_clear_screen() {
        let image = gap();
        assert_eq!((image.width, image.height), SCREEN);
        assert!(image.rgba.iter().all(|&b| b == 0));
    }

    #[test]
    fn the_bar_tints_its_band_and_keeps_each_zone() {
        let image = bar(&full(), &settings(Edge::Top, 0)).unwrap();
        assert_eq!(alpha(&image, 300, 2), 115);
        assert_eq!(alpha(&image, 300, 70), 0, "nothing below the band");
        let chapter = lit(&image, 0, 440, 0..64).unwrap();
        assert!(chapter.0 >= 20 && chapter.1 < 420, "{chapter:?}");
        let comment = lit(&image, 441, 1299, 0..64).unwrap();
        let mid = u32::midpoint(comment.0, comment.1);
        assert!(mid.abs_diff(u32::midpoint(440, 1300)) <= 3, "{comment:?}");
        let status = lit(&image, 1301, 1920, 0..64).unwrap();
        assert!(status.0 > 1320 && status.1 <= 1900, "{status:?}");
    }

    #[test]
    fn the_bar_sits_along_either_edge_with_an_offset() {
        let image = bar(&full(), &settings(Edge::Bottom, 100)).unwrap();
        let top = 1080 - 64 - 100;
        assert_eq!(alpha(&image, 300, top + 2), 115);
        assert_eq!(alpha(&image, 300, top - 2), 0);
        assert_eq!(alpha(&image, 300, top + 66), 0);
        assert_eq!(bar_top(&settings(Edge::Top, 30)), 30);
    }

    #[test]
    fn text_too_wide_ends_in_an_ellipsis_within_its_zone() {
        let state = BarState {
            comment: Some("word ".repeat(200)),
            ..BarState::default()
        };
        let image = bar(&state, &settings(Edge::Top, 0)).unwrap();
        let comment = lit(&image, 0, 1920, 0..64).unwrap();
        assert!(comment.0 >= 460 && comment.1 <= 1280, "{comment:?}");
        let mut runs = vec![Run::Text("ab".into())];
        assert!(shorten_longest_text(&mut runs));
        assert_eq!(runs, [Run::Text("a…".into())]);
        assert!(!shorten_longest_text(&mut runs));
    }

    #[test]
    fn a_card_dims_the_screen_around_its_title() {
        let image = card("2/3 · Install VS Code").unwrap();
        assert_eq!(alpha(&image, 10, 10), 140);
        let title = lit(&image, 0, 1920, 500..580).unwrap();
        assert!(
            u32::midpoint(title.0, title.1).abs_diff(960) <= 2,
            "{title:?}"
        );
    }

    #[test]
    fn kerning_pulls_pairs_together() {
        let width = |s: &str| Line::shape(&[Run::Text(s.into())], 100.0).unwrap().advance;
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
            let width = Line::shape(&[Run::Icon(icon)], STATUS_SIZE)
                .unwrap()
                .advance;
            assert!(width > 14.0, "{icon:?} is {width} wide");
        }
    }

    #[test]
    fn pngs_round_trip() {
        let image = card("x").unwrap();
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
