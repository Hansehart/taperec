# taperec

Scripted macOS tutorial videos: a `.tape` file drives the desktop of a GitHub Actions macOS runner (clicks, typing, waits, fast-forwarded "warps") and taperec records it to MP4. Keep tapes in the repository they document and record them with the action:

```yaml
jobs:
  tutorials:
    runs-on: macos-26
    steps:
      - uses: actions/checkout@v5
      - id: taperec
        uses: Hansehart/taperec@v0
        with:
          tapes: tutorials/*.tape
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

The `videos` output lists the recorded files, one per line. The action runs on `macos-26` (Apple silicon), downloads the release binaries matching its version and verifies their SHA-256. Pin it to a release tag or commit SHA. It caches only its own tooling (Homebrew downloads, and the Rust toolchain and build with `from-source`), never what a tape demonstrates.

Contributing: run `pre-commit install`, work on a branch, and write commits as `type(scope): subject` with a scope from `.commitlintrc.json`.
