# Contributing

Thanks for helping! You don't need to know Rust to make a difference here.

## Where to start

- **A report that comes out wrong.** The most valuable contribution is a **fixture**: a JRXML
  template plus sample data that ferroreport renders differently from JasperReports (or cannot
  render). [Open an issue](https://github.com/cristianEncarnacion/ferroreport/issues/new/choose)
  with the template and a screenshot of both outputs; remove anything private from the data.
- **A first pull request.** Issues labelled
  [`good first issue`](https://github.com/cristianEncarnacion/ferroreport/labels/good%20first%20issue)
  are small and self-contained, with a pointer to the code to change. Comment on one to take it.
- **Something bigger.** Issues labelled
  [`help wanted`](https://github.com/cristianEncarnacion/ferroreport/labels/help%20wanted) are
  the roadmap. Say in the issue how you plan to do it before writing a lot of code, so we can
  agree on the approach first.
- **A question or an idea.** Use [Discussions](https://github.com/cristianEncarnacion/ferroreport/discussions).

## Layout

```
crates/ferroreport/      library: parser, expression interpreter, filler, PDF writer
crates/ferroreport-cli/  `ferroreport` command and HTTP server
bindings/wasm/           JavaScript package (npm), WebAssembly
bindings/python/         Python package (PyPI)
bindings/java/           Java package (Maven Central)
editors/vscode/          VS Code extension for `.ferro`
site/                    website, documentation and the visual designer (site/designer/)
fixtures/<name>/         report.jrxml, data.json, params.json (optional), expected.json; fonts.txt
                         and fontsets.txt (--font, --font-set), jasper-extension/ (the same
                         font sets for the reference run)
tools/compare.py         pixel comparison against JasperReports
tools/compat.py          what changed since the previous release (pages, pixels, CSV)
tools/compare_sheets.py  cell comparison of the Excel and CSV exports
tools/bench.py           speed and memory of one fixture, cold
tools/bench_all.py       every number in the README: cold, warm JVM, each output format,
                         100,000 to 1,000,000 rows (--wait-idle on a shared machine)
tools/bench_concurrent.py  many reports at once, in one process
tools/oracle/            the reference renderer (a small Java program around JasperReports)
```

## Everyday commands

```bash
cargo test                      # unit tests, every fixture's page count, streaming, mutation tests
HOSTILE_SEED=7 cargo test --test hostile   # other mutations of the templates than the usual ones
cargo deny check                # advisories, licenses and sources of the dependencies
cargo clippy --all-targets
cargo fmt
```

## Comparing against JasperReports

You need a JDK (17 or newer) and a directory with the JasperReports 6.x jars and their
dependencies, including `jasperreports-fonts` (the DejaVu families). Then:

```bash
cargo build --release
JASPER_LIB=/path/to/jars python3 tools/compare.py            # all fixtures
JASPER_LIB=/path/to/jars python3 tools/compare.py --only 02  # one fixture
```

Results and diff images (red: only in Jasper, blue: only in ferroreport) are written to
`target/compare/`. When a new fixture matches, record its page count in `expected.json` so
`cargo test` keeps it from regressing.

## Comparing with the previous release

Within 1.x the output may only move closer to JasperReports (Stability in the README).
`tools/compat.py` shows what a change prints differently from a release: every fixture and
example is rendered by both, and their pages (pixel by pixel, as `tools/compare.py`), page
counts and CSV text compared (`--xlsx` adds the workbooks). It needs `pdftoppm` (poppler) and
Python with numpy and Pillow, not Java.

```bash
cargo build --release
python3 tools/compat.py --old 1.3.0                 # the release, downloaded and checked against SHA256SUMS
python3 tools/compat.py --old path/to/ferroreport   # or any other build
```

It writes `target/compat/report.md`, one row per template (the same, or changed: the worst
page's share and the first CSV difference), and a diff picture of each page that changed
(red: only in the old output, blue: only in the new one). `--strict` exits with 1 when anything
changed. CI runs it on every pull request against the latest release, without failing: the
report is in the job summary and the `compat` artifact. Every change it lists should be one the
pull request means, with its line in `CHANGELOG.md`.

## Adding a fixture

1. Create `fixtures/NN-short-name/` with `report.jrxml` and, if needed, `data.json` (an array of
   objects keyed by field name) and `params.json`.
2. Use the DejaVu font families in the template so both engines use the same fonts.
3. Run the comparison. If it differs, open an issue with the diff image, or a pull request
   with the fix.

## Rules of thumb

- Behaviour must match JasperReports; when in doubt, measure it with the reference renderer
  and write down the rule you derived (see `text.rs` for an example).
- The expression interpreter must stay an allow-list. Never add reflection, class loading,
  file or network access.

## Pull requests

- Keep them small: one fix or one feature each. A failing fixture or test that the change
  makes pass is the best description.
- Before pushing: `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
  CI also runs the bindings, the minimum Rust version and the PDF/UA and PDF/A checks.
- Add a line under `## Unreleased` at the top of `CHANGELOG.md` saying what changes for people
  who use ferroreport.
- Existing templates must keep rendering as they do: within 1.x, changes are additive (see
  Stability in the README).
- By contributing you agree that your work is released under the project's licenses, MIT or
  Apache-2.0, at the user's option.

## Conduct

Be kind and assume good faith; see [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
