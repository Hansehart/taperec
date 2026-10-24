//! The YAML form of a tape: typed steps an editor completes and checks from the JSON Schema.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::time::Duration;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer};

use super::{
    duration, key_chord, output_file, AppName, ControlLabel, DockItem, KeyChord, MarkName,
    OutputFile, Point, ShellCommand, Step, Tape, Target, ValueError, WarpLabel, WindowName,
    ZoomScale, DEFAULT_WAIT, NAMED_KEYS,
};

/// A tape: the video it records and what happens on screen.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "taperec tape")]
pub(super) struct Document {
    /// The video file, a plain name ending in `.mp4`, e.g. `intro.mp4`.
    #[serde(deserialize_with = "de_output")]
    #[schemars(with = "OutputSchema")]
    output: OutputFile,
    /// What happens on screen, in order.
    steps: Vec<DocStep>,
    /// An edit file placing texts, speed-ups and zooms against the tape's marks, relative to the tape, e.g.
    /// `intro.edit.yaml`.
    #[serde(default)]
    #[schemars(with = "Name")]
    edit: Option<Name>,
}

/// One action of a tape.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "Step")]
enum DocStep {
    /// Glide the pointer to a target and left-click it.
    Click(DocTarget),
    /// Glide the pointer to a target.
    MoveTo(DocTarget),
    /// Type text keystroke by keystroke; prefer `paste` for anything long.
    Type(String),
    /// Put text on the clipboard and paste it with a single keystroke.
    Paste(String),
    /// Press a key chord, e.g. `return` or `primary+shift+p`.
    Key(
        #[serde(deserialize_with = "de_key")]
        #[schemars(with = "KeySchema")]
        KeyChord,
    ),
    /// Bring an app to the front by name, e.g. `Terminal`, and wait until it is.
    Focus(Name),
    /// Wait until a visible window's title contains the text, ignoring case.
    WaitWindow(WaitWindow),
    /// Wait until a shell command succeeds.
    WaitUntil(WaitUntil),
    /// Wait for a while.
    Sleep(
        #[serde(deserialize_with = "de_duration")]
        #[schemars(with = "DurationSchema")]
        Duration,
    ),
    /// Change the speed of the steps that follow.
    Set(Set),
    /// Show this text from here on, announced in the centre and then kept in its corner, e.g.
    /// `Part 1 · Docker`; `""` hides it.
    Text(Text),
    /// Zoom the video onto a target from here on, or back out with `off`.
    Zoom(DocZoomStep),
    /// Play the nested steps fast-forwarded into `length` under a caption.
    Warp(Warp),
    /// Name this moment for the edit file, e.g. `cage-built`, and with `at`, a position on screen.
    Mark(DocMark),
}

/// A mark: a name alone, or a name and a target whose position it records.
#[derive(Debug)]
enum DocMark {
    /// This moment.
    Here(DocMarkName),
    /// This moment and where the target is.
    At(DocMarkAt),
}

impl<'de> Deserialize<'de> for DocMark {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = DocMark;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a name such as `cage-built` or `{ name, at }`")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                DocMarkName::try_from(value.to_owned())
                    .map(DocMark::Here)
                    .map_err(E::custom)
            }

            // The map goes straight into `DocMarkAt`, keeping its own checks and messages.
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                DocMarkAt::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(DocMark::At)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl JsonSchema for DocMark {
    fn schema_name() -> Cow<'static, str> {
        "Mark".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let name = generator.subschema_for::<DocMarkName>();
        let at = generator.subschema_for::<DocMarkAt>();
        json_schema!({ "oneOf": [name, at] })
    }
}

/// A mark with the position of a target, for the edit file to zoom onto.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MarkAt")]
struct DocMarkAt {
    /// The mark's name.
    name: DocMarkName,
    /// What to locate, like a click target; the mark falls when it is found.
    at: DocTarget,
}

/// A mark's name: lowercase letters, digits and hyphens, other than the implicit `start` and `end`.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
struct DocMarkName(MarkName);

impl TryFrom<String> for DocMarkName {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        match MarkName::new(&text) {
            None => Err("a mark name takes lowercase letters, digits and hyphens"),
            Some(name) if [MarkName::START, MarkName::END].contains(&name.as_str()) => {
                Err("`start` and `end` mark every recording already")
            }
            Some(name) => Ok(Self(name)),
        }
    }
}

impl JsonSchema for DocMarkName {
    fn schema_name() -> Cow<'static, str> {
        "MarkName".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "Lowercase letters, digits and hyphens; `start` and `end` are implicit.",
            "pattern": "^[a-z0-9-]+$",
            "not": { "enum": ["start", "end"] },
            "examples": ["cage-built"],
        })
    }
}

