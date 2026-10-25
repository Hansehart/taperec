# taperec

Scripted macOS tutorial videos: a YAML tape drives the desktop of a GitHub Actions macOS runner (clicks, typing, waits, fast-forwarded "warps") and taperec records it to MP4. Keep tapes in the repository they document and record them with the action:

```yaml
jobs:
  tutorials:
    runs-on: macos-26
    steps:
      - uses: actions/checkout@v5
      - id: taperec
        uses: Hansehart/taperec@v0
        with:
          tapes: tutorials/*.yaml
          dock: Terminal
      - uses: actions/upload-artifact@v4
        with:
          name: tutorials
          path: taperec/
```

| Input | Default | Meaning |
|---|---|---|
| `tapes` | (required) | Tape files to record, one glob per line |
| `out-dir` | `taperec` | Directory for the videos and failure screenshots |
| `dock` | (empty) | Apps the Dock shows, one name per line |
| `from-source` | `false` | Build taperec from the action's source instead of its release |
| `wallpaper` | gradient | Desktop background: a `#rrggbb` color or an image path, filled and cropped to 1920x1080 |
| `recorder` | `auto` | `ffmpeg`, `screencapturekit`, or `auto`: ScreenCaptureKit where the Mac encodes H.264 in hardware (Apple silicon), ffmpeg elsewhere (Intel) |

A tape lists steps; editors with YAML schema support (e.g. VS Code's YAML extension) complete and check them through the schema comment, and `taperec check <tape>...` validates tapes anywhere:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/Hansehart/taperec/main/schema/taperec.schema.json
output: intro.mp4
steps:
  - click: { dock: Terminal }
  - waitWindow: { title: sh, timeout: 30s }
  - paste: git clone https://github.com/Hansehart/o3s.git
  - key: return
  - text: Cloning o3s               # shown until the next text; "" hides it
  - zoom: { point: { x: 480, y: 200 } }   # zoom the video in there until the next zoom; `off` zooms out
  - warp: { label: Cloning, length: 2s, steps: [waitUntil: { command: test -d o3s, timeout: 2m }] }
  - click: { button: Open, timeout: 2m }
```

Each recording also leaves its raw material next to the video: the raw capture (`<name>.raw.mkv` or `.raw.mov`, its first frame's time readable by ffprobe) and `<name>.timeline.yaml`, what capture saw. `taperec render <name>.timeline.yaml [--out FILE]` rebuilds the video from them without recording again, on any machine with ffmpeg; recording needs the macOS runner, rendering does not.

To edit without recording again, a tape names moments with `mark: <name>` (or `mark: { name, at: <target> }` to record a position too) and links an edit file with `edit: <file>`. The edit file places items on tracks against those marks plus the implicit `start` and `end`; within a track a later item lies on top of an earlier one, except speed-ups, which must not overlap. `taperec render <name>.timeline.yaml --edit <file>` re-renders after changing it, and `taperec schema edit` prints its schema:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/Hansehart/taperec/main/schema/taperec-edit.schema.json
tracks:
  text:  [{ text: Building the cage, from: start, until: cage-built }]          # "" hides those beneath
  speed: [{ label: Building, length: 5s, from: reopen + 3s, until: cage-built }]
  zoom:  [{ at: results, scale: 1.75, from: results - 0.5s, duration: 6s }]    # or `point: { x, y }`
  # `window: <mark>` instead frames the window under the mark, as far in as it fits
layout: { text: top-left, effects: top-right, keys: bottom }   # the default; `off` hides a kind
```

Texts and speed-ups (label and real duration) announce themselves in the centre of the video the first time they show, then glide to where `layout` keeps them while they last; a zoom's factor goes straight there, as the zoom points at the centre; each shortcut and paste shows as keycaps above the Dock for 1.5 s, labelled by the recording platform, e.g. ⇧ ⌘ P.

The `videos` output lists the recorded files, one per line. The action runs on `macos-26` (Apple silicon) and `macos-26-intel`, which tapes that need Docker require, downloads the release binaries matching its version and runner and verifies their SHA-256. On Intel it restarts the crash-looping icon agent (actions/runner-images#14751) with Metal logging, rather than aborting on, a shader archive the runner's virtual GPU lacks, so app icons render. Pin it to a release tag or commit SHA. It caches only its own tooling (Homebrew downloads, and the Rust toolchain and build with `from-source`), never what a tape demonstrates.

Overlays are set in [Inter](https://github.com/rsms/inter) (SIL Open Font License 1.1) with icons from [Lucide](https://github.com/lucide-icons/lucide) (ISC); both ship inside the binary, their licenses in `assets/fonts/`, where `subset.sh` cuts the icons taperec uses from a Lucide release.

Contributing: run `pre-commit install`, work on a branch, and write commits as `type(scope): subject` with a scope from `.commitlintrc.json`.
