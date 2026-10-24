//! Overlay images drawn in Rust with bundled fonts, the same on every machine: the bar along the top of the
//! video and the caption of a warp.
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
/// Space between the text and the edge of its background, in pixels.
const PADDING: f32 = 24.0;
/// The bar's text size, in pixels per em.
const BAR_SIZE: f32 = 28.0;
/// A caption's text size, in pixels per em.
const CAPTION_SIZE: f32 = 44.0;
/// The corner radius of a caption's box, in pixels.
const CAPTION_RADIUS: f32 = 16.0;
/// How opaque the black behind the text is.
const BACKDROP: f32 = 0.6;

/// An icon of the Lucide font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Two triangles pointing right, for a fast-forwarded stretch.
    FastForward,
}

impl Icon {
    /// Its codepoint in the Lucide font; `assets/fonts/subset.sh` keeps each one an icon uses.
    fn codepoint(self) -> char {
        match self {
            Self::FastForward => '\u{e0bd}',
        }
    }
}

/// A piece of an overlay's line: text, or an icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    /// Text set in Inter.
    Text(String),
    /// An icon.
    Icon(Icon),
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

/// The bar: `runs` on 60 % black across the video's full width, left-aligned and centred vertically.
///
/// # Errors
///
/// A bundled font that cannot be read, or text that cannot be shaped.
pub fn bar(runs: &[Run]) -> Result<Image, Error> {
    let line = Line::shape(runs, BAR_SIZE)?;
    let mut image = Image::new(BAR.0, BAR.1);
    let (width, height) = (f32_of(BAR.0), f32_of(BAR.1));
    image.fill(&rectangle(width, height, 0.0), [0, 0, 0], BACKDROP);
    let baseline = (height - (line.ascent - line.descent)) / 2.0 + line.ascent;
    image.fill(&line.glyphs(PADDING, baseline)?, [255, 255, 255], 1.0);
    Ok(image)
}