/// Where the pointer goes: exactly one of `point`, `dock` or `button`.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Target")]
#[schemars(extend("oneOf" = [
    { "required": ["point"] },
    { "required": ["dock"] },
    { "required": ["button"] },
]))]
struct DocTarget {
    /// A screen position in pixels from the top-left corner.
    #[serde(default)]
    #[schemars(with = "DocPoint")]
    point: Option<DocPoint>,
    /// A Dock icon by its exact name, e.g. `Terminal`.
    #[serde(default)]
    #[schemars(with = "Name", extend("x-platforms" = ["macos"]))]
    dock: Option<Name>,
    /// A button by its exact label, e.g. `Open`, clicked once it appears.
    #[serde(default)]
    #[schemars(with = "Name")]
    button: Option<Name>,
    /// How long to wait for the button; only with `button`, 15s unless given.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    timeout: Option<Duration>,
}

/// A zoom onto a target, or `off` for the full screen.
#[derive(Debug)]
enum DocZoomStep {
    /// Back to the full screen.
    Off,
    /// Onto a target.
    On(DocZoom),
}

impl<'de> Deserialize<'de> for DocZoomStep {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = DocZoomStep;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("`off` or a target such as `{ button: Open }`")
            }

            // YAML 1.1 reads an unquoted `off` as `false`.
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                if value {
                    Err(E::invalid_value(serde::de::Unexpected::Bool(value), &self))
                } else {
                    Ok(DocZoomStep::Off)
                }
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value == "off" {
                    Ok(DocZoomStep::Off)
                } else {
                    Err(E::invalid_value(serde::de::Unexpected::Str(value), &self))
                }
            }

            // The map goes straight into `DocZoom`, keeping its own checks and messages.
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                map: A,
            ) -> Result<Self::Value, A::Error> {
                DocZoom::deserialize(serde::de::value::MapAccessDeserializer::new(map))
                    .map(DocZoomStep::On)
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

impl JsonSchema for DocZoomStep {
    fn schema_name() -> Cow<'static, str> {
        "ZoomStep".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let zoom = generator.subschema_for::<DocZoom>();
        json_schema!({ "oneOf": [{ "const": "off", "description": "Back to the full screen." }, zoom] })
    }
}

/// What to zoom onto: exactly one of `point`, `dock` or `button`, and how far.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Zoom")]
#[schemars(extend("oneOf" = [
    { "required": ["point"] },
    { "required": ["dock"] },
    { "required": ["button"] },
]))]
struct DocZoom {
    /// A screen position in pixels from the top-left corner.
    #[serde(default)]
    #[schemars(with = "DocPoint")]
    point: Option<DocPoint>,
    /// A Dock icon by its exact name, e.g. `Terminal`.
    #[serde(default)]
    #[schemars(with = "Name", extend("x-platforms" = ["macos"]))]
    dock: Option<Name>,
    /// A button by its exact label, e.g. `Open`, once it appears.
    #[serde(default)]
    #[schemars(with = "Name")]
    button: Option<Name>,
    /// How long to wait for the button; only with `button`, 15s unless given.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    timeout: Option<Duration>,
    /// How far to zoom in, from 1 to 3; 1.5 unless given. Text softens above about 1.5.
    #[serde(default)]
    #[schemars(range(min = 1.0, max = 3.0))]
    scale: Option<f64>,
}

/// A screen position in pixels from the top-left corner.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Point")]
pub(crate) struct DocPoint {
    /// Pixels from the left edge.
    pub(crate) x: u32,
    /// Pixels from the top edge.
    pub(crate) y: u32,
}

/// A window to wait for.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(deny_unknown_fields)]
struct WaitWindow {
    /// Text the title contains, ignoring case.
    title: Name,
    /// The longest wait, 15s unless given.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    timeout: Option<Duration>,
}

/// A shell command to wait for.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaitUntil {
    /// Run with `sh -c` until it exits 0.
    command: Name,
    /// Run with `sh -c` between attempts; exit 0 ends the wait as failed.
    #[serde(default)]
    #[schemars(with = "Name")]
    fail_when: Option<Name>,
    /// The longest wait, 15s unless given.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    timeout: Option<Duration>,
}

/// Speeds for the steps that follow.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(transform = without_defaults)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Set {
    /// Pause between keystrokes of `type`.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    typing_speed: Option<Duration>,
    /// Time each pointer glide takes.
    #[serde(default, deserialize_with = "de_opt_duration")]
    #[schemars(with = "DurationSchema")]
    move_duration: Option<Duration>,
}

