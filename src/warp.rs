//! Time warps: stretches of a capture fast-forwarded under a caption.

use std::ffi::OsString;
use std::fmt::{self, Write as _};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::recorder::{H264, MP4, QUIET};
use crate::tape::Step;

/// One warped stretch of the capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Where the stretch starts in the capture.
    pub start: Duration,
    /// Where the stretch ends in the capture.
    pub end: Duration,
    /// How long it lasts in the final video.
    pub length: Duration,
}

/// A finished warp, timed on the wall clock the capture stamps its frames with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marked {
    /// Where it lies in the capture and how long it lasts in the video.
    pub span: Span,
    /// The tape's label for it.
    pub label: String,
}

/// Collects warps as the tape plays.
#[derive(Debug, Default)]
pub struct Timeline {
    open: Option<(Duration, Duration, String)>,
    closed: Vec<Marked>,
}

impl Timeline {
    /// Notes the warp markers among steps, each happening at wall-clock time `at`.
    pub fn mark(&mut self, step: &Step, at: Duration) {
        match step {
            Step::Warp { label, length } => {
                self.open = Some((at, *length, label.as_str().to_owned()));
            }
            Step::EndWarp => {
                // The parser guarantees every EndWarp closes an open Warp.
                if let Some((start, length, label)) = self.open.take() {
                    self.closed.push(Marked {
                        span: Span {
                            start,
                            end: at,
                            length,
                        },
                        label,
                    });
                }
            }
            _ => {}
        }
    }

    /// The finished warps, in order.
    #[must_use]
    pub fn into_marked(self) -> Vec<Marked> {
        self.closed
    }
}

/// What a recording leaves next to its raw capture for rendering: the warps, on the capture's clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelineFile {
    /// The raw capture, relative to the timeline file.
    pub video: String,
    /// The warps, in order.
    pub warps: Vec<WarpEntry>,
}

/// One warp of a timeline file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarpEntry {
    /// The tape's label for it.
    pub label: String,
    /// When it started, in seconds since the Unix epoch, the clock the capture's frames carry.
    #[serde(with = "epoch_seconds")]
    pub start: Duration,
    /// When it ended, on the same clock.
    #[serde(with = "epoch_seconds")]
    pub end: Duration,
    /// How long it lasts in the final video, in seconds.
    #[serde(with = "epoch_seconds")]
    pub length: Duration,
    /// The caption image, relative to the timeline file.
    pub caption: String,
}

/// Durations as seconds with a fraction, e.g. `1792258264.258`.
mod epoch_seconds {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        value: &Duration,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(value.as_secs_f64())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Duration, D::Error> {
        let seconds = f64::deserialize(deserializer)?;
        Duration::try_from_secs_f64(seconds).map_err(serde::de::Error::custom)
    }
}

/// The spans of `warps` within a capture whose first frame carries `capture_start`.
#[must_use]
pub fn spans(warps: &[WarpEntry], capture_start: Duration) -> Vec<Span> {
    warps
        .iter()
        .map(|warp| Span {
            start: warp.start.saturating_sub(capture_start),
            end: warp.end.saturating_sub(capture_start),
            length: warp.length,
        })
        .collect()
}

/// The warps lasting at least `frame` in the capture; shorter ones play as normal footage.
#[must_use]
pub fn visible(warps: Vec<WarpEntry>, frame: Duration) -> Vec<WarpEntry> {
    warps
        .into_iter()
        .filter(|warp| warp.end.saturating_sub(warp.start) >= frame)
        .collect()
}

/// Why building a warp graph failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Spans overlap, run backwards or are out of order.
    BadSpans,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSpans => write!(f, "warp spans overlap or are out of order"),
        }
    }
}

impl std::error::Error for Error {}

