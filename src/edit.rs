//! Edit files: chapters, comments, speed-ups, zooms and holds placed on tracks against a recording's marks,
//! applied when rendering.
//!
//! ```yaml
//! tracks:
//!   chapter:
//!     - { text: Part 3 · The cage, from: start, until: end }
//!   comment:
//!     - { text: Only allowlisted hosts answer, from: curl-github, until: end }
//!   speed:
//!     - { label: Building the cage, length: 5s, from: reopen + 3s, until: cage-built }
//!   zoom:
//!     - { at: curl-results, scale: 1.75, from: curl-github - 0.5s, duration: 6s }
//!   hold:
//!     - { at: curl-results + 1s, duration: 3s }
//! ```
//!
//! Items in a track may overlap: a later item lies on top of an earlier one, as stacked layers do. Speed-ups
//! are the exception, as a fast-forward cannot nest. The tape's own `chapter`, `warp` and `zoom` steps form the
//! bottom layer.

use std::borrow::Cow;
use std::time::Duration;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;

use crate::tape::{
    self, de_duration, de_opt_duration, without_defaults, DocPoint, DurationSchema, MarkName, Name,
    ParseError, Step, Tape, Text, Where, ZoomScale,
};
use crate::warp::{
    HoldEntry, MarkEntry, TextEntry, TimelineFile, WarpEntry, WindowEntry, ZoomEntry, SCREEN,
};

/// An edit file: items on tracks, each placed against the recording's marks.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "taperec edit")]
pub struct Edit {
    /// Where the bar sits and how long a chapter's card holds; `{ at: top, offset: 0, card: 2s }` unless given.
    #[serde(default)]
    bar: BarSettings,
    /// The tracks; within one, a later item lies on top of an earlier one.
    #[serde(default)]
    tracks: Tracks,
}

/// Where the bar sits, and how long each new chapter's card holds the video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(default, deny_unknown_fields)]
pub struct BarSettings {
    /// The screen's edge the bar sits along.
    pub at: Edge,
    /// Its distance from that edge, in pixels, e.g. to clear the Dock at the bottom.
    pub offset: u32,
    /// How long a new chapter's card holds the video before it moves into the bar; `0s` shows no cards.
    #[serde(deserialize_with = "de_duration")]
    #[schemars(with = "DurationSchema")]
    pub card: Duration,
}

impl Default for BarSettings {
    fn default() -> Self {
        Self {
            at: Edge::Top,
            offset: 0,
            card: Duration::from_secs(2),
        }
    }
}

/// An edge of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    /// The top, over the macOS menu bar.
    Top,
    /// The bottom.
    Bottom,
}

impl Edit {
    /// Where the bar sits and how long cards hold.
    #[must_use]
    pub fn bar(&self) -> BarSettings {
        self.bar
    }
}

/// The tracks of an edit.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Tracks {
    /// Where we are: each new chapter shows on a card, then in the bar; `""` hides what lies beneath.
    #[serde(default)]
    chapter: Vec<TextItem>,
    /// What to notice now, in the bar's middle; `""` hides what lies beneath.
    #[serde(default)]
    comment: Vec<TextItem>,
    /// Stretches fast-forwarded, named in the bar while they play; they must not overlap.
    #[serde(default)]
    speed: Vec<SpeedItem>,
    /// Zooms onto a point, a mark's position or the window under a mark.
    #[serde(default)]
    zoom: Vec<ZoomItem>,
    /// Frames held still for reading time; never within a speed-up.
    #[serde(default)]
    hold: Vec<HoldItem>,
}

/// The video held still at a moment for a while.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct HoldItem {
    /// The moment held.
    at: TimeRef,
    /// How long it holds.
    #[serde(deserialize_with = "de_duration")]
    #[schemars(with = "DurationSchema")]
    duration: Duration,
}

/// A chapter or comment shown from `from` until `until`, or for `duration`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(deny_unknown_fields)]
#[schemars(extend("oneOf" = [{ "required": ["until"] }, { "required": ["duration"] }]))]
struct TextItem {
    /// What it says; `""` hides the texts beneath.
    text: Text,
    /// When it starts.
    from: TimeRef,
    /// When it ends.
    #[serde(default)]
    until: Option<TimeRef>,
    /// How long it lasts.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    duration: Option<Duration>,
}

