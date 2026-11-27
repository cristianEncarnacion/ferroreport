# ferroreport for JavaScript

**Reports in milliseconds from JavaScript**: a template and your data in, a PDF, an Excel
workbook, a Word document, a PowerPoint presentation, an OpenDocument file (ODS, ODT), a CSV or
an HTML preview out. It reads **JasperReports `.jrxml` templates** and runs in Node.js,
Deno, Bun or the browser. No JVM: the engine is Rust compiled to WebAssembly, and a typical invoice renders
in a few milliseconds ([benchmarks](https://github.com/cristianEncarnacion/ferroreport#performance),
[live playground](https://cristianencarnacion.github.io/ferroreport/)). Output is verified pixel by pixel against
JasperReports 6.20, and templates cannot run code or read files, so they are safe to accept
from users.

```bash
npm install ferroreport
```

```js
// Node.js, Bun, Deno (npm:ferroreport)
import { render } from "ferroreport";
import { readFileSync, writeFileSync } from "node:fs";

const pdf = render(
  readFileSync("invoice.jrxml", "utf8"),
  [{ item: "Cemento Portland", qty: 10, price: 395 }],      // rows for the fields
  { CUSTOMER: "Distribuidora Caribe" },                      // parameters
  {
    locale: "es_DO",
    // Images and subreports referenced by the template, by the path it uses:
    resources: { "logo.png": readFileSync("logo.png"), "lines.jrxml": readFileSync("lines.jrxml") },
  },
);
writeFileSync("invoice.pdf", pdf);

// The same report as an Excel workbook (or CSV), laid out like JasperReports' exporters:
writeFileSync("invoice.xlsx", render(template, rows, params, { format: "xlsx" }));
```

In browsers (and bundlers that do not pick the Node build), initialise the module first:

```js
import init, { render } from "ferroreport";
await init();
const pdf = render(template, rows, params, { locale: "en_US" });
const url = URL.createObjectURL(new Blob([pdf], { type: "application/pdf" }));
```

## API

- `render(template, data?, params?, options?) => Uint8Array`
  - `template`: the JRXML source text.
  - `data`: array of row objects (or JSON text). Nested arrays feed subreports.
  - `params`: object with parameter values (or JSON text).
  - `options.locale`: `en_US`, `es_DO`, `es_MX`, `es_ES`, `fr`, `fr_FR`, `fr_BE`, `fr_CA`, `fr_CH`, `pt`, `pt_BR`, `pt_PT`, `it`, `it_IT`, `it_CH`, `de`, `de_DE`, `de_AT`, `de_CH`, ...
  - `options.resources`: `{ [path]: Uint8Array | string }` images (PNG, JPEG) and subreport
    templates (a subreport named `lines.jasper` is read from `lines.jrxml`).
  - `options.now`: value of `new Date()` in expressions (`Date` or ISO string).
  - `options.format`: `"pdf"` (default), `"xlsx"` (Excel workbook), `"docx"` (Word document), `"pptx"` (PowerPoint
    presentation), `"ods"` (OpenDocument
    spreadsheet), `"odt"` (OpenDocument text), `"csv"` (UTF-8 text) or
    `"html"` (a web page that looks like the PDF, for previews: UTF-8 text).
  - `options.maxPages`: stop with an error if the report grows past this many pages (set it
    when templates or data come from other people).
  - `options.maxRows`: stop with an error if the report reads more than this many rows, the
    rows of its subreports, tables, lists, charts and crosstabs included (a summary over many
    rows prints one page, which `maxPages` does not stop).
  - `options.csvSafe`: in CSV, put an apostrophe before values a spreadsheet would run as a
    formula. `options.xlsxNumbers`: in Excel and ODS, write numbers as numbers.
- `inspect(template) => { name, parameters, fields, warnings }`: `warnings` lists what the
  engine does not support in that template yet.
- `toTree(template, options?) => { tag, attrs, children, text? }`: any template (JRXML 6 or 7,
  or `.ferro`) as a tree of its JRXML 7 form, for editors; `attrs` are `[name, value]` pairs.
  `options.resources` holds the files a `.ferro` template imports. The
  [designer](https://cristianencarnacion.github.io/ferroreport/designer/) is built on it.
- `fromTree(tree, to?) => string`: the tree written back as a template, `"jrxml"` (default)
  or `"ferro"`.
- `version() => string`

Throws an `Error` with the reason when a template cannot be rendered.

See the [project README](https://github.com/cristianEncarnacion/ferroreport) for the list of
supported JasperReports features, benchmarks and other languages.