/// A fast-forwarded stretch.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Warp {
    /// The caption shown while it plays, e.g. `Installing`.
    label: Name,
    /// How long it lasts in the video.
    #[serde(deserialize_with = "de_duration")]
    #[schemars(with = "DurationSchema")]
    length: Duration,
    /// What happens meanwhile; no warp inside a warp.
    steps: Vec<DocStep>,
}

/// Drops the `default: null` schemars adds to optional fields, so editors never suggest it.
pub(crate) fn without_defaults(schema: &mut Schema) {
    if let Some(properties) = schema.get_mut("properties").and_then(|p| p.as_object_mut()) {
        for property in properties.values_mut() {
            if let Some(property) = property.as_object_mut() {
                property.remove("default");
            }
        }
    }
}

/// Non-empty text without control characters.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Name(pub(crate) String);

impl TryFrom<String> for Name {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text.trim().is_empty() {
            Err("must not be empty")
        } else if text.chars().any(char::is_control) {
            Err("must not contain control characters")
        } else {
            Ok(Self(text))
        }
    }
}

impl JsonSchema for Name {
    fn schema_name() -> Cow<'static, str> {
        "Name".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "minLength": 1, "pattern": "\\S" })
    }
}

/// Text without control characters, possibly empty.
#[derive(Debug, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Text(pub(crate) String);

impl TryFrom<String> for Text {
    type Error = &'static str;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text.chars().any(char::is_control) {
            Err("must not contain control characters")
        } else {
            Ok(Self(text))
        }
    }
}

impl JsonSchema for Text {
    fn schema_name() -> Cow<'static, str> {
        "Text".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string" })
    }
}

/// Schema of a duration such as `500ms`, `1.5s` or `2m`.
pub(crate) struct DurationSchema;

impl JsonSchema for DurationSchema {
    fn schema_name() -> Cow<'static, str> {
        "Duration".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "A duration: whole milliseconds like `500ms`, or seconds or minutes like `1.5s` or `2m`.",
            "pattern": "^([0-9]+ms|[0-9]+(\\.[0-9]+)?[sm])$",
            "examples": ["500ms", "1.5s", "2m"],
        })
    }
}

/// Schema of a key chord: a named key or character, after `+`-joined modifiers.
struct KeySchema;

impl JsonSchema for KeySchema {
    fn schema_name() -> Cow<'static, str> {
        "KeyChord".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        let names = NAMED_KEYS.join("|");
        json_schema!({
            "type": "string",
            "description": "Modifiers (`primary`, `cmd`, `ctrl`, `alt`, `shift`, `fn`) joined by `+` before a named key or one character, e.g. `return` or `primary+shift+p`. `primary` is cmd on macOS.",
            "anyOf": [
                { "enum": NAMED_KEYS.to_vec() },
                { "pattern": format!("^((primary|cmd|ctrl|alt|shift|fn)\\+)*({names}|[!-*,-~])$") },
            ],
        })
    }
}

/// Schema of an output file name.
struct OutputSchema;

impl JsonSchema for OutputSchema {
    fn schema_name() -> Cow<'static, str> {
        "Output".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[A-Za-z0-9_-][A-Za-z0-9_.-]*\\.mp4$",
            "examples": ["intro.mp4"],
        })
    }
}

/// Reads a string with `parse`, turning its error into a deserialize error.
fn parsed<'de, D, T>(
    deserializer: D,
    parse: fn(&str) -> Result<T, ValueError>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    parse(&text).map_err(serde::de::Error::custom)
}

pub(crate) fn de_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Duration, D::Error> {
    parsed(deserializer, duration)
}

pub(crate) fn de_opt_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Duration>, D::Error> {
    parsed(deserializer, duration).map(Some)
}

fn de_key<'de, D: Deserializer<'de>>(deserializer: D) -> Result<KeyChord, D::Error> {
    parsed(deserializer, key_chord)
}

fn de_output<'de, D: Deserializer<'de>>(deserializer: D) -> Result<OutputFile, D::Error> {
    parsed(deserializer, output_file)
}

/// A rule the YAML shape allows but a tape must not break, at a step path like `steps[2].warp.steps[0]`.
pub(super) struct Violation {
    pub(super) path: String,
    pub(super) message: &'static str,
}

impl Document {
    /// Flattens the document into playable steps, warps becoming `Warp`/`EndWarp` pairs.
    pub(super) fn lower(self) -> Result<Tape, Violation> {
        let mut steps = Vec::new();
        lower_steps(self.steps, "steps", false, &mut BTreeSet::new(), &mut steps)?;
        Ok(Tape {
            output: self.output,
            steps,
            edit: self.edit.map(|Name(path)| path),
        })
    }
}