/// A stretch fast-forwarded into `length` under a caption.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(deny_unknown_fields)]
#[schemars(extend("oneOf" = [{ "required": ["until"] }, { "required": ["duration"] }]))]
struct SpeedItem {
    /// The caption shown while it plays, e.g. `Installing`.
    label: Name,
    /// How long it lasts in the video.
    #[serde(deserialize_with = "de_duration")]
    #[schemars(with = "DurationSchema")]
    length: Duration,
    /// Where the stretch starts.
    from: TimeRef,
    /// Where it ends.
    #[serde(default)]
    until: Option<TimeRef>,
    /// How long it really takes.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    duration: Option<Duration>,
}

/// A zoom onto a point, a mark's position or the window under a mark, eased in at `from` and out at the end.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(deny_unknown_fields)]
#[schemars(extend("allOf" = [
    { "oneOf": [{ "required": ["at"] }, { "required": ["point"] }, { "required": ["window"] }] },
    { "oneOf": [{ "required": ["until"] }, { "required": ["duration"] }] },
]))]
struct ZoomItem {
    /// A mark recorded with a position, e.g. `mark: { name: results, at: { button: Run } }`.
    #[serde(default)]
    #[schemars(with = "MarkRef")]
    at: Option<MarkRef>,
    /// A screen position in pixels from the top-left corner.
    #[serde(default)]
    #[schemars(with = "DocPoint")]
    point: Option<DocPoint>,
    /// A mark recorded with a position, whose window the zoom frames with a margin, as far in as fits.
    #[serde(default)]
    #[schemars(with = "MarkRef")]
    window: Option<MarkRef>,
    /// How far to zoom in, from 1 to 3; 1.5 unless given; the window sets it for `window`.
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 3.0))]
    scale: Option<f64>,
    /// When the zoom starts.
    from: TimeRef,
    /// When it ends.
    #[serde(default)]
    until: Option<TimeRef>,
    /// How long it lasts.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    duration: Option<Duration>,
}

/// A moment: a mark, optionally moved by a duration, e.g. `cage-built`, `reopen + 3s` or `results - 0.5s`.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
struct TimeRef {
    mark: MarkName,
    /// Whether the offset moves the moment earlier.
    earlier: bool,
    offset: Duration,
}

impl TryFrom<String> for TimeRef {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        let bad = || format!("expected a mark like `cage-built` or `reopen + 3s`, got `{text}`");
        let (name, offset) = match text.split_once(' ') {
            None => (text.as_str(), None),
            Some((name, offset)) => (name, Some(offset)),
        };
        let mark = MarkName::new(name).ok_or_else(bad)?;
        let (earlier, offset) = match offset.map(|o| o.split_once(' ')) {
            None => (false, Duration::ZERO),
            Some(Some(("+", by))) => (false, tape::parse_duration(by).ok_or_else(bad)?),
            Some(Some(("-", by))) => (true, tape::parse_duration(by).ok_or_else(bad)?),
            Some(_) => return Err(bad()),
        };
        Ok(Self {
            mark,
            earlier,
            offset,
        })
    }
}

impl JsonSchema for TimeRef {
    fn schema_name() -> Cow<'static, str> {
        "Moment".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "A mark of the tape, `start` or `end`, optionally moved by a duration.",
            "pattern": "^[a-z0-9-]+( [+-] ([0-9]+ms|[0-9]+(\\.[0-9]+)?[sm]))?$",
            "examples": ["start", "cage-built", "reopen + 3s", "results - 0.5s"],
        })
    }
}

/// A mark by name.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
struct MarkRef(MarkName);

impl TryFrom<String> for MarkRef {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        MarkName::new(&text)
            .map(Self)
            .ok_or("a mark name takes lowercase letters, digits and hyphens")
    }
}

impl JsonSchema for MarkRef {
    fn schema_name() -> Cow<'static, str> {
        "MarkRef".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": "^[a-z0-9-]+$", "examples": ["results"] })
    }
}

/// Parses a YAML edit file.
///
/// # Errors
///
/// The first problem found, at its line and column.
pub fn parse(source: &str) -> Result<Edit, ParseError> {
    serde_saphyr::from_str(source).map_err(|e| tape::yaml_error(&e))
}

/// The JSON Schema of an edit file, for editors to complete and check against.
#[must_use]
pub fn schema() -> String {
    let schema = schemars::schema_for!(Edit);
    // Serializing a schema to JSON cannot fail; an empty string would fail the committed-schema test.
    serde_json::to_string_pretty(&schema).unwrap_or_default() + "\n"
}

/// An error at an item path such as `tracks.speed[1].from`.
fn error(path: String, message: impl Into<String>) -> ParseError {
    ParseError {
        at: Where::Step(path),
        message: message.into(),
    }
}

