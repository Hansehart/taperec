//! What the overlays show and when, in the final video's time: texts and speed-ups announce themselves in
//! the centre, then glide to where the layout keeps them; zooms and shortcuts show straight where they belong.

use std::time::Duration;

use crate::edit::{Layout, Position};
use crate::overlay::{Icon, Run};
use crate::warp::{self, Span, TimelineFile, WarpEntry, KEY_SHOWN};

/// How long a text or effect stays announced in the centre.
pub const ANNOUNCE: Duration = Duration::from_millis(1200);
/// How long it takes to glide from the centre to where it settles.
pub const GLIDE: Duration = Duration::from_millis(500);
/// Presses closer together than this share one pill.
pub const KEY_JOIN: Duration = Duration::from_millis(1000);
/// The most presses one pill shows; older ones drop out.
pub const KEY_GROUPS: usize = 6;

/// How an overlay looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Look {
    /// A near-opaque rounded box: texts and effects.
    Card,
    /// A translucent pill of keycaps: shortcuts.
    Pill,
}

/// One overlay over its lifetime, in the video's time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// What it shows.
    pub content: Vec<Run>,
    /// How it looks.
    pub look: Look,
    /// Where it settles.
    pub home: Position,
    /// When it appears.
    pub appear: Duration,
    /// When it goes.
    pub end: Duration,
    /// Whether it first announces itself in the centre.
    pub announce: bool,
}

/// One overlay at a moment: what it shows, and how far it has glided from the centre to its home.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    /// What it shows.
    pub content: Vec<Run>,
    /// How it looks.
    pub look: Look,
    /// Where it settles.
    pub home: Position,
    /// 0 while announced in the centre, 1 once settled.
    pub progress: f32,
}

/// What the video shows from `from` until `until`.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// When it starts.
    pub from: Duration,
    /// When it ends.
    pub until: Duration,
    /// The overlays, oldest first.
    pub shown: Vec<Shown>,
}

/// The overlays of a recording whose first frame carries `start`, with `spans` fast-forwarded; `warps` are
/// the visible ones. Nothing outlasts the recording's end, as the overlay would otherwise hold the video's
/// last frame until the overlays' stream ends.
#[must_use]
pub fn elements(
    timeline: &TimelineFile,
    warps: &[WarpEntry],
    spans: &[Span],
    start: Duration,
    layout: &Layout,
) -> Vec<Element> {
    let out = |t: Duration| warp::output_time(t.saturating_sub(start), spans);
    let end = out(warp::end_of(timeline));
    let mut elements = Vec::new();
    let mut card =
        |content: Vec<Run>, home: Position, appear: Duration, until: Duration, announce: bool| {
            if home != Position::Off {
                elements.push(Element {
                    content,
                    look: Look::Card,
                    home,
                    appear: out(appear),
                    end: out(until),
                    announce,
                });
            }
        };
    for text in timeline.texts.iter().filter(|text| !text.text.is_empty()) {
        card(
            vec![Run::Text(text.text.clone())],
            layout.text,
            text.start,
            text.end,
            true,
        );
    }
    for warp in warps {
        let real = warp.end.saturating_sub(warp.start);
        card(
            warp::caption(&warp.label, real),
            layout.effects,
            warp.start,
            warp.end,
            true,
        );
    }
    let zooms = &timeline.zooms;
    let ends = zooms
        .iter()
        .skip(1)
        .map(|next| next.at)
        .chain([warp::end_of(timeline)]);
    for (zoom, until) in zooms.iter().zip(ends) {
        if zoom.percent > 100 {
            let runs = vec![
                Run::Icon(Icon::ZoomIn),
                Run::Text(format!("  {}×", warp::factor(zoom.percent))),
            ];
            // The zoom points at the centre, so its badge goes straight to its corner.
            card(runs, layout.effects, zoom.at, until, false);
        }
    }
    if layout.keys != Position::Off {
        // Presses within a warp would only flash.
        let presses: Vec<(Duration, &Vec<String>)> = timeline
            .keys
            .iter()
            .filter(|key| !warps.iter().any(|w| w.start <= key.at && key.at < w.end))
            .map(|key| (out(key.at), &key.labels))
            .collect();
        elements.extend(pills(&presses, layout.keys));
    }
    for element in &mut elements {
        element.end = element.end.min(end);
    }
    elements.retain(|element| element.appear < element.end);
    elements.sort_by_key(|element| element.appear);
    // What was announced once comes back settled, e.g. a text resuming after one laid over it.
    let mut announced: Vec<Vec<Run>> = Vec::new();
    for element in elements.iter_mut().filter(|e| e.announce) {
        if announced.contains(&element.content) {
            element.announce = false;
        } else {
            announced.push(element.content.clone());
        }
    }
    elements
}

