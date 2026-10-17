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

A tape lists steps; editors with YAML schema support (e.g. VS Code's YAML extension) complete and check them through the schema comment, and `taperec check <tape>...` validates tapes anywhere:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/Hansehart/taperec/main/schema/taperec.schema.json
output: intro.mp4
steps:
  - click: { dock: Terminal }
  - waitWindow: { title: sh, timeout: 30s }
  - paste: git clone https://github.com/Hansehart/o3s.git
  - key: return
  - warp: { label: Cloning, length: 2s, steps: [waitUntil: { command: test -d o3s, timeout: 2m }] }
  - click: { button: Open, timeout: 2m }
```

Each recording also leaves its raw material next to the video (`<name>.raw.mkv`, `<name>.timeline.yaml` and the warp captions), so `taperec render <name>.timeline.yaml [--out FILE]` rebuilds the video anywhere ffmpeg runs, without recording again.

The `videos` output lists the recorded files, one per line. The action runs on `macos-26` (Apple silicon) and `macos-26-intel`, which tapes that need Docker require, downloads the release binaries matching its version and runner and verifies their SHA-256. On Intel it switches off the crash-looping icon agent (actions/runner-images#14751), so apps the Dock has not cached show a generic icon. Pin it to a release tag or commit SHA. It caches only its own tooling (Homebrew downloads, and the Rust toolchain and build with `from-source`), never what a tape demonstrates.

Contributing: run `pre-commit install`, work on a branch, and write commits as `type(scope): subject` with a scope from `.commitlintrc.json`.