/// The caption for a warp: its label and how long it really took.
#[must_use]
pub fn caption(label: &str, real: Duration) -> String {
    let seconds = real.as_secs();
    let took = match (seconds / 3600, seconds % 3600 / 60) {
        (0, 0) => format!("{seconds} s"),
        (0, minutes) => format!("{minutes} min"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    };
    format!("▶▶  {label}  ·  {took}")
}

/// The `-filter_complex` graph applying `spans` to input `0:v`, captioning warp `k` with input `k + 1`.
///
/// # Errors
///
/// [`Error::BadSpans`] for overlapping or unordered spans.
pub fn filter_graph(spans: &[Span], fps: NonZeroU32) -> Result<String, Error> {
    // Segments in output order.
    let mut segments: Vec<Segment> = Vec::new();
    let mut cursor = Duration::ZERO;
    for (caption, span) in (1..).zip(spans) {
        if span.start < cursor || span.end <= span.start {
            return Err(Error::BadSpans);
        }
        if span.start > cursor {
            segments.push(Segment::Normal(normal(cursor, Some(span.start))));
        }
        segments.push(Segment::Warped(warped(span), caption));
        cursor = span.end;
    }
    segments.push(Segment::Normal(normal(cursor, None)));

    let count = segments.len();
    // Writing to a `String` always succeeds.
    let mut graph = format!("[0:v]split={count}");
    for i in 0..count {
        let _ = write!(graph, "[i{i}]");
    }
    graph.push(';');
    for (i, segment) in segments.iter().enumerate() {
        let _ = match segment {
            Segment::Normal(chain) => write!(graph, "[i{i}]{chain}[v{i}];"),
            // `overlay` holds the caption's single frame for the whole segment.
            Segment::Warped(chain, caption) => write!(
                graph,
                "[i{i}]{chain}[w{i}];[w{i}][{caption}:v]overlay=x=(W-w)/2:y=H-h-140[v{i}];"
            ),
        };
    }
    for i in 0..count {
        let _ = write!(graph, "[v{i}]");
    }
    let _ = write!(graph, "concat=n={count}:v=1:a=0,fps={fps}[out]");
    Ok(graph)
}

/// One piece of the output, cut from the capture.
enum Segment {
    /// Played at real speed.
    Normal(String),
    /// Sped up under the caption from the given input index.
    Warped(String, usize),
}

/// A segment played at real speed; `end: None` runs to the end of the capture.
fn normal(start: Duration, end: Option<Duration>) -> String {
    let end = end
        .map(|e| format!(":end={}", seconds(e)))
        .unwrap_or_default();
    format!("trim=start={}{end},setpts=PTS-STARTPTS", seconds(start))
}

/// A segment sped up to `span.length` when it really took longer.
fn warped(span: &Span) -> String {
    let real = span.end.saturating_sub(span.start);
    let speed = if real > span.length {
        format!(
            "(PTS-STARTPTS)*{}/{}",
            span.length.as_millis(),
            real.as_millis()
        )
    } else {
        "PTS-STARTPTS".to_owned()
    };
    format!(
        "trim=start={}:end={},setpts={speed}",
        seconds(span.start),
        seconds(span.end)
    )
}

/// Seconds with millisecond precision, e.g. `2.500`.
fn seconds(duration: Duration) -> String {
    let millis = duration.as_millis();
    format!("{}.{:03}", millis / 1000, millis % 1000)
}

/// ffmpeg arguments rendering `capture` and its caption inputs through `graph` into `out`.
#[must_use]
pub fn render_args(capture: &Path, captions: &[PathBuf], graph: &str, out: &Path) -> Vec<OsString> {
    let inputs = std::iter::once(capture)
        .chain(captions.iter().map(PathBuf::as_path))
        .flat_map(|path| [OsString::from("-i"), path.as_os_str().to_owned()]);
    QUIET
        .map(OsString::from)
        .into_iter()
        .chain(inputs)
        .chain(["-filter_complex", graph, "-map", "[out]"].map(OsString::from))
        .chain(H264.map(OsString::from))
        .chain(MP4.map(OsString::from))
        .chain([out.as_os_str().to_owned()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn span(start: u64, end: u64, length: u64) -> Span {
        Span {
            start: secs(start),
            end: secs(end),
            length: secs(length),
        }
    }

    fn graph(spans: &[Span]) -> Result<String, Error> {
        filter_graph(spans, NonZeroU32::new(30).unwrap())
    }

    #[test]
    fn timeline_times_warps_between_markers() {
        let tape =
            crate::tape::parse(
            "output: t.mp4\nsteps:\n  - warp: { label: Build, length: 5s, steps: [sleep: 1s] }\n  - sleep: 1s\n",
        )
                .unwrap();
        let mut timeline = Timeline::default();
        for (step, at) in tape.steps.iter().zip([2, 3, 600, 601]) {
            timeline.mark(step, secs(at));
        }
        assert_eq!(
            timeline.into_marked(),
            [Marked {
                span: Span {
                    start: secs(2),
                    end: secs(600),
                    length: secs(5),
                },
                label: "Build".into()
            }]
        );
    }

    fn entry(start: Duration, end: Duration) -> WarpEntry {
        WarpEntry {
            label: "x".into(),
            start,
            end,
            length: secs(1),
            caption: "x.warp0.png".into(),
        }
    }

    /// A wall-clock instant, seconds since the Unix epoch.
    fn epoch(seconds: u64) -> Duration {
        secs(1_792_258_264 + seconds)
    }

    #[test]
    fn spans_sit_where_the_capture_clock_says() {
        let ms = Duration::from_millis;
        // The capture's first frame came 0.4 s after the tape started.
        let start = epoch(0) + ms(400);
        assert_eq!(
            spans(
                &[entry(epoch(5), epoch(30)), entry(epoch(40), epoch(50))],
                start
            ),
            [
                Span {
                    start: ms(4600),
                    end: ms(29_600),
                    length: secs(1)
                },
                Span {
                    start: ms(39_600),
                    end: ms(49_600),
                    length: secs(1)
                },
            ]
        );
    }

    #[test]
    fn a_stalled_capture_keeps_later_warps_in_place() {
        // Frames stop for minutes mid-way, yet a warp after the stall stays at its own time.
        let warps = [entry(epoch(600), epoch(3000))];
        assert_eq!(spans(&warps, epoch(0))[0].start, secs(600));
    }

    #[test]
    fn spans_saturate_at_the_capture_start() {
        assert_eq!(
            spans(&[entry(epoch(0), epoch(3))], epoch(1)),
            [Span {
                start: secs(0),
                end: secs(2),
                length: secs(1)
            }]
        );
    }

    #[test]
    fn keeps_only_warps_at_least_a_frame_long() {
        let ms = Duration::from_millis;
        let frame = ms(33);
        assert_eq!(visible(vec![entry(ms(100), ms(110))], frame), []);
        assert_eq!(
            visible(vec![entry(ms(100), ms(200))], frame),
            [entry(ms(100), ms(200))]
        );
    }

    #[test]
    fn timeline_files_round_trip_through_yaml() {
        let file = TimelineFile {
            video: "smoke.raw.mkv".into(),
            warps: vec![WarpEntry {
                label: "Building the cage".into(),
                start: epoch(10) + Duration::from_millis(250),
                end: epoch(2100),
                length: secs(5),
                caption: "smoke.warp0.png".into(),
            }],
        };
        let yaml = serde_saphyr::to_string(&file).unwrap();
        assert_eq!(
            serde_saphyr::from_str::<TimelineFile>(&yaml).unwrap(),
            file,
            "{yaml}"
        );
    }

    #[test]
    fn timeline_files_reject_unknown_fields_and_negative_times() {
        let base =
            "video: a.raw.mkv\nwarps:\n  - { label: a, start: 1, end: 2, length: 1, caption: a.png";
        assert!(serde_saphyr::from_str::<TimelineFile>(&format!("{base}, extra: 1 }}\n")).is_err());
        let negative = "video: a.raw.mkv\nwarps:\n  - { label: a, start: -1, end: 2, length: 1, caption: a.png }\n";
        assert!(serde_saphyr::from_str::<TimelineFile>(negative).is_err());
    }

    #[test]
    fn captions_show_label_and_real_time() {
        assert_eq!(
            caption("Building the cage", secs(14 * 60 + 20)),
            "▶▶  Building the cage  ·  14 min"
        );
        assert_eq!(caption("Installing", secs(45)), "▶▶  Installing  ·  45 s");
        assert_eq!(
            caption("Pulling", secs(65 * 60)),
            "▶▶  Pulling  ·  1 h 5 min"
        );
    }

    #[test]
    fn warps_a_middle_stretch_under_its_caption() {
        assert_eq!(
            graph(&[span(2, 602, 5)]).unwrap(),
            "[0:v]split=3[i0][i1][i2];\
             [i0]trim=start=0.000:end=2.000,setpts=PTS-STARTPTS[v0];\
             [i1]trim=start=2.000:end=602.000,setpts=(PTS-STARTPTS)*5000/600000[w1];\
             [w1][1:v]overlay=x=(W-w)/2:y=H-h-140[v1];\
             [i2]trim=start=602.000,setpts=PTS-STARTPTS[v2];\
             [v0][v1][v2]concat=n=3:v=1:a=0,fps=30[out]"
        );
    }

    #[test]
    fn each_warp_overlays_its_own_caption_input() {
        let g = graph(&[span(0, 10, 2), span(10, 20, 2)]).unwrap();
        assert!(
            g.starts_with("[0:v]split=3[i0][i1][i2];[i0]trim=start=0.000:end=10.000,"),
            "{g}"
        );
        assert!(g.contains("[w0][1:v]overlay="), "{g}");
        assert!(g.contains("[w1][2:v]overlay="), "{g}");
        assert!(
            g.ends_with("[v0][v1][v2]concat=n=3:v=1:a=0,fps=30[out]"),
            "{g}"
        );
    }

    #[test]
    fn short_warps_keep_real_speed() {
        let g = graph(&[span(1, 3, 5)]).unwrap();
        assert!(
            g.contains("[i1]trim=start=1.000:end=3.000,setpts=PTS-STARTPTS[w1];"),
            "{g}"
        );
    }

    #[test]
    fn rejects_bad_spans() {
        assert_eq!(graph(&[span(5, 4, 1)]), Err(Error::BadSpans));
        assert_eq!(
            graph(&[span(0, 10, 1), span(5, 20, 1)]),
            Err(Error::BadSpans)
        );
    }

    #[test]
    fn renders_with_the_capture_settings() {
        let captions = [PathBuf::from("/o/c0.png"), PathBuf::from("/o/c1.png")];
        let args: Vec<String> = render_args(
            Path::new("/o/.x.capture.mp4"),
            &captions,
            "G",
            Path::new("/o/x.mp4"),
        )
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
        #[rustfmt::skip]
        let expected = [
            "-hide_banner", "-loglevel", "error", "-y",
            "-i", "/o/.x.capture.mp4", "-i", "/o/c0.png", "-i", "/o/c1.png",
            "-filter_complex", "G", "-map", "[out]",
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
            "-movflags", "+faststart",
            "/o/x.mp4",
        ];
        assert_eq!(args, expected);
    }
}