/// The pills for `presses`, in the video's time: one per press, holding the presses of its group so far.
fn pills(presses: &[(Duration, &Vec<String>)], home: Position) -> Vec<Element> {
    let mut pills = Vec::new();
    let mut group_start = 0;
    let mut before: Option<Duration> = None;
    for (index, (at, _)) in presses.iter().enumerate() {
        if before.is_some_and(|before| at.saturating_sub(before) >= KEY_JOIN) {
            group_start = index;
        }
        before = Some(*at);
        let first = group_start.max((index + 1).saturating_sub(KEY_GROUPS));
        let mut content = Vec::new();
        for (_, labels) in presses.get(first..=index).unwrap_or_default() {
            if !content.is_empty() {
                content.push(Run::Text("  ".to_owned()));
            }
            content.push(Run::Keys((*labels).clone()));
        }
        let shown_until = at.saturating_add(KEY_SHOWN);
        let end = presses
            .get(index + 1)
            .map_or(shown_until, |(next, _)| shown_until.min(*next));
        pills.push(Element {
            content,
            look: Look::Pill,
            home,
            appear: *at,
            end,
            announce: false,
        });
    }
    pills
}

impl Element {
    /// When it starts to glide home, if it announces itself.
    fn settle(&self) -> Option<Duration> {
        self.announce.then(|| self.appear.saturating_add(ANNOUNCE))
    }

    /// How far it has glided home at `t`: 0 in the centre, 1 settled.
    fn progress(&self, t: Duration) -> f32 {
        match self.settle() {
            None => 1.0,
            Some(settle) if t < settle => 0.0,
            Some(settle) => (t.saturating_sub(settle).as_secs_f32() / GLIDE.as_secs_f32()).min(1.0),
        }
    }

    fn is_gliding(&self, t: Duration) -> bool {
        self.settle()
            .is_some_and(|settle| settle <= t && t < settle.saturating_add(GLIDE))
    }
}

