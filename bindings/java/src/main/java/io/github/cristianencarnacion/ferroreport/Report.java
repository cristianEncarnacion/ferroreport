package io.github.cristianencarnacion.ferroreport;

import java.io.BufferedWriter;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.sql.SQLException;
import java.time.Duration;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.NoSuchElementException;
import java.util.Objects;
import java.util.concurrent.atomic.AtomicReference;

/**
 * One report about to be rendered: its template, rows, parameters and options. Build it from a
 * {@link Ferroreport}, then call {@link #render()} or one of the {@code renderTo} methods. A
 * {@code Report} is used once and by one thread.
 */
public final class Report {
    private final Ferroreport engine;
    private final Path template;
    private final String source;
    private final String name;
    private Iterable<?> rows;
    private final Map<String, Object> params = new LinkedHashMap<>();
    private final List<Path> resources = new ArrayList<>();
    private String locale = "en_US";
    private Format format;
    private Integer maxPages;
    private Integer maxRows;
    private Duration timeout;
    private LocalDateTime now;
    private boolean csvSafe;
    private boolean tagged;
    private String pdfa;
    private String password;
    private String ownerPassword;
    private String permissions;
    private final java.util.List<Path> attachments = new java.util.ArrayList<>();
    private boolean xlsxNumbers;

    Report(Ferroreport engine, Path template, String source, String name) {
        this.engine = engine;
        this.template = template;
        this.source = source;
        this.name = name;
    }

    /**
     * The rows of the report: Maps from field name to value, or anything else the template's fields
     * can be read from as JSON (nested Maps and Lists feed subreports and tables). They are read
     * one at a time while the report renders, so a lazy Iterable (a cursor, a stream's iterator)
     * is never held whole in memory.
     */
    public Report rows(Iterable<?> rows) {
        this.rows = Objects.requireNonNull(rows, "rows");
        return this;
    }

    /**
     * The rows of a query, read from the {@code ResultSet} as the report needs them: column labels
     * are the field names. The ResultSet stays yours to close. For a result larger than memory, ask
     * the driver to stream it (a fetch size on PostgreSQL, {@code Integer.MIN_VALUE} on MySQL).
     */
    public Report rows(ResultSet results) {
        Objects.requireNonNull(results, "results");
        this.rows = (Iterable<Map<String, Object>>) () -> new ResultSetRows(results);
        return this;
    }

    /** Sets a report parameter ({@code $P{name}} in the template). */
    public Report param(String name, Object value) {
        params.put(Objects.requireNonNull(name, "name"), value);
        return this;
    }

    /** Sets several report parameters. */
    public Report params(Map<String, ?> values) {
        params.putAll(values);
        return this;
    }

    /** Number and date formatting: {@code en_US} (the default), {@code es_DO}, {@code es_MX}, {@code es_ES}, {@code fr_FR}, {@code fr_CA}, {@code pt_BR}, {@code pt_PT}, {@code it_IT}, {@code it_CH}, {@code de_DE}, {@code de_AT}, {@code de_CH}... */
    public Report locale(String tag) {
        this.locale = Objects.requireNonNull(tag, "tag");
        return this;
    }

    /** The output format. Without it, {@code renderTo(Path)} goes by the file's extension and everything else gives PDF. */
    public Report format(Format format) {
        this.format = Objects.requireNonNull(format, "format");
        return this;
    }

    /** An extra directory for images and subreports (local engine). The template's own directory is always searched. */
    public Report resources(Path directory) {
        resources.add(Objects.requireNonNull(directory, "directory"));
        return this;
    }

    /** Stops with a {@link FerroreportException} if the report goes over this many pages (local engine). */
    public Report maxPages(int pages) {
        this.maxPages = pages;
        return this;
    }

    /**
     * Stops with a {@link FerroreportException} if the report reads more than this many rows, those of
     * its subreports, tables, lists, charts and crosstabs included (local engine). A report that reads
     * many rows but prints few pages, such as a summary of totals, stops here and not at {@link #maxPages}.
     */
    public Report maxRows(int rows) {
        this.maxRows = rows;
        return this;
    }

    /** Stops with a {@link FerroreportException} if rendering takes longer than this (local engine). */
    public Report timeout(Duration longest) {
        this.timeout = Objects.requireNonNull(longest, "longest");
        return this;
    }