/// When an item starts and ends, as moments relative to marks.
struct Placement<'a> {
    path: String,
    from: &'a TimeRef,
    until: Option<&'a TimeRef>,
    duration: Option<Duration>,
}

impl Edit {
    /// Every item's placement, and each zoom's mark, as (path, mark name, whether it needs a position).
    fn references(&self) -> Vec<(String, &MarkName, bool)> {
        let mut refs = Vec::new();
        for placement in self.placements() {
            refs.push((
                format!("{}.from", placement.path),
                &placement.from.mark,
                false,
            ));
            if let Some(until) = placement.until {
                refs.push((format!("{}.until", placement.path), &until.mark, false));
            }
        }
        for (index, hold) in self.tracks.hold.iter().enumerate() {
            refs.push((format!("tracks.hold[{index}].at"), &hold.at.mark, false));
        }
        for (index, zoom) in self.tracks.zoom.iter().enumerate() {
            if let Some(MarkRef(mark)) = &zoom.at {
                refs.push((format!("tracks.zoom[{index}].at"), mark, true));
            }
            if let Some(MarkRef(mark)) = &zoom.window {
                refs.push((format!("tracks.zoom[{index}].window"), mark, true));
            }
        }
        refs
    }

    /// Where every item lies, track by track.
    fn placements(&self) -> Vec<Placement<'_>> {
        let tracks = &self.tracks;
        let chapter = tracks
            .chapter
            .iter()
            .enumerate()
            .map(|(i, item)| Placement {
                path: format!("tracks.chapter[{i}]"),
                from: &item.from,
                until: item.until.as_ref(),
                duration: item.duration,
            });
        let comment = tracks
            .comment
            .iter()
            .enumerate()
            .map(|(i, item)| Placement {
                path: format!("tracks.comment[{i}]"),
                from: &item.from,
                until: item.until.as_ref(),
                duration: item.duration,
            });
        let speed = tracks.speed.iter().enumerate().map(|(i, item)| Placement {
            path: format!("tracks.speed[{i}]"),
            from: &item.from,
            until: item.until.as_ref(),
            duration: item.duration,
        });
        let zoom = tracks.zoom.iter().enumerate().map(|(i, item)| Placement {
            path: format!("tracks.zoom[{i}]"),
            from: &item.from,
            until: item.until.as_ref(),
            duration: item.duration,
        });
        chapter.chain(comment).chain(speed).chain(zoom).collect()
    }
}

/// Checks that `edit` only names marks `tape` sets, and zooms only onto marks with a position.
///
/// # Errors
///
/// The first item naming a mark the tape lacks, or ending both with `until` and `duration`, or with neither.
pub fn check(edit: &Edit, tape: &Tape) -> Result<(), ParseError> {
    let marks: Vec<(&str, bool)> = [(MarkName::START, false), (MarkName::END, false)]
        .into_iter()
        .chain(tape.steps.iter().filter_map(|step| match step {
            Step::Mark { name, at } => Some((name.as_str(), at.is_some())),
            _ => None,
        }))
        .collect();
    validate(edit)?;
    for (path, mark, needs_position) in edit.references() {
        match marks.iter().find(|(name, _)| *name == mark.as_str()) {
            None => return Err(unknown(path, mark)),
            Some((_, false)) if needs_position => return Err(no_position(path, mark)),
            Some(_) => {}
        }
    }
    Ok(())
}

/// The rules an edit keeps whatever the recording: each item ends one way, speed-ups take time, zooms have
/// one centre and a scale in range.
fn validate(edit: &Edit) -> Result<(), ParseError> {
    for (index, hold) in edit.tracks.hold.iter().enumerate() {
        if hold.duration.is_zero() {
            return Err(error(
                format!("tracks.hold[{index}].duration"),
                "a hold must last longer than zero",
            ));
        }
    }
    for placement in edit.placements() {
        if placement.until.is_some() == placement.duration.is_some() {
            return Err(error(
                placement.path,
                "an item ends with exactly one of `until` or `duration`",
            ));
        }
    }
    for (index, item) in edit.tracks.speed.iter().enumerate() {
        if item.length.is_zero() {
            return Err(error(
                format!("tracks.speed[{index}].length"),
                "a speed-up must last longer than zero",
            ));
        }
    }
    for (index, item) in edit.tracks.zoom.iter().enumerate() {
        let centres = [
            item.at.is_some(),
            item.point.is_some(),
            item.window.is_some(),
        ];
        if centres.iter().filter(|given| **given).count() != 1 {
            return Err(error(
                format!("tracks.zoom[{index}]"),
                "a zoom takes exactly one of `at`, `point` or `window`",
            ));
        }
        if item.window.is_some() && item.scale.is_some() {
            return Err(error(
                format!("tracks.zoom[{index}].scale"),
                "a `window` zoom takes its scale from the window",
            ));
        }
        if scale(item).is_none() {
            return Err(error(
                format!("tracks.zoom[{index}].scale"),
                "`scale` must be between 1 and 3",
            ));
        }
    }
    Ok(())
}

