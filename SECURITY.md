# Security

ferroreport is built to render templates and data that come from people you do not trust. This
page says what that means, what limits protect the process, and how to report a problem.

## Reporting a vulnerability

Please report security problems privately through GitHub's
[security advisories](https://github.com/cristianEncarnacion/ferroreport/security/advisories/new)
rather than in a public issue. Include a template and data that show the problem if you can. You
will get an answer within a few days, and a fix as soon as possible.

## What a template cannot do

- **Run code.** Expressions are interpreted from a fixed list of operations (string, number, date
  and collection methods, formatting). There is no reflection, no class loading, no `Runtime`, no
  Groovy `execute()`, and compiled `.jasper` files are never deserialized: a subreport named
  `x.jasper` is read from `x.jrxml`.
- **Read files outside its resource directories.** Images and subreports are read only from the
  template's own directory and the ones you add. Absolute paths, `..` and symbolic links that
  point outside those directories are refused. Without a resource directory nothing is read
  from disk (the JavaScript build never touches it).
- **Use the network.** `http://`, `https://` and `file://` locations are not fetched.
- **Expand XML entities.** Templates with a `DOCTYPE` are refused, so entity bombs and external
  entities (XXE) cannot work.

## Limits on resources

A template or its data cannot make a render use unbounded stack, memory or time:

| Limit | Value |
|---|---|
| Element nesting in a template | 100 levels |
| Expression nesting (parentheses, operators, calls) | 256 levels |
| Image size | 64 million pixels, checked before decoding |
| Subreport nesting | 8 levels |
| Subreports, tables and lists filled in one render | 500,000 |
| Sizes in a template (widths, heights, font sizes) | finite numbers up to 10 million; `inf`, `NaN` and the like are refused |
| Pages in a report | yours to set: `--max-pages`, `Limits::max_pages`, `max_pages=`, `maxPages` |
| Rows read in one render (the report's and those of its subreports, tables, lists, charts and crosstabs, together) | yours to set: `--max-rows`, `Renderer::max_rows`, `max_rows=`, `maxRows`, `.maxRows(n)` |
| Time for a render | yours to set: `--timeout`, `Limits::timeout`, `timeout=`; 60 s by default in `ferroreport serve` |
| One row of JSON data | 256 MB by default (`--max-row`, `JsonRows::max_row_bytes`); the buffer never grows past the limit before the row is refused |
| HTTP request body (`ferroreport serve`) | 50 MB by default (`--max-body`). Rows sent one per line (`application/x-ndjson`) are read as they are needed and have no size limit of their own: the time, page and row limits stop them |
| Memory for one document waiting to be sent | 32 MB by default (`--spool-memory`); the rest goes to a file |
| Disk for documents waiting to be sent, all requests together | 4 GB by default (`--spool-disk`); over it the answer is `507` |

Other inputs grow the work in proportion to their size: pages follow the rows of the data. That
is what the page, row and time limits are for: set them whenever templates or data come from other
people. A report can read many rows and print few pages (only a summary, totals by group with an
empty detail, one chart over all the rows): the row limit stops it at the first row past it,
where the page limit would not stop it at all. The time limit is checked on every page, every few hundred rows and at every
subreport, table and list, so it also stops work that happens inside them. (In the browser
build there is no clock to check: use `maxPages` and `maxRows` there.)

Memory does not grow with the report: rows are read as they are needed and each page is
written as it is finished, in PDF, Excel, Word, CSV and HTML alike. What is kept until the end
is small and grows only with what the report declares: page totals waiting for the last page
("page X of Y"), Excel's styles, merged ranges, pictures and links, the PDF's bookmarks and
named anchors. Encrypted PDFs are encrypted object by object as they are written.

## PDF encryption

`--user-password`, `--owner-password` and `--permissions` (and the template's
`net.sf.jasperreports.export.pdf.encrypted` properties) encrypt PDFs with AES-256, the standard
security handler of PDF 2.0 (revision 6). Keys, salts and initialization vectors come from the
operating system's random numbers. Without an owner password, a random one nobody knows is used,
so the permissions cannot be lifted. Permissions are honoured by readers, not enforced by
cryptography: anyone who can open the document can read it, so use a user password for
documents that must stay private. Pass passwords through `FERROREPORT_PDF_USER_PASSWORD` and
`FERROREPORT_PDF_OWNER_PASSWORD` rather than the command line, where other users of the machine
can see them; ferroreport never writes them to its output or its logs.

## How this is tested

- **Mutation tests on every push.** Each test template and its data are damaged thousands of
  ways (bytes flipped, pieces cut, repeated and swapped, numbers blown up, markup injected) and
  rendered to PDF, Excel and CSV on a 1 MB stack under a page and time limit. The engine must
  answer with a document or an error: a panic or a hang fails the build. Run other mutations
  with `HOSTILE_SEED=<n> cargo test --test hostile`. These tests found, before any release
  carried them, an infinite loop on an infinite cell height, a crash in the Excel exporter on
  negative sizes, a crash on a color with non-ASCII characters and an overflow on a font of
  size 0.
- **Attack templates.** Deep nesting, image bombs, links out of the resource directory and
  subreports that multiply are each covered by a test.
- **Dependencies.** `cargo deny` runs on every push and once a week: known vulnerabilities,
  licenses (permissive only) and sources (crates.io only).

## The HTTP server

`ferroreport serve` listens on `127.0.0.1` unless told otherwise, and refuses to start on any
other address without a token (`--token` or `FERROREPORT_TOKEN`): requests must then send
`Authorization: Bearer <token>`, compared in constant time. If the port can only be reached by
your own services (a private network, a sidecar), say so with `--allow-anonymous` or
`FERROREPORT_ALLOW_ANONYMOUS=1`. The Docker image listens on `0.0.0.0`, so it needs one or the
other. Template names in URLs must be
relative paths inside `--templates`. A failure while rendering one request answers `500` and
does not stop the server. `GET /health` answers without the token (it only says the server is up); every other
path, `GET /metrics` included, needs it.

The access log (`--access-log`, plain or `json`) holds the method, the path without its query,
the status, the time, the request ID and, for a render, the template's name, the format and how
many pages, rows and bytes came out, and the error message the client was sent. It never holds
the token, the request body, parameter values or rows. An error message can quote a template
name or a field name from the request. A client's `X-Request-Id` is kept only when it is
printable ASCII of 128 characters or fewer, so it cannot break a log line or a header.

Every render runs under `--timeout` (60 seconds unless you change it), `--max-pages` and
`--max-rows` (no limit unless you set one); a render that goes over answers `413` and is counted
in `ferroreport_render_limited_total`. The number of renders at
once is `--workers`, and `--rate-limit <n>` caps the requests per minute from one client address
(over it, the answer is `429` with `Retry-After`). Behind a reverse proxy every request comes
from the proxy's address, so set the limit there instead.

A document is kept until it is complete (in memory up to `--spool-memory`, then in a file in
`--spool-dir`, unlinked as soon as it is created so that it cannot outlive the server) and
sent with its length, so an error is never half a document. The files of all requests together
stay under `--spool-disk`; a document that does not fit is answered with `507`. A client can
ask for the document as it renders instead (`Prefer: respond-streaming`): nothing is kept
then, a client that disconnects stops its render within milliseconds, and a render that fails
after its first bytes ends the response in a way HTTP clients report as an error rather than
as a complete document. Without that header, a client that disconnects is only noticed when
the document is sent, so its render costs at most `--timeout`.

## Spreadsheet exports

XLSX and CSV exports contain the report's text exactly as JasperReports writes it. A value that
starts with `=`, `+`, `-` or `@` is written as text in XLSX, but spreadsheet programs may treat
it as a formula when they open a CSV file. If your data comes from other people and the CSV
will be opened in a spreadsheet, turn on the formula guard (`--csv-safe`, `csv_safe=True`,
`csvSafe: true`, `CsvOptions::guard_formulas`): such values get an apostrophe in front, which
spreadsheets show as text. Plain numbers such as `-12.50` are left alone. It is off by default
so that the CSV stays byte for byte what JasperReports writes.

## HTML output

The HTML export writes the report's text as text. Everything that comes from the template or the
data is escaped, so a value such as `<script>` shows as those characters and cannot run. The
page holds no scripts and loads nothing from the network: images are embedded.

## The Java library

The Java library runs the engine as a child process (or talks to a server). If no engine is
installed it downloads the release with its own version from this project's GitHub releases over
HTTPS and checks it against the release's SHA-256 list before installing it in the user's cache
directory. Where programs may not be fetched at run time, install the engine in the image and
set `-Dferroreport.download=false`.

## Dependencies

`cargo deny` (see `deny.toml`) reports no known vulnerabilities in the dependencies. Two are
listed as no longer maintained, with no known flaws: `chrono` (dates) and `ttf-parser` (fonts,
which only reads the fonts built in and the ones you register).