    /** Fixes the value of {@code new Date()} in the template's expressions, for reproducible output. */
    public Report now(LocalDateTime now) {
        this.now = Objects.requireNonNull(now, "now");
        return this;
    }

    /**
     * Writes PDF/A, for archiving: {@code "2b"}, {@code "2u"}, {@code "2a"}, {@code "3b"}, {@code "3u"} or
     * {@code "3a"} (level a is also tagged). Every font must be inside the document.
     */
    public Report pdfa(String conformance) {
        this.pdfa = conformance;
        return this;
    }

    /**
     * Encrypts the PDF (AES-256): {@code password} opens it ({@code ""} opens it without asking),
     * {@code ownerPassword} lifts the permissions. The passwords reach the engine through its
     * environment, not its command line.
     */
    public Report password(String password, String ownerPassword) {
        this.password = password == null ? "" : password;
        this.ownerPassword = ownerPassword;
        return this;
    }

    /**
     * What readers allow in an encrypted PDF without the owner password: {@code "print"}, {@code "copy"},
     * {@code "modify"}, {@code "annotate"}, {@code "fill"}, {@code "accessibility"}, {@code "assemble"}
     * or {@code "all"}. Encrypts the PDF.
     */
    public Report permissions(String... permissions) {
        this.permissions = String.join(",", permissions);
        return this;
    }

    /** Carries a file inside the PDF (PDF/A-3 allows it), such as the XML of an electronic invoice. Local engine only. */
    public Report attach(Path file) {
        this.attachments.add(file);
        return this;
    }

    /** In PDF, says what the content is (tagged PDF), for screen readers: texts as paragraphs, images as figures. */
    public Report tagged(boolean on) {
        this.tagged = on;
        return this;
    }

    /** In CSV, puts an apostrophe before values a spreadsheet would run as a formula. Use it when the data comes from other people. */
    public Report csvSafe(boolean on) {
        this.csvSafe = on;
        return this;
    }

    /** In Excel and ODS, writes numbers as numbers instead of as the formatted text. */
    public Report xlsxNumbers(boolean on) {
        this.xlsxNumbers = on;
        return this;
    }

    /** Renders the report and returns the document's bytes. */
    public byte[] render() {
        ByteArrayOutputStream out = new ByteArrayOutputStream(1 << 16);
        run(out, format != null ? format : Format.PDF);
        return out.toByteArray();
    }

    /**
     * Renders the report to a file, page by page. The format follows the extension
     * ({@code .pdf}, {@code .xlsx}, {@code .docx}, {@code .pptx}, {@code .ods}, {@code .odt}, {@code .csv}, {@code .html}) unless {@link #format} was set. If
     * rendering fails, no file is left behind.
     */
    public void renderTo(Path file) {
        Format f = format != null ? format : Format.of(file.getFileName().toString());
        Path dir = file.toAbsolutePath().getParent();
        Path part = null;
        try {
            part = Files.createTempFile(dir, file.getFileName().toString(), ".part");
            try (OutputStream out = Files.newOutputStream(part)) {
                run(out, f);
            }
            Files.move(part, file, StandardCopyOption.REPLACE_EXISTING);
            part = null;
        } catch (IOException e) {
            throw new FerroreportException("cannot write " + file + ": " + e.getMessage(), e);
        } finally {
            if (part != null) {
                try {
                    Files.deleteIfExists(part);
                } catch (IOException ignored) {
                    // The report's own error is the one to show.
                }
            }
        }
    }

    /**
     * Renders the report to a stream as it is made (an HTTP response, for example); the stream is
     * not closed. If rendering fails part of the way, what was already written stays written: use
     * {@link #render()} when the document must arrive whole or not at all.
     */
    public void renderTo(OutputStream out) {
        run(Objects.requireNonNull(out, "out"), format != null ? format : Format.PDF);
    }

    private void run(OutputStream out, Format f) {
        if (engine.server != null) {
            remote(out, f);
        } else {
            local(out, f);
        }
    }

    // ---------------------------------------------------------------------------------------
    // The engine as a child process: rows go in through its standard input as they are read,
    // the document comes out through its standard output as it is made.