/// How far a zoom item zooms in, if its scale lies in range.
fn scale(item: &ZoomItem) -> Option<ZoomScale> {
    item.scale
        .map_or(Some(ZoomScale::DEFAULT), ZoomScale::from_factor)
}

fn unknown(path: String, mark: &MarkName) -> ParseError {
    error(
        path,
        format!(
            "no mark `{}`; set it with a `mark` step in the tape",
            mark.as_str()
        ),
    )
}

fn no_position(path: String, mark: &MarkName) -> ParseError {
    error(
        path,
        format!(
            "mark `{}` has no position; give it one with `at`",
            mark.as_str()
        ),
    )
}

/// Where an item lies on the capture's clock: from its start until its end.
fn place(
    placement: &Placement<'_>,
    marks: &[MarkEntry],
) -> Result<(Duration, Duration), ParseError> {
    let from = moment(placement.from, marks, format!("{}.from", placement.path))?;
    let until = match (placement.until, placement.duration) {
        (Some(until), _) => moment(until, marks, format!("{}.until", placement.path))?,
        (None, duration) => from.saturating_add(duration.unwrap_or_default()),
    };
    if until <= from {
        return Err(error(
            placement.path.clone(),
            "an item must end after it starts",
        ));
    }
    Ok((from, until))
}

/// The time `at` names in a recording with `marks`.
fn moment(at: &TimeRef, marks: &[MarkEntry], path: String) -> Result<Duration, ParseError> {
    let mark = find(marks, at.mark.as_str()).ok_or_else(|| unknown(path, &at.mark))?;
    Ok(if at.earlier {
        mark.at.saturating_sub(at.offset)
    } else {
        mark.at.saturating_add(at.offset)
    })
}

fn find<'a>(marks: &'a [MarkEntry], name: &str) -> Option<&'a MarkEntry> {
    marks.iter().find(|mark| mark.name == name)
}

/// A view of the screen: its centre in pixels and how far it zooms in, in percent.
type View = (u32, u32, u16);

/// The full screen.
const FULL: View = (SCREEN.0 / 2, SCREEN.1 / 2, 100);

