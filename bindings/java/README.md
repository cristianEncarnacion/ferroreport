# ferroreport for Java

**Reports in milliseconds from Java**: a template and your rows in; a PDF, an Excel workbook, a
Word document, a PowerPoint presentation, an OpenDocument spreadsheet or text, a CSV or a web
page out. Templates are the JasperReports `.jrxml` files you already have, rendered
without JasperReports (or anything else) on your classpath: this library has no dependencies.

Rows are read from a `ResultSet` or any `Iterable` as the report needs them and pages are
written as they are finished, so a report can be far larger than the heap: a million rows take
about 19 MB in the engine and a few megabytes in your JVM.

```java
Ferroreport reports = Ferroreport.local();

byte[] pdf = reports.template(Path.of("reports/invoice.jrxml"))
    .rows(lines)                                  // List<Map<String, Object>>, or any Iterable
    .param("CUSTOMER", "Distribuidora Caribe")
    .locale("es_DO")
    .render();

// Straight from the database to a file, in constant memory:
try (ResultSet sales = statement.executeQuery("select * from sales order by region")) {
    reports.template(Path.of("reports/sales.jrxml"))
        .rows(sales)
        .maxPages(50_000).timeout(Duration.ofMinutes(2))
        .renderTo(Path.of("sales.pdf"));          // .xlsx, .docx, .pptx, .ods, .odt, .csv and .html work the same way
}
```

## Install

From Maven Central:

```xml
<dependency>
  <groupId>io.github.cristianencarnacion</groupId>
  <artifactId>ferroreport</artifactId>
  <version>1.3.0</version>
</dependency>
```

```kotlin
// Gradle
dependencies { implementation("io.github.cristianencarnacion:ferroreport:1.3.0") }
```

Java 11 or newer. The library has no dependencies; JasperReports is not needed. (JitPack builds
it from this repository's releases too, as `com.github.cristianEncarnacion:ferroreport:v1.3.0`.)
Coming from `JasperFillManager`? The
[migration guide](https://cristianencarnacion.github.io/ferroreport/migrate.html) shows the same
call with this library, step by step.

## The engine

The library runs the native `ferroreport` engine, which it looks for in this order:

1. `-Dferroreport.bin=/path/to/ferroreport`, or the environment variable `FERROREPORT_BIN`;
2. `ferroreport` on the `PATH`;
3. a copy it downloads once from this project's
   [releases](https://github.com/cristianEncarnacion/ferroreport/releases) (the release with the
   library's own version, checked against the release's SHA-256 list) into the user's cache
   directory.

In a container, install the engine in the image and turn the download off
(`-Dferroreport.download=false`), so nothing is fetched at run time:

```dockerfile
COPY --from=ghcr.io/cristianencarnacion/ferroreport /ferroreport /usr/local/bin/ferroreport
```

Or run the engine as a server next to your application and talk to it over HTTP:

```java
Ferroreport reports = Ferroreport.server(URI.create("http://reports:8080"), System.getenv("FERROREPORT_TOKEN"));
byte[] pdf = reports.named("invoice").rows(lines).param("CUSTOMER", "Caribe").render();
```

## API

| Call | Does |
|---|---|
| `Ferroreport.local()`, `local(Path binary)` | The engine on this machine, as a child process per report |
| `Ferroreport.server(URI, token)` | A `ferroreport serve` instance (0.9.0 or later). Rows are sent to it one per line as they are read, so a large `ResultSet` is never in memory here or there, and the answer is the whole document or an exception |
| `.template(Path)`, `.source(String jrxml)`, `.named(String)` | A report from a file, from JRXML text, or by the name a server knows it by |
| `.rows(Iterable)`, `.rows(ResultSet)` | The rows, read one at a time. Maps of field name to value; nested Maps and Lists feed subreports and tables |
| `.param(name, value)`, `.params(Map)` | Report parameters |
| `.locale("es_DO")`, `.now(LocalDateTime)` | Number and date formatting; the value of `new Date()` in expressions |
| `.format(Format.PDF \| XLSX \| DOCX \| PPTX \| ODS \| ODT \| CSV \| HTML)` | The output format (`renderTo(Path)` goes by the extension) |
| `.maxPages(n)`, `.maxRows(n)`, `.timeout(Duration)` | Stop a report that grows past what you expect: `maxRows` counts the rows of subreports and datasets too (local engine; a server has its own limits) |
| `.csvSafe(true)`, `.xlsxNumbers(true)` | Guard CSV against spreadsheet formulas in the data; numbers as numbers in Excel and ODS |
| `.tagged(true)` | A tagged PDF, for screen readers: texts as paragraphs, pictures as figures |
| `.resources(Path)` | An extra directory for images and subreports |
| `.render()` | The document's bytes |
| `.renderTo(Path)` | Writes the file page by page; nothing is left behind if it fails |
| `.renderTo(OutputStream)` | Writes to a stream as the document is made (an HTTP response) |

Values are converted for you: `BigDecimal` keeps every digit, `LocalDate`, `LocalDateTime`,
`java.util.Date` and `java.sql` dates become dates, `byte[]` becomes an image. Errors are a
`FerroreportException` with the engine's message. `Ferroreport` is immutable and thread-safe; a
`Report` is built and used once.

## Quarkus and Spring

```java
@Path("/invoices")
public class InvoiceResource {
    private final Ferroreport reports = Ferroreport.local();

    @GET
    @Path("/{id}.pdf")
    @Produces("application/pdf")
    public Response pdf(@PathParam("id") long id) {
        Invoice invoice = Invoice.findById(id);
        StreamingOutput body = out -> reports.template(java.nio.file.Path.of("reports/invoice.jrxml"))
            .rows(invoice.lines())
            .param("CUSTOMER", invoice.customer)
            .locale("es_DO")
            .renderTo(out);
        return Response.ok(body).build();
    }
}
```

```java
@GetMapping(value = "/invoices/{id}.pdf", produces = "application/pdf")
public byte[] pdf(@PathVariable long id) {
    Invoice invoice = invoices.findById(id).orElseThrow();
    return reports.template(Path.of("reports/invoice.jrxml")).rows(invoice.getLines()).param("CUSTOMER", invoice.getCustomer()).render();
}
```

Rows can be your own objects' data as Maps (`Map.of("sku", line.sku, "qty", line.qty)`), or the
`ResultSet` of the query itself, which is the fastest path for large reports.
