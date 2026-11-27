# ferroreport for Python

**Reports in milliseconds from Python**: a template and your data in, a PDF, an Excel workbook,
a Word document, a PowerPoint presentation, an OpenDocument file (ODS, ODT), a CSV or an HTML
preview out. An invoice takes about 3 ms, and a million rows take about 18 seconds and 19 MB
of memory, because rows are read as they are needed and pages are written as they are finished.

It reads **JasperReports `.jrxml` templates** and gives the same output, verified pixel by pixel
against JasperReports 6.20, with no JVM and no Java bridge: the engine is native Rust, about
200× faster than JasperReports from a cold start and 3 to 5× faster than a warm JVM
([benchmarks](https://github.com/cristianEncarnacion/ferroreport#performance)). Templates
cannot run code, so they are safe to accept from users.

```bash
pip install ferroreport
```

```python
import ferroreport

pdf = ferroreport.render(
    "invoice.jrxml",                                   # path or JRXML source text
    data=[{"item": "Cemento Portland", "qty": 10, "price": 395}],
    params={"CUSTOMER": "Distribuidora Caribe"},
    locale="es_DO",
)
with open("invoice.pdf", "wb") as f:
    f.write(pdf)
```

- `data` is a list of dicts (or objects); `datetime`, `date`, `Decimal` and `bytes` (images)
  are converted to the classes the template declares. Nested lists feed subreports.
- Images and subreports are found next to the template; add `resource_dirs=[...]`, or pass
  them in memory with `resources={"logo.png": b"...", "lines.jrxml": "..."}`. A subreport
  named `lines.jasper` is read from `lines.jrxml`; compiled `.jasper` files are never loaded.
- `format="xlsx"` returns an Excel workbook, `format="docx"` a Word document, `format="pptx"` a
  PowerPoint presentation (a slide per page), `format="ods"` and
  `format="odt"` OpenDocument files (LibreOffice), and `format="csv"` CSV text (UTF-8 bytes), laid
  out like JasperReports' exporters; `format="html"` returns a web page that looks like the PDF,
  for previews in a browser.
- `data` can also be any iterator of rows (a generator, a database cursor): it is read one row
  at a time, so the rows are never all in memory.
- `ferroreport.render_to_file(template, "out.pdf", data, params)` writes the file as it is made
  (PDF and CSV pages go to disk as they are finished); the format follows the extension
  (`.pdf`, `.xlsx`, `.docx`, `.pptx`, `.ods`, `.odt`, `.csv`, `.html`). It returns `{"pages": ..., "rows": ...}`.
- For rows that do not fit in memory, give `data_file="rows.jsonl"` instead of `data`: a JSON
  array or one object per line, read as the report needs it. A million rows render in about
  19 MB.
- `max_pages=`, `max_rows=` and `timeout=` (seconds) stop a report that grows past what you
  expect (`max_rows=` counts the rows of subreports and datasets too, and stops a summary over
  a million rows that `max_pages=` would not); set them when templates or data come from other
  people. `csv_safe=True` guards CSV against
  spreadsheet formulas in the data, and `xlsx_numbers=True` writes numbers as numbers (Excel and ODS).
- `ferroreport.inspect(template)` returns the template's parameters, fields and the features
  the engine does not support yet.
- Rendering releases the GIL, so it runs in parallel from threads (Django, FastAPI, Celery).

Errors raise `ferroreport.FerroreportError`.

See the [project README](https://github.com/cristianEncarnacion/ferroreport) for supported
JasperReports features, benchmarks and other languages.