/// The timeline of `file` with `edit` on top of the tape's own after effects.
///
/// # Errors
///
/// An item naming a mark the recording lacks, ending before it starts, or speeding up a stretch another
/// speed-up covers, at the item's path.
pub fn apply(edit: &Edit, file: TimelineFile) -> Result<TimelineFile, ParseError> {
    validate(edit)?;
    let marks = &file.marks;
    let placements = edit.placements();
    let (chapter_items, rest) = placements.split_at(edit.tracks.chapter.len());
    let (comment_items, rest) = rest.split_at(edit.tracks.comment.len());
    let (speeds, zooms) = rest.split_at(edit.tracks.speed.len());

    // The tape's chapters below, the edit's above; an empty one hides what lies beneath.
    let chapters = layered_texts(&file.chapters, &edit.tracks.chapter, chapter_items, marks)?;
    let comments = layered_texts(&file.comments, &edit.tracks.comment, comment_items, marks)?;

    // Speed-ups side by side, never stacked.
    let mut warps: Vec<(WarpEntry, Option<&str>)> =
        file.warps.iter().map(|warp| (warp.clone(), None)).collect();
    for (item, placement) in edit.tracks.speed.iter().zip(speeds) {
        let (start, end) = place(placement, marks)?;
        let warp = WarpEntry {
            label: item.label.0.clone(),
            start,
            end,
            length: item.length,
        };
        warps.push((warp, Some(placement.path.as_str())));
    }
    warps.sort_by_key(|(warp, _)| warp.start);
    let overlap = warps
        .iter()
        .zip(warps.iter().skip(1))
        .find(|(before, after)| after.0.start < before.0.end);
    if let Some((before, after)) = overlap {
        // The tape's own warps never overlap, so one of the two is the edit's.
        let path = after.1.or(before.1).unwrap_or("tracks.speed");
        return Err(error(
            path.to_owned(),
            "speed-ups must not overlap; end one before the next starts",
        ));
    }
    let warps: Vec<WarpEntry> = warps.into_iter().map(|(warp, _)| warp).collect();

    // Holds stop the video between speed-ups, never within one.
    let mut holds = file.holds.clone();
    for (index, item) in edit.tracks.hold.iter().enumerate() {
        let path = format!("tracks.hold[{index}].at");
        let at = moment(&item.at, marks, path.clone())?;
        if warps.iter().any(|warp| warp.start < at && at < warp.end) {
            return Err(error(path, "a hold must not fall within a speed-up"));
        }
        holds.push(HoldEntry {
            at,
            length: item.duration,
        });
    }
    holds.sort_by_key(|hold| hold.at);

    // Zoom: the tape's changes of view become stretches, under the edit's.
    let end = find(marks, MarkName::END).map_or(Duration::MAX, |mark| mark.at);
    let mut layers: Vec<(Duration, Duration, View)> = file
        .zooms
        .iter()
        .zip(file.zooms.iter().skip(1).map(|next| next.at).chain([end]))
        .filter(|(zoom, until)| zoom.percent > 100 && zoom.at < *until)
        .map(|(zoom, until)| (zoom.at, until, (zoom.x, zoom.y, zoom.percent)))
        .collect();
    for (item, placement) in edit.tracks.zoom.iter().zip(zooms) {
        let (centre, percent) = match (&item.at, &item.point, &item.window) {
            (Some(MarkRef(name)), _, _) => {
                let path = format!("{}.at", placement.path);
                let mark = find(marks, name.as_str()).ok_or_else(|| unknown(path.clone(), name))?;
                let centre = mark.x.zip(mark.y).ok_or_else(|| no_position(path, name))?;
                (centre, scale(item).unwrap_or(ZoomScale::DEFAULT).percent())
            }
            (None, Some(point), _) => (
                (point.x, point.y),
                scale(item).unwrap_or(ZoomScale::DEFAULT).percent(),
            ),
            (None, None, Some(MarkRef(name))) => {
                let path = format!("{}.window", placement.path);
                let mark = find(marks, name.as_str()).ok_or_else(|| unknown(path.clone(), name))?;
                let window = mark.window.ok_or_else(|| {
                    error(
                        path,
                        format!("mark `{}` found no window under it", name.as_str()),
                    )
                })?;
                fit(window)
            }
            // `validate` rules this out.
            (None, None, None) => ((FULL.0, FULL.1), 100),
        };
        let (from, until) = place(placement, marks)?;
        let view = if percent == 100 {
            FULL
        } else {
            (centre.0, centre.1, percent)
        };
        layers.push((from, until, view));
    }
    let zooms = changes(&layer(&layers), end)
        .into_iter()
        .map(|(at, (x, y, percent))| ZoomEntry { at, x, y, percent })
        .collect();

    Ok(TimelineFile {
        warps,
        chapters,
        comments,
        holds,
        zooms,
        ..file
    })
}