fn lower_steps(
    doc: Vec<DocStep>,
    at: &str,
    in_warp: bool,
    marks: &mut BTreeSet<MarkName>,
    out: &mut Vec<Step>,
) -> Result<(), Violation> {
    for (index, step) in doc.into_iter().enumerate() {
        let path = format!("{at}[{index}]");
        let violation = |message| Violation {
            path: path.clone(),
            message,
        };
        match step {
            DocStep::Click(target) => out.push(Step::Click(lower_target(target, &path)?)),
            DocStep::MoveTo(target) => out.push(Step::MoveTo(lower_target(target, &path)?)),
            DocStep::Type(text) => out.push(Step::Type(text)),
            DocStep::Paste(text) => out.push(Step::Paste(text)),
            DocStep::Text(Text(text)) => out.push(Step::Text(text)),
            DocStep::Zoom(DocZoomStep::Off) => out.push(Step::Zoom(None)),
            DocStep::Zoom(DocZoomStep::On(zoom)) => {
                let scale = match zoom.scale {
                    None => ZoomScale::DEFAULT,
                    Some(scale) => ZoomScale::from_factor(scale)
                        .ok_or_else(|| violation("`scale` must be between 1 and 3"))?,
                };
                let target = DocTarget {
                    point: zoom.point,
                    dock: zoom.dock,
                    button: zoom.button,
                    timeout: zoom.timeout,
                };
                out.push(Step::Zoom(Some((lower_target(target, &path)?, scale))));
            }
            DocStep::Key(chord) => out.push(Step::Key(chord)),
            DocStep::Focus(Name(app)) => out.push(Step::Focus {
                app: AppName(app),
                timeout: DEFAULT_WAIT,
            }),
            DocStep::WaitWindow(wait) => out.push(Step::WaitWindow {
                name: WindowName(wait.title.0),
                timeout: wait.timeout.unwrap_or(DEFAULT_WAIT),
            }),
            DocStep::WaitUntil(wait) => out.push(Step::WaitUntil {
                command: ShellCommand(wait.command.0),
                fail_when: wait.fail_when.map(|Name(command)| ShellCommand(command)),
                timeout: wait.timeout.unwrap_or(DEFAULT_WAIT),
            }),
            DocStep::Sleep(length) => out.push(Step::Sleep(length)),
            DocStep::Set(set) => {
                if set.typing_speed.is_none() && set.move_duration.is_none() {
                    return Err(violation("`set` needs `typingSpeed` or `moveDuration`"));
                }
                out.extend(set.typing_speed.map(Step::TypingSpeed));
                out.extend(set.move_duration.map(Step::MoveDuration));
            }
            DocStep::Warp(warp) => {
                if in_warp {
                    return Err(violation("a warp cannot sit inside another warp"));
                }
                if warp.length.is_zero() {
                    return Err(violation("a warp must last longer than zero"));
                }
                out.push(Step::Warp {
                    label: WarpLabel(warp.label.0),
                    length: warp.length,
                });
                lower_steps(warp.steps, &format!("{path}.warp.steps"), true, marks, out)?;
                out.push(Step::EndWarp);
            }
            DocStep::Mark(mark) => {
                let (DocMarkName(name), at) = match mark {
                    DocMark::Here(name) => (name, None),
                    DocMark::At(mark) => (mark.name, Some(lower_target(mark.at, &path)?)),
                };
                if !marks.insert(name.clone()) {
                    return Err(violation("a mark's name must be unique in its tape"));
                }
                out.push(Step::Mark { name, at });
            }
        }
    }
    Ok(())
}

fn lower_target(target: DocTarget, path: &str) -> Result<Target, Violation> {
    let violation = |message| Violation {
        path: path.to_owned(),
        message,
    };
    match (target.point, target.dock, target.button, target.timeout) {
        (Some(DocPoint { x, y }), None, None, None) => Ok(Target::Point(Point { x, y })),
        (None, Some(Name(name)), None, None) => Ok(Target::Dock(DockItem(name))),
        (None, None, Some(Name(label)), timeout) => Ok(Target::Control {
            label: ControlLabel(label),
            timeout: timeout.unwrap_or(DEFAULT_WAIT),
        }),
        (None, None, None, _) => Err(violation("a target needs `point`, `dock` or `button`")),
        (_, _, None, Some(_)) => Err(violation("`timeout` only applies to a `button` target")),
        _ => Err(violation(
            "a target takes exactly one of `point`, `dock` or `button`",
        )),
    }
}

/// The JSON Schema of a tape, for editors to complete and check against.
#[must_use]
pub fn schema() -> String {
    let schema = schemars::schema_for!(Document);
    // Serializing a schema to JSON cannot fail; an empty string would fail the committed-schema test.
    serde_json::to_string_pretty(&schema).unwrap_or_default() + "\n"
}