    private void local(OutputStream out, Format f) {
        Path temp = null;
        Path paramsFile = null;
        try {
            Path jrxml = template;
            if (jrxml == null) {
                temp = Files.createTempFile("ferroreport", ".jrxml");
                Files.write(temp, source.getBytes(StandardCharsets.UTF_8));
                jrxml = temp;
            }
            List<String> cmd = new ArrayList<>(List.of(engine.binary().toString(), "render", jrxml.toString(), "-o", "-", "-f", f.id(), "-q", "-l", locale));
            if (rows != null) {
                cmd.add("-d");
                cmd.add("-");
            }
            if (!params.isEmpty()) {
                paramsFile = Files.createTempFile("ferroreport", ".json");
                Files.write(paramsFile, Json.text(params).getBytes(StandardCharsets.UTF_8));
                cmd.add("-p");
                cmd.add(paramsFile.toString());
            }
            for (Path dir : resources) {
                cmd.add("-r");
                cmd.add(dir.toString());
            }
            if (maxPages != null) {
                cmd.add("--max-pages");
                cmd.add(maxPages.toString());
            }
            if (maxRows != null) {
                cmd.add("--max-rows");
                cmd.add(maxRows.toString());
            }
            if (timeout != null) {
                cmd.add("--timeout");
                cmd.add(String.valueOf(timeout.toMillis() / 1000.0));
            }
            if (now != null) {
                cmd.add("--now");
                cmd.add(now.withNano(0).toString().length() == 16 ? now.withNano(0) + ":00" : now.withNano(0).toString());
            }
            if (tagged) {
                cmd.add("--tagged");
            }
            if (pdfa != null) {
                cmd.add("--pdfa");
                cmd.add(pdfa);
            }
            if (permissions != null) {
                cmd.add("--permissions");
                cmd.add(permissions);
            }
            for (Path file : attachments) {
                cmd.add("--attach");
                cmd.add(file.toString());
            }
            if (csvSafe) {
                cmd.add("--csv-safe");
            }
            if (xlsxNumbers) {
                cmd.add("--xlsx-numbers");
            }
            ProcessBuilder builder = new ProcessBuilder(cmd);
            builder.environment().remove("FERROREPORT_PDF_USER_PASSWORD");
            builder.environment().remove("FERROREPORT_PDF_OWNER_PASSWORD");
            if (password != null) {
                builder.environment().put("FERROREPORT_PDF_USER_PASSWORD", password);
            }
            if (ownerPassword != null) {
                builder.environment().put("FERROREPORT_PDF_OWNER_PASSWORD", ownerPassword);
            }
            Process process = builder.start();
            AtomicReference<Throwable> rowsFailure = new AtomicReference<>();
            Thread feeder = new Thread(() -> feed(process, rowsFailure), "ferroreport-rows");
            ByteArrayOutputStream errors = new ByteArrayOutputStream();
            Thread listener = new Thread(() -> drain(process.getErrorStream(), errors), "ferroreport-errors");
            feeder.setDaemon(true);
            listener.setDaemon(true);
            feeder.start();
            listener.start();
            try (InputStream document = process.getInputStream()) {
                document.transferTo(out);
            } catch (IOException e) {
                process.destroyForcibly();
                feeder.join(2000);
                // When the rows failed, the engine was stopped on purpose: that is the error to report.
                if (rowsFailure.get() == null) {
                    throw new FerroreportException("cannot write the document: " + e.getMessage(), e);
                }
            }
            int exit = process.waitFor();
            feeder.join();
            listener.join();
            Throwable failed = rowsFailure.get();
            if (failed != null) {
                throw new FerroreportException("cannot read the rows: " + failed.getMessage(), failed);
            }
            if (exit != 0) {
                String message = new String(errors.toByteArray(), StandardCharsets.UTF_8).trim();
                // The engine's last word is a line that starts with "error: " (warnings may come before it).
                int at = message.startsWith("error: ") ? 0 : message.lastIndexOf("\nerror: ") + 1;
                boolean found = message.startsWith("error: ", at);
                throw new FerroreportException(found ? message.substring(at + 7) : message.isEmpty() ? "the engine stopped with status " + exit : message);
            }
        } catch (IOException e) {
            throw new FerroreportException("cannot run the ferroreport engine: " + e.getMessage(), e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new FerroreportException("interrupted while rendering", e);
        } finally {
            delete(temp);
            delete(paramsFile);
        }
    }

    /** Writes the rows as JSON, one per line, as the engine asks for them. */
    private void feed(Process process, AtomicReference<Throwable> failure) {
        try (Writer w = new BufferedWriter(new OutputStreamWriter(process.getOutputStream(), StandardCharsets.UTF_8), 1 << 16)) {
            if (rows != null) {
                for (Object row : rows) {
                    Json.write(row, w);
                    w.write('\n');
                }
            }
        } catch (IOException e) {
            // The engine closed its input: it stopped (an error it reports itself) or needed no more rows.
        } catch (RuntimeException e) {
            failure.set(e.getCause() instanceof SQLException ? e.getCause() : e);
            process.destroyForcibly();
        }
    }

    private static void drain(InputStream in, OutputStream out) {
        try (in) {
            in.transferTo(out);
        } catch (IOException ignored) {
            // Nothing more to read.
        }
    }

    private static void delete(Path file) {
        if (file != null) {
            try {
                Files.deleteIfExists(file);
            } catch (IOException ignored) {
                // A leftover temporary file is not worth failing a report for.
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // The engine as a server.

    private void remote(OutputStream out, Format f) {
        StringBuilder head = new StringBuilder(1 << 12);
        AtomicReference<Throwable> rowsFailure = new AtomicReference<>();
        try {
            // The request on the first line, then one row per line: the rows leave as they
            // are read (from a ResultSet, say) and the server reads them as the report needs
            // them, so neither side ever holds them all.
            head.append('{');
            if (name == null) {
                head.append("\"template\":");
                Json.string(source != null ? source : new String(Files.readAllBytes(template), StandardCharsets.UTF_8), head);
                head.append(',');
            }
            head.append("\"format\":\"").append(f.id()).append("\",\"locale\":");
            Json.string(locale, head);
            if (now != null) {
                head.append(",\"now\":");
                Json.string(now.toString(), head);
            }
            if (tagged) {
                head.append(",\"tagged\":true");
            }
            if (pdfa != null) {
                head.append(",\"pdfa\":");
                Json.string(pdfa, head);
            }
            if (password != null) {
                head.append(",\"password\":");
                Json.string(password, head);
            }
            if (ownerPassword != null) {
                head.append(",\"ownerPassword\":");
                Json.string(ownerPassword, head);
            }
            if (permissions != null) {
                head.append(",\"permissions\":");
                Json.string(permissions, head);
            }
            if (csvSafe) {
                head.append(",\"csvSafe\":true");
            }
            if (xlsxNumbers) {
                head.append(",\"xlsxNumbers\":true");
            }
            head.append(",\"params\":");
            Json.write(params, head);
            head.append("}\n");

            String base = engine.server.toString().replaceAll("/+$", "");
            URI uri = URI.create(base + (name == null ? "/render" : "/render/" + name.replaceAll("^/+", "")));
            byte[] first = head.toString().getBytes(StandardCharsets.UTF_8);
            HttpRequest.Builder request = HttpRequest.newBuilder(uri)
                .header("Content-Type", "application/x-ndjson")
                .POST(HttpRequest.BodyPublishers.ofInputStream(() -> new Lines(first, rows, rowsFailure)));
            if (engine.token != null) {
                request.header("Authorization", "Bearer " + engine.token);
            }
            HttpResponse<InputStream> response = HTTP.send(request.build(), HttpResponse.BodyHandlers.ofInputStream());
            try (InputStream document = response.body()) {
                if (response.statusCode() != 200) {
                    String text = new String(document.readAllBytes(), StandardCharsets.UTF_8);
                    String message = errorOf(text, response.statusCode());
                    if (response.statusCode() == 400 && message.startsWith("invalid JSON") && rowsFailure.get() == null) {
                        message += " (is the server older than ferroreport 0.9.0? This library sends rows one per line, which older servers do not read)";
                    }
                    throw new FerroreportException(message);
                }
                document.transferTo(out);
            }
        } catch (IOException e) {
            Throwable failed = rowsFailure.get();
            if (failed != null) {
                throw new FerroreportException("cannot read the rows: " + failed.getMessage(), failed);
            }
            throw new FerroreportException("cannot reach the ferroreport server at " + engine.server + ": " + e.getMessage(), e);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new FerroreportException("interrupted while rendering", e);
        }
        Throwable failed = rowsFailure.get();
        if (failed != null) {
            throw new FerroreportException("cannot read the rows: " + failed.getMessage(), failed);
        }
    }

    /** The body of a request, produced as it is sent: the first line, then each row on its own. */
    private static final class Lines extends InputStream {
        private final Iterator<?> rows;
        private final AtomicReference<Throwable> failure;
        private final StringBuilder text = new StringBuilder(1 << 16);
        private byte[] piece;
        private int at;

        Lines(byte[] first, Iterable<?> rows, AtomicReference<Throwable> failure) {
            this.piece = first;
            this.rows = rows == null ? null : rows.iterator();
            this.failure = failure;
        }

        /** Makes the next piece (about 64 KB of rows); false when there are no more. */
        private boolean more() throws IOException {
            if (rows == null) {
                return false;
            }
            text.setLength(0);
            try {
                while (text.length() < (1 << 16) && rows.hasNext()) {
                    Json.write(rows.next(), text);
                    text.append('\n');
                }
            } catch (RuntimeException e) {
                Throwable cause = e.getCause() instanceof SQLException ? e.getCause() : e;
                failure.compareAndSet(null, cause);
                // Stops the request: a report from half of the rows would be a wrong report.
                throw new IOException("cannot read the rows", cause);
            }
            piece = text.toString().getBytes(StandardCharsets.UTF_8);
            at = 0;
            return piece.length > 0;
        }

        @Override
        public int read(byte[] out, int off, int len) throws IOException {
            if (len == 0) {
                return 0;
            }
            if (at == piece.length && !more()) {
                return -1;
            }
            int n = Math.min(len, piece.length - at);
            System.arraycopy(piece, at, out, off, n);
            at += n;
            return n;
        }

        @Override
        public int read() throws IOException {
            byte[] one = new byte[1];
            return read(one, 0, 1) < 0 ? -1 : one[0] & 0xff;
        }
    }

    private static final HttpClient HTTP =
        HttpClient.newBuilder().version(HttpClient.Version.HTTP_1_1).connectTimeout(Duration.ofSeconds(10)).build();

    /** The message in a server's {@code {"error": "..."}} answer. */
    static String errorOf(String body, int status) {
        int key = body.indexOf("\"error\"");
        int start = key < 0 ? -1 : body.indexOf('"', body.indexOf(':', key) + 1);
        if (start < 0) {
            return "the server answered " + status + (body.isBlank() ? "" : ": " + body.trim());
        }
        StringBuilder message = new StringBuilder();
        for (int i = start + 1; i < body.length(); i++) {
            char c = body.charAt(i);
            if (c == '"') {
                break;
            }
            if (c == '\\' && i + 1 < body.length()) {
                char next = body.charAt(++i);
                if (next == 'n') {
                    message.append('\n');
                } else if (next == 't') {
                    message.append('\t');
                } else if (next == 'u' && i + 4 < body.length()) {
                    message.append((char) Integer.parseInt(body.substring(i + 1, i + 5), 16));
                    i += 4;
                } else {
                    message.append(next);
                }
            } else {
                message.append(c);
            }
        }
        return message.toString();
    }

    /** The rows of a ResultSet as Maps, read one at a time. */
    private static final class ResultSetRows implements Iterator<Map<String, Object>> {
        private final ResultSet results;
        private final String[] labels;
        private Boolean ready;

        ResultSetRows(ResultSet results) {
            this.results = results;
            try {
                ResultSetMetaData meta = results.getMetaData();
                labels = new String[meta.getColumnCount()];
                for (int i = 0; i < labels.length; i++) {
                    labels[i] = meta.getColumnLabel(i + 1);
                }
            } catch (SQLException e) {
                throw new IllegalStateException(e);
            }
        }

        @Override
        public boolean hasNext() {
            if (ready == null) {
                try {
                    ready = results.next();
                } catch (SQLException e) {
                    throw new IllegalStateException(e);
                }
            }
            return ready;
        }

        @Override
        public Map<String, Object> next() {
            if (!hasNext()) {
                throw new NoSuchElementException();
            }
            ready = null;
            Map<String, Object> row = new LinkedHashMap<>(labels.length * 2);
            try {
                for (int i = 0; i < labels.length; i++) {
                    Object value = results.getObject(i + 1);
                    if (value instanceof java.sql.Clob) {
                        java.sql.Clob clob = (java.sql.Clob) value;
                        value = clob.getSubString(1, (int) Math.min(clob.length(), Integer.MAX_VALUE));
                    } else if (value instanceof java.sql.Blob) {
                        java.sql.Blob blob = (java.sql.Blob) value;
                        value = blob.getBytes(1, (int) Math.min(blob.length(), Integer.MAX_VALUE));
                    }
                    row.put(labels[i], value);
                }
            } catch (SQLException e) {
                throw new IllegalStateException(e);
            }
            return row;
        }
    }
}
