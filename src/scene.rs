//! What the bar and the chapter cards show, and when, in the final video's time: chapters, comments, effects
//! and shortcuts each keep their own zone of one bar; a new chapter first shows on a card while the video holds.

use std::time::Duration;

use crate::overlay::{Icon, Run};
use crate::warp::{self, Hold, TimelineFile, Timing, WarpEntry, KEY_SHOWN};

/// Presses closer together than this share the bar's shortcut zone.
pub const KEY_JOIN: Duration = Duration::from_millis(1000);
/// The most presses the zone shows; older ones drop out.
pub const KEY_GROUPS: usize = 6;

/// What the bar shows at one moment, zone by zone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BarState {
    /// Where we are.
    pub chapter: Option<String>,
    /// What to notice now.
    pub comment: Option<String>,
    /// The running speed-up and zoom, each as runs.
    pub effects: Vec<Vec<Run>>,
    /// The latest shortcuts, one group of keycaps per press.
    pub keys: Vec<Vec<String>>,
}

impl BarState {
    /// Whether the bar has nothing to show.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chapter.is_none()
            && self.comment.is_none()
            && self.effects.is_empty()
            && self.keys.is_empty()
    }
}

/// What lies over the video for a while.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Show {
    /// The bar.
    Bar(BarState),
    /// A new chapter's card over the dimmed screen, while the video holds.
    Card(String),
}

/// What lies over the video from `from` until `until`, in the video's time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// When it starts.
    pub from: Duration,
    /// When it ends.
    pub until: Duration,
    /// What shows.
    pub show: Show,
}

/// The holds that cards add: one of `card` where each chapter first starts, on the clock of a capture whose
/// first frame carries `start`.
#[must_use]
pub fn card_holds(timeline: &TimelineFile, start: Duration, card: Duration) -> Vec<Hold> {
    if card.is_zero() {
        return Vec::new();
    }
    first_chapters(timeline)
        .map(|chapter| Hold {
            at: chapter.start.saturating_sub(start),
            length: card,
        })
        .collect()
}

/// Each chapter the first time its title shows; a title resuming after another is no new chapter.
fn first_chapters(timeline: &TimelineFile) -> impl Iterator<Item = &warp::TextEntry> {
    let mut seen: Vec<&str> = Vec::new();
    timeline.chapters.iter().filter(move |chapter| {
        let new = !seen.contains(&chapter.text.as_str());
        if new {
            seen.push(&chapter.text);
        }
        new
    })
}

/// One thing the bar or a card shows over a stretch of the video.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Chapter(String),
    Comment(String),
    /// An effect, ordered speed-up first.
    Effect(u8, Vec<Run>),
    Keys(Vec<Vec<String>>),
    Card(String),
}

/// What lies over the video of a capture whose first frame carries `start`, frame by frame: a card while a new
/// chapter's hold lasts, else the bar while it has something to show. `warps` are the visible ones, `timing`
/// maps the capture to the video, and nothing outlasts the recording's end.
#[must_use]
pub fn frames(
    timeline: &TimelineFile,
    warps: &[WarpEntry],
    timing: &Timing,
    start: Duration,
    card: Duration,
) -> Vec<Frame> {
    let out = |t: Duration| timing.output(t.saturating_sub(start));
    let end = out(warp::end_of(timeline));
    let mut parts: Vec<(Duration, Duration, Part)> = Vec::new();
    for chapter in &timeline.chapters {
        parts.push((
            out(chapter.start),
            out(chapter.end),
            Part::Chapter(chapter.text.clone()),
        ));
    }
    if !card.is_zero() {
        for chapter in first_chapters(timeline) {
            let from = out(chapter.start);
            parts.push((
                from,
                from.saturating_add(card),
                Part::Card(chapter.text.clone()),
            ));
        }
    }
    for comment in &timeline.comments {
        parts.push((
            out(comment.start),
            out(comment.end),
            Part::Comment(comment.text.clone()),
        ));
    }
    for warp in warps {
        let real = warp.end.saturating_sub(warp.start);
        let runs = warp::caption(&warp.label, real);
        parts.push((out(warp.start), out(warp.end), Part::Effect(0, runs)));
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
            parts.push((out(zoom.at), out(until), Part::Effect(1, runs)));
        }
    }
    // Presses within a warp would only flash.
    let presses: Vec<(Duration, &Vec<String>)> = timeline
        .keys
        .iter()
        .filter(|key| !warps.iter().any(|w| w.start <= key.at && key.at < w.end))
        .map(|key| (out(key.at), &key.labels))
        .collect();
    parts.extend(key_groups(&presses));
    for part in &mut parts {
        part.1 = part.1.min(end);
    }
    parts.retain(|(from, until, _)| from < until);

    let mut cuts: Vec<Duration> = parts
        .iter()
        .flat_map(|(from, until, _)| [*from, *until])
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut frames: Vec<Frame> = Vec::new();
    for (&from, &until) in cuts.iter().zip(cuts.iter().skip(1)) {
        let active: Vec<&Part> = parts
            .iter()
            .filter(|(start, stop, _)| *start <= from && until <= *stop)
            .map(|(_, _, part)| part)
            .collect();
        let Some(show) = compose(&active) else {
            continue;
        };
        match frames.last_mut() {
            Some(last) if last.until == from && last.show == show => last.until = until,
            _ => frames.push(Frame { from, until, show }),
        }
    }
    frames
}