/// The `items` of a chapter or comment track laid over `beneath`; an empty text hides what lies beneath.
fn layered_texts(
    beneath: &[TextEntry],
    items: &[TextItem],
    placements: &[Placement<'_>],
    marks: &[MarkEntry],
) -> Result<Vec<TextEntry>, ParseError> {
    let mut layers: Vec<(Duration, Duration, String)> = beneath
        .iter()
        .map(|text| (text.start, text.end, text.text.clone()))
        .collect();
    for (item, placement) in items.iter().zip(placements) {
        let (from, until) = place(placement, marks)?;
        layers.push((from, until, item.text.0.clone()));
    }
    Ok(layer(&layers)
        .into_iter()
        .filter(|(_, _, text)| !text.is_empty())
        .map(|(start, end, text)| TextEntry { text, start, end })
        .collect())
}

/// Space kept around a window a zoom frames, in pixels.
const WINDOW_MARGIN: u32 = 40;

/// The view framing `window` with [`WINDOW_MARGIN`] around it: its centre, and the zoom in percent as far as
/// it fits, from 100 to 300.
fn fit(window: WindowEntry) -> ((u32, u32), u16) {
    let room = |screen: u32, size: u32| {
        u64::from(screen) * 100 / u64::from(size + 2 * WINDOW_MARGIN).max(1)
    };
    let percent = room(SCREEN.0, window.w)
        .min(room(SCREEN.1, window.h))
        .clamp(100, 300);
    let centre = (
        window.x.saturating_add(window.w / 2),
        window.y.saturating_add(window.h / 2),
    );
    (centre, u16::try_from(percent).unwrap_or(300))
}

/// What shows where items lie on top of each other, later over earlier: runs of (start, end, value) in order,
/// with touching runs of one value joined.
fn layer<T: Clone + PartialEq>(items: &[(Duration, Duration, T)]) -> Vec<(Duration, Duration, T)> {
    let mut cuts: Vec<Duration> = items.iter().flat_map(|(s, e, _)| [*s, *e]).collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut runs: Vec<(Duration, Duration, T)> = Vec::new();
    for (&from, &to) in cuts.iter().zip(cuts.iter().skip(1)) {
        let Some((_, _, top)) = items
            .iter()
            .rev()
            .find(|(start, end, _)| *start <= from && to <= *end)
        else {
            continue;
        };
        match runs.last_mut() {
            Some((_, end, value)) if *end == from && value == top => *end = to,
            _ => runs.push((from, to, top.clone())),
        }
    }
    runs
}

/// The changes of view zoom `runs` make, back to the full screen between and after them, none from `end` on.
fn changes(runs: &[(Duration, Duration, View)], end: Duration) -> Vec<(Duration, View)> {
    // Each run, then the full screen where it ends; a run starting right there replaces that return.
    let mut wanted: Vec<(Duration, View)> = Vec::new();
    for (start, stop, view) in runs {
        if wanted.last().is_some_and(|(at, _)| at == start) {
            wanted.pop();
        }
        wanted.extend([(*start, *view), (*stop, FULL)]);
    }
    let mut now = FULL;
    wanted.retain(|(at, view)| {
        let keep = *view != now && *at < end;
        if keep {
            now = *view;
        }
        keep
    });
    wanted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn mark(name: &str, at: u64, position: Option<(u32, u32)>) -> MarkEntry {
        MarkEntry {
            name: name.into(),
            at: secs(at),
            x: position.map(|p| p.0),
            y: position.map(|p| p.1),
            window: None,
        }
    }

    /// A timeline from 0 s to 100 s, marking `built` at 40 s and `results` at 60 s on (400, 300).
    fn timeline() -> TimelineFile {
        TimelineFile {
            video: "t.raw.mkv".into(),
            warps: Vec::new(),
            chapters: Vec::new(),
            comments: Vec::new(),
            holds: Vec::new(),
            zooms: Vec::new(),
            marks: vec![
                mark("start", 0, None),
                mark("built", 40, None),
                mark("results", 60, Some((400, 300))),
                mark("end", 100, None),
            ],
            keys: Vec::new(),
        }
    }

    fn applied(yaml: &str, file: TimelineFile) -> Result<TimelineFile, ParseError> {
        apply(&parse(yaml).unwrap(), file)
    }

    fn shown(text: &str, start: u64, end: u64) -> TextEntry {
        TextEntry {
            text: text.into(),
            start: secs(start),
            end: secs(end),
        }
    }

    #[test]
    fn moments_take_an_offset_either_way() {
        let file = applied(
            "tracks:\n  chapter:\n    - { text: a, from: built + 2s, until: results - 500ms }\n",
            timeline(),
        )
        .unwrap();
        assert_eq!(file.chapters[0].start, secs(42));
        assert_eq!(file.chapters[0].end, Duration::from_millis(59_500));
        for bad in [
            "built +2s",
            "built + 2",
            "Built",
            "built * 2s",
            "built + 2s extra",
        ] {
            let yaml =
                format!("tracks:\n  chapter:\n    - {{ text: a, from: {bad}, duration: 1s }}\n");
            assert!(parse(&yaml).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_later_text_lies_on_top_and_an_empty_one_hides_those_beneath() {
        let mut file = timeline();
        file.chapters = vec![shown("tape", 0, 100)];
        let file = applied(
            "tracks:\n  chapter:\n    - { text: edit, from: built, until: results }\n    - { text: '', from: start + 90s, until: end }\n",
            file,
        )
        .unwrap();
        assert_eq!(
            file.chapters,
            [
                shown("tape", 0, 40),
                shown("edit", 40, 60),
                shown("tape", 60, 90)
            ]
        );
    }

    #[test]
    fn speed_ups_join_the_tapes_warps_in_order_and_never_overlap() {
        let mut file = timeline();
        file.warps = vec![WarpEntry {
            label: "tape".into(),
            start: secs(70),
            end: secs(80),
            length: secs(1),
        }];
        let file = applied(
            "tracks:\n  speed:\n    - { label: Building, length: 5s, from: start + 10s, until: built }\n",
            file,
        )
        .unwrap();
        let warps: Vec<_> = file
            .warps
            .iter()
            .map(|w| (w.label.as_str(), w.start))
            .collect();
        assert_eq!(warps, [("Building", secs(10)), ("tape", secs(70))]);

        let err = applied(
            "tracks:\n  speed:\n    - { label: a, length: 1s, from: start, until: results }\n    - { label: b, length: 1s, from: built, duration: 5s }\n",
            timeline(),
        )
        .unwrap_err();
        assert_eq!(err.at, Where::Step("tracks.speed[1]".into()), "{err}");
    }

    fn view(at: u64, x: u32, y: u32, percent: u16) -> ZoomEntry {
        ZoomEntry {
            at: secs(at),
            x,
            y,
            percent,
        }
    }

    fn window(x: u32, y: u32, w: u32, h: u32) -> WindowEntry {
        WindowEntry { x, y, w, h }
    }

    #[test]
    fn a_window_is_framed_as_far_in_as_it_fits() {
        // 1920 / (560 + 80) = 3, 1080 / (280 + 80) = 3.
        assert_eq!(fit(window(680, 400, 560, 280)), ((960, 540), 300));
        // A small dialog stops at 3×.
        assert_eq!(fit(window(800, 500, 200, 100)).1, 300);
        // 1920 / (1200 + 80) = 1.5; 1080 / (600 + 80) = 1.58.
        assert_eq!(fit(window(100, 200, 1200, 600)), ((700, 500), 150));
        // A window the screen hardly holds is not zoomed.
        assert_eq!(fit(window(0, 0, 1920, 1050)).1, 100);
    }

    #[test]
    fn a_window_zoom_frames_the_window_under_its_mark() {
        let mut file = timeline();
        file.marks[2].window = Some(window(100, 200, 1200, 600));
        let file = applied(
            "tracks:\n  zoom:\n    - { window: results, from: results, duration: 10s }\n",
            file,
        )
        .unwrap();
        assert_eq!(
            file.zooms,
            [view(60, 700, 500, 150), view(70, 960, 540, 100)]
        );
        let err = applied(
            "tracks:\n  zoom:\n    - { window: results, from: results, duration: 10s }\n",
            timeline(),
        )
        .unwrap_err();
        assert_eq!(err.at, Where::Step("tracks.zoom[0].window".into()));
        assert!(err.message.contains("found no window"), "{err}");
        for bad in [
            "tracks:\n  zoom:\n    - { window: results, at: results, from: start, until: end }\n",
            "tracks:\n  zoom:\n    - { window: results, scale: 2, from: start, until: end }\n",
        ] {
            assert!(applied(bad, timeline()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_zoom_onto_a_mark_eases_in_and_back_out() {
        let file = applied(
            "tracks:\n  zoom:\n    - { at: results, scale: 2, from: results - 1s, duration: 10s }\n",
            timeline(),
        )
        .unwrap();
        assert_eq!(
            file.zooms,
            [view(59, 400, 300, 200), view(69, 960, 540, 100)]
        );
    }

    #[test]
    fn an_edit_zoom_lies_over_the_tapes_and_hands_back_to_it() {
        let mut file = timeline();
        // The tape zooms in at 10 s and out at 50 s.
        file.zooms = vec![view(10, 100, 100, 150), view(50, 960, 540, 100)];
        let file = applied(
            "tracks:\n  zoom:\n    - { point: { x: 800, y: 600 }, from: start + 20s, until: start + 30s }\n    - { point: { x: 1, y: 1 }, scale: 1, from: start + 40s, until: end }\n",
            file,
        )
        .unwrap();
        assert_eq!(
            file.zooms,
            [
                view(10, 100, 100, 150),
                view(20, 800, 600, 150),
                view(30, 100, 100, 150),
                view(40, 960, 540, 100),
            ]
        );
    }

    #[test]
    fn a_tape_zoom_left_on_lasts_to_the_end() {
        let mut file = timeline();
        file.zooms = vec![view(10, 100, 100, 150)];
        let file = applied("tracks: {}\n", file).unwrap();
        assert_eq!(file.zooms, [view(10, 100, 100, 150)]);
    }

    #[test]
    fn errors_name_the_item() {
        let at = |yaml: &str| applied(yaml, timeline()).unwrap_err().at;
        assert_eq!(
            at("tracks:\n  chapter:\n    - { text: a, from: start, until: missing }\n"),
            Where::Step("tracks.chapter[0].until".into())
        );
        assert_eq!(
            at("tracks:\n  chapter:\n    - { text: a, from: results, until: built }\n"),
            Where::Step("tracks.chapter[0]".into())
        );
        assert_eq!(
            at("tracks:\n  chapter:\n    - { text: a, from: start, until: end, duration: 1s }\n"),
            Where::Step("tracks.chapter[0]".into())
        );
        assert_eq!(
            at("tracks:\n  zoom:\n    - { at: built, from: start, until: end }\n"),
            Where::Step("tracks.zoom[0].at".into())
        );
        assert_eq!(
            at("tracks:\n  zoom:\n    - { point: { x: 1, y: 1 }, scale: 4, from: start, until: end }\n"),
            Where::Step("tracks.zoom[0].scale".into())
        );
        assert!(matches!(
            parse("tracks:\n  sound: []\n").unwrap_err().at,
            Where::Position { line: 2, .. }
        ));
    }

    #[test]
    fn the_bar_sits_at_an_edge_with_a_card_length() {
        assert_eq!(parse("tracks: {}\n").unwrap().bar(), BarSettings::default());
        let edit = parse("bar: { at: bottom, offset: 120, card: 0s }\n").unwrap();
        assert_eq!(
            edit.bar(),
            BarSettings {
                at: Edge::Bottom,
                offset: 120,
                card: Duration::ZERO,
            }
        );
        for bad in [
            "bar: { at: left }\n",
            "bar: { offset: -1 }\n",
            "bar: { layout: top }\n",
        ] {
            let err = parse(bad).unwrap_err();
            assert!(
                matches!(err.at, Where::Position { line: 1, .. }),
                "{bad}: {err}"
            );
        }
    }

    #[test]
    fn comments_layer_on_their_own_track() {
        let mut file = timeline();
        file.chapters = vec![shown("tape", 0, 100)];
        let file = applied(
            "tracks:\n  comment:\n    - { text: note, from: built, until: results }\n",
            file,
        )
        .unwrap();
        assert_eq!(file.chapters, [shown("tape", 0, 100)]);
        assert_eq!(file.comments, [shown("note", 40, 60)]);
    }

    #[test]
    fn holds_fall_on_marks_outside_speed_ups() {
        let file = applied(
            "tracks:\n  hold:\n    - { at: results + 1s, duration: 3s }\n    - { at: built, duration: 2s }\n",
            timeline(),
        )
        .unwrap();
        let holds: Vec<(Duration, Duration)> =
            file.holds.iter().map(|h| (h.at, h.length)).collect();
        assert_eq!(holds, [(secs(40), secs(2)), (secs(61), secs(3))]);
        let err = applied(
            "tracks:\n  speed:\n    - { label: a, length: 1s, from: built, until: results }\n  hold:\n    - { at: built + 5s, duration: 1s }\n",
            timeline(),
        )
        .unwrap_err();
        assert_eq!(err.at, Where::Step("tracks.hold[0].at".into()));
        let zero = applied(
            "tracks:\n  hold:\n    - { at: built, duration: 0s }\n",
            timeline(),
        )
        .unwrap_err();
        assert_eq!(zero.at, Where::Step("tracks.hold[0].duration".into()));
    }

    #[test]
    fn check_holds_an_edit_to_the_tapes_marks() {
        let tape = tape::parse(
            "output: t.mp4\nsteps:\n  - mark: built\n  - mark: { name: results, at: { point: { x: 1, y: 1 } } }\n",
        )
        .unwrap();
        let ok =
            parse("tracks:\n  zoom:\n    - { at: results, from: built, until: end }\n").unwrap();
        assert_eq!(check(&ok, &tape), Ok(()));
        let bad =
            parse("tracks:\n  zoom:\n    - { at: built, from: start, until: nope }\n").unwrap();
        assert_eq!(
            check(&bad, &tape).unwrap_err().at,
            Where::Step("tracks.zoom[0].until".into())
        );
    }

    #[test]
    fn layers_join_touching_runs_of_one_value() {
        let runs = layer(&[
            (secs(0), secs(10), 'a'),
            (secs(5), secs(8), 'a'),
            (secs(20), secs(30), 'b'),
        ]);
        assert_eq!(runs, [(secs(0), secs(10), 'a'), (secs(20), secs(30), 'b')]);
    }

    #[test]
    fn committed_schema_is_current() {
        assert_eq!(
            include_str!("../schema/taperec-edit.schema.json"),
            schema(),
            "regenerate with ./test.sh run -q -- schema edit > schema/taperec-edit.schema.json"
        );
    }
}