/// A warp caption: `runs` in a rounded box of 60 % black sized to the text.
///
/// # Errors
///
/// A bundled font that cannot be read, or text that cannot be shaped.
pub fn caption(runs: &[Run]) -> Result<Image, Error> {
    let line = Line::shape(runs, CAPTION_SIZE)?;
    let width = (line.advance + 2.0 * PADDING).ceil();
    let height = (line.ascent - line.descent + 2.0 * PADDING).ceil();
    let mut image = Image::new(pixels(width), pixels(height));
    image.fill(
        &rectangle(width, height, CAPTION_RADIUS),
        [0, 0, 0],
        BACKDROP,
    );
    image.fill(
        &line.glyphs(PADDING, PADDING + line.ascent)?,
        [255, 255, 255],
        1.0,
    );
    Ok(image)
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

/// One glyph placed on the line: its font, id and pen position, in pixels from the line's start.
struct Placed {
    font: &'static Font,
    glyph: GlyphId,
    x: f32,
    /// How far above the baseline it sits, in pixels; icons are lifted onto the text's middle.
    rise: f32,
}

/// A shaped line of runs at one size.
struct Line {
    placed: Vec<Placed>,
    size: f32,
    /// The line's width in pixels.
    advance: f32,
    /// The text font's ascent above the baseline, in pixels.
    ascent: f32,
    /// Its descent below the baseline, in pixels, as a negative value.
    descent: f32,
}

impl Line {
    /// Shapes `runs` at `size` pixels per em: text in Inter with its kerning, icons from Lucide.
    fn shape(runs: &[Run], size: f32) -> Result<Self, Error> {
        let fonts = fonts()?;
        let text = ScalerFont::unhinted(fonts.text.clone(), Some(size));
        let (ascent, descent) = text
            .metrics()
            .h_line()
            .map_or((size, 0.0), |line| (line.ascender, line.descender));
        let cap = text.metrics().cap_height().unwrap_or(ascent * 0.7);
        let mut placed = Vec::new();
        let mut pen = 0.0;
        for run in runs {
            let (font, content, rise) = match run {
                Run::Text(text) => (&fonts.text, text.clone(), 0.0),
                Run::Icon(icon) => {
                    let icons = ScalerFont::unhinted(fonts.icons.clone(), Some(size));
                    // The icon's middle on the middle of a capital letter.
                    let rise = icon_bounds(&icons, &fonts.icons, *icon)
                        .map_or(0.0, |(low, high)| cap / 2.0 - f32::midpoint(low, high));
                    (&fonts.icons, icon.codepoint().to_string(), rise)
                }
            };
            let scale = size / f32::from(font.units_per_em().max(1));
            let mut buffer = Buffer::new();
            buffer.push_str(&content);
            buffer.set_direction(Direction::LeftToRight);
            buffer.guess_segment_properties();
            harfrust::shape(&ShaperFont::new(font), &mut buffer, ShapeOptions::new())
                .map_err(|e| Error::Shape(format!("{e:?}")))?;
            for (info, position) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
                placed.push(Placed {
                    font,
                    glyph: GlyphId::new(info.glyph_id),
                    x: pen + units(position.x_offset) * scale,
                    rise: rise + units(position.y_offset) * scale,
                });
                pen += units(position.x_advance) * scale;
            }
        }
        Ok(Self {
            placed,
            size,
            advance: pen,
            ascent,
            descent,
        })
    }

    /// The outlines of the line's glyphs, starting at `x` on a baseline at `baseline`, in image pixels.
    fn glyphs(&self, x: f32, baseline: f32) -> Result<Shape, Error> {
        let mut path = Path2d::default();
        for glyph in &self.placed {
            let scaler = ScalerFont::unhinted(glyph.font.clone(), Some(self.size));
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

/// A size in whole pixels, at least one.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "overlay sizes are small and positive; the cast saturates otherwise"
)]
fn pixels(value: f32) -> u32 {
    (value.ceil() as u32).max(1)
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

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let at = as_index((y * image.width + x) * 4);
        let mut rgba = [0; 4];
        rgba.copy_from_slice(&image.rgba[at..at + 4]);
        rgba
    }

    /// The columns with any bright pixel, as (first, last).
    fn lit_columns(image: &Image) -> Option<(u32, u32)> {
        let lit: Vec<u32> = (0..image.width)
            .filter(|&x| (0..image.height).any(|y| pixel(image, x, y)[0] > 128))
            .collect();
        Some((*lit.first()?, *lit.last()?))
    }

    #[test]
    fn the_bar_spans_the_video_in_sixty_percent_black() {
        let image = bar(&text("Part 1 · Docker")).unwrap();
        assert_eq!((image.width, image.height), BAR);
        assert_eq!(pixel(&image, 1900, 2), [0, 0, 0, 153]);
        let (first, last) = lit_columns(&image).unwrap();
        assert!((24..30).contains(&first), "text starts at {first}");
        assert!(last < 400, "text ends at {last}");
    }

    #[test]
    fn bar_text_sits_in_the_middle() {
        let image = bar(&text("HHHH")).unwrap();
        let rows: Vec<u32> = (0..image.height)
            .filter(|&y| (0..200).any(|x| pixel(&image, x, y)[0] > 128))
            .collect();
        let (top, bottom) = (rows[0], rows[rows.len() - 1]);
        let middle = u32::midpoint(top, bottom);
        assert!((30..=34).contains(&middle), "capitals span {top}..{bottom}");
    }

    #[test]
    fn a_caption_grows_with_its_text_and_rounds_its_corners() {
        let short = caption(&text("Pulling")).unwrap();
        let long = caption(&text("Building the cage")).unwrap();
        assert!(
            long.width > short.width + 100,
            "{} vs {}",
            long.width,
            short.width
        );
        assert_eq!(short.height, long.height);
        // A corner pixel lies outside the rounding; the middle of the left edge inside.
        assert_eq!(pixel(&short, 0, 0)[3], 0);
        assert_eq!(pixel(&short, 1, short.height / 2), [0, 0, 0, 153]);
    }

    #[test]
    fn kerning_pulls_pairs_together() {
        let width = |s: &str| Line::shape(&text(s), 100.0).unwrap().advance;
        assert!(width("AV") < width("A") + width("V") - 1.0);
    }

    #[test]
    fn icons_come_from_lucide() {
        let fonts = fonts().unwrap();
        assert!(fonts
            .icons
            .charmap()
            .map_unicode(Icon::FastForward.codepoint())
            .is_some());
        let with_icon = bar(&[Run::Icon(Icon::FastForward)]).unwrap();
        let (first, last) = lit_columns(&with_icon).unwrap();
        assert!(last - first > 14, "the icon spans {first}..{last}");
    }

    #[test]
    fn pngs_round_trip() {
        let image = caption(&text("x")).unwrap();
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