/// What the parts active together show: a card above all, else the bar if it has anything.
fn compose(active: &[&Part]) -> Option<Show> {
    if let Some(title) = active.iter().find_map(|part| match part {
        Part::Card(title) => Some(title.clone()),
        _ => None,
    }) {
        return Some(Show::Card(title));
    }
    let mut bar = BarState::default();
    let mut effects: Vec<(u8, Vec<Run>)> = Vec::new();
    for part in active {
        match part {
            Part::Chapter(text) => bar.chapter = Some(text.clone()),
            Part::Comment(text) => bar.comment = Some(text.clone()),
            Part::Effect(order, runs) => effects.push((*order, runs.clone())),
            Part::Keys(groups) => bar.keys.clone_from(groups),
            Part::Card(_) => {}
        }
    }
    effects.sort_by_key(|(order, _)| *order);
    bar.effects = effects.into_iter().map(|(_, runs)| runs).collect();
    (!bar.is_empty()).then_some(Show::Bar(bar))
}

/// The shortcut zone's contents over time: each press joins the presses less than [`KEY_JOIN`] before it, up
/// to [`KEY_GROUPS`], and shows for [`KEY_SHOWN`] unless the next press comes sooner.
fn key_groups(presses: &[(Duration, &Vec<String>)]) -> Vec<(Duration, Duration, Part)> {
    let mut groups = Vec::new();
    let mut group_start = 0;
    let mut before: Option<Duration> = None;
    for (index, (at, _)) in presses.iter().enumerate() {
        if before.is_some_and(|before| at.saturating_sub(before) >= KEY_JOIN) {
            group_start = index;
        }
        before = Some(*at);
        let first = group_start.max((index + 1).saturating_sub(KEY_GROUPS));
        let shown: Vec<Vec<String>> = presses
            .get(first..=index)
            .unwrap_or_default()
            .iter()
            .map(|(_, labels)| (*labels).clone())
            .collect();
        let shown_until = at.saturating_add(KEY_SHOWN);
        let end = presses
            .get(index + 1)
            .map_or(shown_until, |(next, _)| shown_until.min(*next));
        groups.push((*at, end, Part::Keys(shown)));
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::warp::{KeyEntry, MarkEntry, Span, TextEntry, ZoomEntry};

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
            chapters: Vec::new(),
            comments: Vec::new(),
            holds: Vec::new(),
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

    fn bar(frame: &Frame) -> &BarState {
        match &frame.show {
            Show::Bar(bar) => bar,
            Show::Card(title) => panic!("a card for {title}"),
        }
    }

    fn plain(file: &TimelineFile) -> Vec<Frame> {
        let timing = Timing::default();
        frames(file, &file.warps, &timing, Duration::ZERO, Duration::ZERO)
    }

    #[test]
    fn each_kind_keeps_its_zone() {
        let mut file = timeline();
        file.chapters = vec![text("Part 1", 0, 50)];
        file.comments = vec![text("Look here", 10, 20)];
        file.zooms = vec![
            ZoomEntry {
                at: secs(15),
                x: 1,
                y: 1,
                percent: 150,
            },
            ZoomEntry {
                at: secs(30),
                x: 960,
                y: 540,
                percent: 100,
            },
        ];
        file.keys = vec![key(secs(16), "a")];
        let frames = plain(&file);
        let at = |t: u64| {
            frames
                .iter()
                .find(|f| f.from <= secs(t) && secs(t) < f.until)
                .map(bar)
        };
        let busy = at(16).unwrap();
        assert_eq!(busy.chapter.as_deref(), Some("Part 1"));
        assert_eq!(busy.comment.as_deref(), Some("Look here"));
        assert_eq!(
            busy.effects,
            [vec![Run::Icon(Icon::ZoomIn), Run::Text("  1.5×".into())]]
        );
        assert_eq!(busy.keys, [vec!["a".to_owned()]]);
        assert_eq!(
            at(40).unwrap(),
            &BarState {
                chapter: Some("Part 1".into()),
                ..BarState::default()
            }
        );
        assert!(at(60).is_none(), "the bar goes once it has nothing");
    }

    #[test]
    fn presses_close_together_share_the_zone_for_a_moment() {
        let mut file = timeline();
        file.keys = vec![key(secs(10), "a"), key(ms(10_500), "b"), key(secs(20), "c")];
        let groups: Vec<(Duration, Duration, usize)> = plain(&file)
            .iter()
            .map(|f| (f.from, f.until, bar(f).keys.len()))
            .collect();
        assert_eq!(
            groups,
            [
                (secs(10), ms(10_500), 1),
                (ms(10_500), secs(12), 2),
                (secs(20), ms(21_500), 1)
            ]
        );
    }

    #[test]
    fn a_new_chapter_shows_on_a_card_while_the_video_holds() {
        let mut file = timeline();
        file.chapters = vec![
            text("Part 1", 0, 10),
            text("Aside", 10, 20),
            text("Part 1", 20, 30),
        ];
        let holds = card_holds(&file, Duration::ZERO, secs(2));
        // Resuming "Part 1" is no new chapter.
        assert_eq!(
            holds,
            [
                Hold {
                    at: secs(0),
                    length: secs(2)
                },
                Hold {
                    at: secs(10),
                    length: secs(2)
                }
            ]
        );
        let timing = Timing {
            spans: Vec::new(),
            holds,
        };
        let frames = frames(&file, &[], &timing, Duration::ZERO, secs(2));
        let shows: Vec<(Duration, Duration, String)> = frames
            .iter()
            .map(|f| {
                let what = match &f.show {
                    Show::Card(title) => format!("card {title}"),
                    Show::Bar(bar) => format!("bar {}", bar.chapter.clone().unwrap_or_default()),
                };
                (f.from, f.until, what)
            })
            .collect();
        assert_eq!(
            shows,
            [
                (secs(0), secs(2), "card Part 1".into()),
                (secs(2), secs(12), "bar Part 1".into()),
                (secs(12), secs(14), "card Aside".into()),
                (secs(14), secs(24), "bar Aside".into()),
                (secs(24), secs(34), "bar Part 1".into()),
            ]
        );
        assert_eq!(card_holds(&file, Duration::ZERO, Duration::ZERO), []);
    }

    #[test]
    fn a_speed_up_shows_in_the_effects_zone_and_hides_presses() {
        let mut file = timeline();
        file.warps = vec![WarpEntry {
            label: "Building".into(),
            start: secs(10),
            end: secs(70),
            length: secs(6),
        }];
        file.keys = vec![key(secs(30), "x")];
        let timing = Timing {
            spans: warp::spans(&file.warps, Duration::ZERO),
            holds: Vec::new(),
        };
        let frames = frames(&file, &file.warps, &timing, Duration::ZERO, Duration::ZERO);
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert_eq!((frames[0].from, frames[0].until), (secs(10), secs(16)));
        assert_eq!(
            bar(&frames[0]).effects,
            [warp::caption("Building", secs(60))]
        );
        assert_eq!(bar(&frames[0]).keys, Vec::<Vec<String>>::new());
    }

    #[test]
    fn holds_push_later_moments_back() {
        let mut file = timeline();
        file.comments = vec![text("after", 20, 30)];
        let timing = Timing {
            spans: vec![Span {
                start: secs(40),
                end: secs(50),
                length: secs(10),
            }],
            holds: vec![Hold {
                at: secs(10),
                length: secs(3),
            }],
        };
        let frames = frames(&file, &[], &timing, Duration::ZERO, Duration::ZERO);
        assert_eq!((frames[0].from, frames[0].until), (secs(23), secs(33)));
    }

    #[test]
    fn nothing_outlasts_the_recording() {
        let mut file = timeline();
        file.keys = vec![key(secs(99), "a"), key(secs(101), "b")];
        let frames = plain(&file);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].until, secs(100));
    }
}