/// The frames to draw: one per stretch where nothing moves, one per `frame` while something glides.
#[must_use]
pub fn frames(elements: &[Element], frame: Duration) -> Vec<Frame> {
    let mut cuts: Vec<Duration> = elements
        .iter()
        .flat_map(|e| {
            let settle = e.settle();
            [
                Some(e.appear),
                Some(e.end),
                settle,
                settle.map(|s| s.saturating_add(GLIDE)),
            ]
        })
        .flatten()
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let shown = |t: Duration| -> Vec<Shown> {
        elements
            .iter()
            .filter(|e| e.appear <= t && t < e.end)
            .map(|e| Shown {
                content: e.content.clone(),
                look: e.look,
                home: e.home,
                progress: e.progress(t),
            })
            .collect()
    };
    let mut frames = Vec::new();
    for (&from, &until) in cuts.iter().zip(cuts.iter().skip(1)) {
        let moving = elements
            .iter()
            .any(|e| e.appear <= from && from < e.end && e.is_gliding(from));
        let step = if moving && !frame.is_zero() {
            frame
        } else {
            until.saturating_sub(from)
        };
        let mut t = from;
        while t < until {
            let next = t.saturating_add(step).min(until);
            let content = shown(t);
            if !content.is_empty() {
                frames.push(Frame {
                    from: t,
                    until: next,
                    shown: content,
                });
            }
            t = next;
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warp::{KeyEntry, MarkEntry, TextEntry, ZoomEntry};

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A timeline from 0 s to 100 s.
    fn timeline() -> TimelineFile {
        TimelineFile {
            video: "t.raw.mkv".into(),
            warps: Vec::new(),
            texts: Vec::new(),
            zooms: Vec::new(),
            marks: vec![MarkEntry {
                name: "end".into(),
                at: secs(100),
                x: None,
                y: None,
                window: None,
            }],
            keys: Vec::new(),
        }
    }

    fn text(text: &str, start: u64, end: u64) -> TextEntry {
        TextEntry {
            text: text.into(),
            start: secs(start),
            end: secs(end),
        }
    }

    fn key(at: Duration, label: &str) -> KeyEntry {
        KeyEntry {
            at,
            labels: vec![label.into()],
        }
    }

    fn warp(start: u64, end: u64, length: u64) -> WarpEntry {
        WarpEntry {
            label: "Building".into(),
            start: secs(start),
            end: secs(end),
            length: secs(length),
        }
    }

    fn all(file: &TimelineFile) -> Vec<Element> {
        let spans = warp::spans(&file.warps, Duration::ZERO);
        elements(
            file,
            &file.warps,
            &spans,
            Duration::ZERO,
            &Layout::default(),
        )
    }

    #[test]
    fn texts_announce_and_zooms_go_straight_to_their_corner() {
        let mut file = timeline();
        file.texts = vec![text("Part 1", 0, 50), text("", 50, 60)];
        file.zooms = vec![
            ZoomEntry {
                at: secs(10),
                x: 1,
                y: 1,
                percent: 150,
            },
            ZoomEntry {
                at: secs(20),
                x: 960,
                y: 540,
                percent: 100,
            },
        ];
        let got = all(&file);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(
            (got[0].home, got[0].appear, got[0].end),
            (Position::TopLeft, secs(0), secs(50))
        );
        assert_eq!(
            got[1].content,
            [Run::Icon(Icon::ZoomIn), Run::Text("  1.5×".into())]
        );
        assert_eq!(
            (got[1].home, got[1].appear, got[1].end),
            (Position::TopRight, secs(10), secs(20))
        );
        assert!(got.iter().all(|e| e.look == Look::Card));
        assert_eq!((got[0].announce, got[1].announce), (true, false));
        assert_eq!(got[0].progress(ms(1100)), 0.0);
        assert!((got[0].progress(ms(1450)) - 0.5).abs() < 1e-6);
        assert_eq!(got[0].progress(ms(1700)), 1.0);
    }

    #[test]
    fn what_was_announced_comes_back_settled() {
        let mut file = timeline();
        file.texts = vec![
            text("Part 1", 0, 10),
            text("Typing", 10, 20),
            text("Part 1", 20, 30),
        ];
        let announced: Vec<bool> = all(&file).iter().map(|e| e.announce).collect();
        assert_eq!(announced, [true, true, false]);
    }

    #[test]
    fn a_speed_up_shows_for_its_length_in_the_video() {
        let mut file = timeline();
        file.warps = vec![warp(10, 70, 6)];
        file.keys = vec![key(secs(30), "x")];
        let got = all(&file);
        assert_eq!(got.len(), 1, "a press within the warp is dropped: {got:?}");
        assert_eq!((got[0].appear, got[0].end), (secs(10), secs(16)));
        // The recording's end moves up by the 54 s the warp saves.
        let mut late = timeline();
        late.warps = vec![warp(10, 70, 6)];
        late.texts = vec![text("t", 0, 100)];
        assert_eq!(all(&late)[0].end, secs(46));
    }

    #[test]
    fn presses_close_together_share_a_pill_for_a_moment() {
        let mut file = timeline();
        file.keys = vec![key(secs(10), "a"), key(ms(10_500), "b"), key(secs(20), "c")];
        let got = all(&file);
        let pills: Vec<(Duration, Duration, usize)> = got
            .iter()
            .map(|e| {
                let keys = e
                    .content
                    .iter()
                    .filter(|r| matches!(r, Run::Keys(_)))
                    .count();
                (e.appear, e.end, keys)
            })
            .collect();
        assert_eq!(
            pills,
            [
                (secs(10), ms(10_500), 1),
                (ms(10_500), secs(12), 2),
                (secs(20), ms(21_500), 1)
            ]
        );
        assert!(got
            .iter()
            .all(|e| !e.announce && e.home == Position::Bottom));
    }

    #[test]
    fn a_pill_keeps_the_latest_presses() {
        let mut file = timeline();
        file.keys = (0..9).map(|i| key(secs(10) + ms(100 * i), "k")).collect();
        let last = all(&file).pop().unwrap();
        let keys = last
            .content
            .iter()
            .filter(|r| matches!(r, Run::Keys(_)))
            .count();
        assert_eq!(keys, KEY_GROUPS);
    }

    #[test]
    fn the_layout_moves_or_hides_each_kind() {
        let mut file = timeline();
        file.texts = vec![text("t", 0, 10)];
        file.keys = vec![key(secs(5), "k")];
        let layout = Layout {
            text: Position::BottomRight,
            effects: Position::TopRight,
            keys: Position::Off,
        };
        let got = elements(&file, &[], &[], Duration::ZERO, &layout);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].home, Position::BottomRight);
    }

    #[test]
    fn nothing_outlasts_the_recording() {
        let mut file = timeline();
        file.keys = vec![key(secs(99), "a"), key(secs(101), "b")];
        let got = all(&file);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].end, secs(100));
    }

    #[test]
    fn frames_hold_still_and_step_while_gliding() {
        let mut file = timeline();
        file.texts = vec![text("t", 0, 4)];
        let frames = frames(&all(&file), ms(100));
        let spans: Vec<(Duration, Duration)> = frames.iter().map(|f| (f.from, f.until)).collect();
        // Announced 0–1.2 s, five glide steps to 1.7 s, then settled until 4 s.
        assert_eq!(spans.first(), Some(&(Duration::ZERO, ms(1200))));
        assert_eq!(spans.len(), 1 + 5 + 1, "{spans:?}");
        assert_eq!(spans.last(), Some(&(ms(1700), secs(4))));
        assert_eq!(frames[0].shown[0].progress, 0.0);
        assert_eq!(frames.last().unwrap().shown[0].progress, 1.0);
    }

    #[test]
    fn an_element_gone_before_its_glide_never_moves() {
        let mut file = timeline();
        file.texts = vec![text("t", 0, 1)];
        let frames = frames(&all(&file), ms(100));
        assert_eq!(frames.len(), 1);
        assert_eq!((frames[0].from, frames[0].until), (Duration::ZERO, secs(1)));
    }
}
