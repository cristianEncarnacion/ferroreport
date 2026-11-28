package io.github.cristianencarnacion.ferroreport;

import java.net.URI;
import java.nio.file.Path;
import java.util.Objects;

/**
 * A report engine for Java: a template and your rows in; a PDF, an Excel workbook, a CSV or a
 * web page out. Templates are JasperReports {@code .jrxml} files, rendered without JasperReports
 * on the classpath.
 *
 * <pre>{@code
 * Ferroreport reports = Ferroreport.local();
 *
 * byte[] pdf = reports.template(Path.of("invoice.jrxml"))
 *     .rows(lines)                              // a List of Maps, or any Iterable
 *     .param("CUSTOMER", "Distribuidora Caribe")
 *     .locale("es_DO")
 *     .render();
 *
 * // A million rows straight from the database, in constant memory:
 * try (ResultSet sales = statement.executeQuery("select * from sales order by region")) {
 *     reports.template(Path.of("sales.jrxml")).rows(sales).renderTo(Path.of("sales.pdf"));
 * }
 * }</pre>
 *
 * <p>{@link #local()} runs the native engine as a child process and feeds it the rows as they are
 * read, so neither the rows nor the document are ever held whole in the JVM. {@link #server} talks
 * to a {@code ferroreport serve} instance over HTTP instead (a sidecar container, for example).
 * Instances are immutable and safe to share between threads.
 */
public final class Ferroreport {
    /** The version of this library, and of the engine it downloads when none is installed. */
    public static final String VERSION = "1.4.0";

    final Path binary;
    final URI server;
    final String token;

    private Ferroreport(Path binary, URI server, String token) {
        this.binary = binary;
        this.server = server;
        this.token = token;
    }

    /**
     * The engine on this machine: the program named by {@code -Dferroreport.bin} or
     * {@code FERROREPORT_BIN}, else {@code ferroreport} on the {@code PATH}, else a copy downloaded
     * once from the project's releases (checked against their SHA-256 list) into the user's cache
     * directory. {@code -Dferroreport.download=false} forbids the download.
     */
    public static Ferroreport local() {
        return new Ferroreport(null, null, null);
    }

    /** The engine at {@code binary}. */
    public static Ferroreport local(Path binary) {
        return new Ferroreport(Objects.requireNonNull(binary, "binary"), null, null);
    }

    /**
     * A {@code ferroreport serve} instance, such as {@code http://reports:8080}. {@code token} is
     * its bearer token ({@code null} when it runs without one). Page and time limits are the
     * server's own.
     */
    public static Ferroreport server(URI server, String token) {
        return new Ferroreport(null, Objects.requireNonNull(server, "server"), token);
    }

    /** A report from a template file. Images and subreports are looked up next to it. */
    public Report template(Path jrxml) {
        return new Report(this, Objects.requireNonNull(jrxml, "jrxml"), null, null);
    }

    /** A report from the JRXML text itself (a template kept in a database, for example). */
    public Report source(String jrxml) {
        return new Report(this, null, Objects.requireNonNull(jrxml, "jrxml"), null);
    }

    /**
     * A report by the name a server knows it by ({@code ferroreport serve --templates}):
     * {@code "sales/invoice"} is {@code sales/invoice.jrxml} in its templates directory.
     */
    public Report named(String name) {
        if (server == null) {
            throw new IllegalStateException("named templates live on a server: use Ferroreport.server(...), or template(Path)");
        }
        return new Report(this, null, null, Objects.requireNonNull(name, "name"));
    }

    Path binary() {
        return binary != null ? binary : Binary.locate();
    }
}
