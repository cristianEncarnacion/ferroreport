package io.github.cristianencarnacion.ferroreport;

import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertThrows;
import static org.junit.Assert.assertTrue;
import static org.junit.Assume.assumeTrue;

import java.io.ByteArrayOutputStream;
import java.lang.reflect.Proxy;
import java.math.BigDecimal;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.sql.ResultSet;
import java.sql.ResultSetMetaData;
import java.time.Duration;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.zip.GZIPOutputStream;
import org.junit.Test;

public class FerroreportTest {
    private static final Path FIXTURES = Paths.get("..", "..", "fixtures").toAbsolutePath().normalize();

    /** The engine under test: set FERROREPORT_BIN to a build of this repository. */
    private static Ferroreport engine() {
        String bin = System.getenv("FERROREPORT_BIN");
        assumeTrue("set FERROREPORT_BIN to run the rendering tests", bin != null && !bin.isEmpty());
        return Ferroreport.local(Paths.get(bin));
    }

    private static List<Map<String, Object>> invoiceLines() {
        List<Map<String, Object>> rows = new ArrayList<>();
        for (int i = 0; i < 46; i++) {
            Map<String, Object> row = new LinkedHashMap<>();
            row.put("sku", "FX-" + (1000 + 7 * i));
            row.put("description", i % 5 == 4 ? "Electric motor 5HP three-phase with mounting bracket \"heavy\" & warranty" : "Stainless steel valve 3/4\"");
            row.put("qty", 1 + i % 9);
            row.put("price", new BigDecimal("185.00"));
            row.put("discount", i % 3 == 0 ? new BigDecimal("10.00") : null);
            rows.add(row);
        }
        return rows;
    }

    /** `count` rows made as they are asked for; the one at `failAt` throws. */
    private static Iterable<Map<String, Object>> people(int count, int failAt) {
        return () -> new java.util.Iterator<Map<String, Object>>() {
            private int i;

            @Override
            public boolean hasNext() {
                return i < count;
            }

            @Override
            public Map<String, Object> next() {
                if (i == failAt) {
                    throw new IllegalStateException("row " + i + " failed");
                }
                Map<String, Object> row = new LinkedHashMap<>();
                row.put("id", ++i);
                row.put("name", "Person " + i);
                row.put("department", "Engineering");
                row.put("salary", 1000.5 + i);
                return row;
            }
        };
    }

    private static Report invoice(Ferroreport engine) {
        return filled(engine.template(FIXTURES.resolve("02-invoice/report.jrxml")));
    }

    private static Report filled(Report report) {
        return report
            .rows(invoiceLines())
            .param("COMPANY", "Ferro Industrial SRL")
            .param("INVOICE_NO", "F-2026-0042")
            .param("CUSTOMER", "Distribuidora Caribe, S.A.")
            .param("ISSUED", LocalDate.of(2026, 3, 5))
            .param("TAX_RATE", new BigDecimal("0.18"))
            .now(LocalDateTime.of(2026, 3, 5, 10, 0));
    }

    @Test
    public void javaValuesBecomeTheJsonTheEngineReads() {
        Map<String, Object> row = new LinkedHashMap<>();
        row.put("name", "Añasco \"x\"\n\\");
        row.put("price", new BigDecimal("185.00"));
        row.put("qty", 3);
        row.put("ratio", 0.5);
        row.put("bad", Double.NaN);
        row.put("when", LocalDate.of(2026, 3, 5));
        row.put("at", java.sql.Timestamp.valueOf("2026-03-05 10:30:00"));
        row.put("logo", new byte[] {1, 2, 3});
        row.put("lines", List.of(Map.of("n", 1)));
        row.put("none", null);
        row.put("ctrl", "\u0001");
        assertEquals(
            "{\"name\":\"Añasco \\\"x\\\"\\n\\\\\",\"price\":\"185.00\",\"qty\":3,\"ratio\":0.5,\"bad\":null,"
                + "\"when\":\"2026-03-05\",\"at\":\"2026-03-05T10:30\",\"logo\":\"AQID\",\"lines\":[{\"n\":1}],\"none\":null,\"ctrl\":\"\\u0001\"}",
            Json.text(row));
    }

    @Test
    public void theRightReleaseFileIsChosenAndChecked() throws Exception {
        assertEquals("ferroreport-linux-x86_64.tar.gz", Binary.asset("Linux", "amd64"));
        assertEquals("ferroreport-linux-aarch64.tar.gz", Binary.asset("Linux", "aarch64"));
        assertEquals("ferroreport-macos-aarch64.tar.gz", Binary.asset("Mac OS X", "aarch64"));
        assertEquals("ferroreport-macos-x86_64.tar.gz", Binary.asset("Mac OS X", "x86_64"));
        assertEquals("ferroreport-windows-x86_64.zip", Binary.asset("Windows 11", "amd64"));
        assertThrows(FerroreportException.class, () -> Binary.asset("Linux", "riscv64"));

        String sums = "aa11  ferroreport-linux-x86_64.tar.gz\nbb22 *ferroreport-windows-x86_64.zip\n";
        assertTrue(Binary.listed(sums, "ferroreport-linux-x86_64.tar.gz", "AA11"));
        assertTrue(Binary.listed(sums, "ferroreport-windows-x86_64.zip", "bb22"));
        assertFalse(Binary.listed(sums, "ferroreport-linux-x86_64.tar.gz", "bb22"));
        assertFalse(Binary.listed(sums, "other.tar.gz", "aa11"));

        // A tar with a readme and the program in a folder.
        ByteArrayOutputStream tar = new ByteArrayOutputStream();
        byte[] program = "#!engine".repeat(100).getBytes(StandardCharsets.UTF_8);
        for (Object[] entry : new Object[][] {{"ferroreport-linux/README.md", "hello".getBytes(StandardCharsets.UTF_8)}, {"ferroreport-linux/ferroreport", program}}) {
            byte[] header = new byte[512];
            byte[] name = ((String) entry[0]).getBytes(StandardCharsets.UTF_8);
            byte[] data = (byte[]) entry[1];
            System.arraycopy(name, 0, header, 0, name.length);
            byte[] size = String.format("%011o", data.length).getBytes(StandardCharsets.UTF_8);
            System.arraycopy(size, 0, header, 124, size.length);
            header[156] = '0';
            tar.write(header);
            tar.write(data);
            tar.write(new byte[(512 - data.length % 512) % 512]);
        }
        tar.write(new byte[1024]);
        ByteArrayOutputStream gz = new ByteArrayOutputStream();
        try (GZIPOutputStream out = new GZIPOutputStream(gz)) {
            out.write(tar.toByteArray());
        }
        assertArrayEquals(program, Binary.fromTarGz(gz.toByteArray()));
        assertEquals("0a1bff", Binary.hex(new byte[] {10, 27, -1}));
    }

    @Test
    public void formatsFollowFileNames() {
        assertEquals(Format.XLSX, Format.of("Sales.XLSX"));
        assertEquals(Format.HTML, Format.of("preview.htm"));
        assertEquals(Format.PDF, Format.of("invoice"));
        assertEquals(Format.ODS, Format.of("sales.ods"));
        assertEquals(Format.ODT, Format.of("letter.odt"));
        assertEquals(Format.PPTX, Format.of("Board.PPTX"));
        assertEquals("text/csv; charset=utf-8", Format.CSV.mediaType());
    }

    @Test
    public void rendersEveryFormat() throws Exception {
        Ferroreport engine = engine();
        byte[] pdf = invoice(engine).render();
        assertEquals("%PDF-", new String(pdf, 0, 5, StandardCharsets.US_ASCII));
        assertEquals("PK", new String(invoice(engine).format(Format.XLSX).render(), 0, 2, StandardCharsets.US_ASCII));
        assertEquals("PK", new String(invoice(engine).format(Format.PPTX).render(), 0, 2, StandardCharsets.US_ASCII));
        String csv = new String(invoice(engine).format(Format.CSV).render(), StandardCharsets.UTF_8);
        assertTrue(csv, csv.contains("FX-1000") && csv.contains("Distribuidora Caribe"));
        String html = new String(invoice(engine).format(Format.HTML).render(), StandardCharsets.UTF_8);
        assertTrue(html.startsWith("<!DOCTYPE html>") && html.contains("Ferro Industrial SRL"));

        Path dir = Files.createTempDirectory("ferroreport-test");
        Path file = dir.resolve("invoice.xlsx");
        invoice(engine).renderTo(file);
        assertEquals("PK", new String(Files.readAllBytes(file), 0, 2, StandardCharsets.US_ASCII));
        ByteArrayOutputStream stream = new ByteArrayOutputStream();
        invoice(engine).renderTo(stream);
        assertArrayEquals(pdf, stream.toByteArray());

        // The template as text.
        String source = new String(Files.readAllBytes(FIXTURES.resolve("01-employee-list/report.jrxml")), StandardCharsets.UTF_8);
        byte[] list = engine.source(source).rows(List.of(Map.of("id", 1, "name", "Ana", "department", "Sales", "salary", 10.5))).render();
        assertEquals("%PDF-", new String(list, 0, 5, StandardCharsets.US_ASCII));
    }

    @Test
    public void rowsAreReadAsTheReportNeedsThemAndLimitsStopIt() throws Exception {
        Ferroreport engine = engine();
        Path template = FIXTURES.resolve("09-large/report.jrxml");
        AtomicInteger made = new AtomicInteger();
        Iterable<Map<String, Object>> endless = () -> new Iterator<>() {
            @Override
            public boolean hasNext() {
                return true;
            }

            @Override
            public Map<String, Object> next() {
                int i = made.incrementAndGet();
                return Map.of("id", i, "name", "Employee " + i, "department", "Sales", "salary", 1000.5 + i);
            }
        };
        // Rows without end: only a stream can stop after the pages it was allowed.
        FerroreportException tooLong = assertThrows(FerroreportException.class, () -> engine.template(template).rows(endless).maxPages(20).render());
        assertTrue(tooLong.getMessage(), tooLong.getMessage().contains("more than 20 pages"));
        assertTrue("rows made: " + made.get(), made.get() < 200_000);

        Path dir = Files.createTempDirectory("ferroreport-test");
        assertThrows(FerroreportException.class, () -> engine.template(template).rows(endless).maxPages(3).renderTo(dir.resolve("never.pdf")));
        try (java.util.stream.Stream<Path> left = Files.list(dir)) {
            assertEquals(0, left.count());
        }
        assertThrows(FerroreportException.class, () -> engine.template(template).rows(endless).timeout(Duration.ofMillis(1)).render());

        // A report that only prints a total: one page however many rows, stopped by the row limit.
        String total = "<jasperReport name=\"total\" pageWidth=\"200\" pageHeight=\"100\" columnWidth=\"200\" leftMargin=\"0\" rightMargin=\"0\" topMargin=\"0\" bottomMargin=\"0\">"
            + "<field name=\"id\" class=\"java.lang.Integer\"/><summary><band height=\"20\"><textField><reportElement x=\"0\" y=\"0\" width=\"200\" height=\"20\"/>"
            + "<textFieldExpression><![CDATA[\"rows \" + $V{REPORT_COUNT}]]></textFieldExpression></textField></band></summary></jasperReport>";
        made.set(0);
        FerroreportException tooMany = assertThrows(FerroreportException.class, () -> engine.source(total).rows(endless).maxRows(1000).maxPages(1).render());
        assertTrue(tooMany.getMessage(), tooMany.getMessage().contains("reads more than 1000 rows"));
        assertTrue("rows made: " + made.get(), made.get() < 200_000);
        Iterable<Map<String, Object>> thousand = () -> java.util.stream.IntStream.range(0, 1000).mapToObj(i -> Map.<String, Object>of("id", i)).iterator();
        String counted = new String(engine.source(total).rows(thousand).maxRows(1000).format(Format.CSV).render(), StandardCharsets.UTF_8);
        assertEquals("rows 1000", counted.trim());

        // 60,000 rows from a lazy source, as CSV: one line per row plus headers.
        Iterable<Map<String, Object>> many = () -> new Iterator<>() {
            int i = 0;

            @Override
            public boolean hasNext() {
                return i < 60_000;
            }

            @Override
            public Map<String, Object> next() {
                i++;
                return Map.of("id", i, "name", "Employee " + i, "department", "Sales", "salary", 1000.5 + i);
            }
        };
        String csv = new String(engine.template(template).rows(many).format(Format.CSV).render(), StandardCharsets.UTF_8);
        assertTrue(csv.contains("Employee 60000") && csv.split("\n").length > 60_000);

        // A failure while producing rows is reported as such.
        Iterable<Object> broken = () -> new Iterator<>() {
            @Override
            public boolean hasNext() {
                return true;
            }

            @Override
            public Object next() {
                throw new IllegalStateException("cursor closed");
            }
        };
        FerroreportException rows = assertThrows(FerroreportException.class, () -> engine.template(template).rows(broken).render());
        assertTrue(rows.getMessage(), rows.getMessage().contains("cursor closed"));
        FerroreportException bad = assertThrows(FerroreportException.class, () -> engine.source("<jasperReport").render());
        assertTrue(bad.getMessage(), bad.getMessage().contains("XML"));
    }

    @Test
    public void readsAResultSet() throws Exception {
        String[] labels = {"id", "name", "department", "salary"};
        Object[][] data = {{1, "Ana Rodríguez", "Engineering", 70132.18}, {2, "María Gómez", "Finance", 100743.02}};
        int[] at = {-1};
        ResultSetMetaData meta = (ResultSetMetaData) Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[] {ResultSetMetaData.class}, (p, m, a) -> {
            switch (m.getName()) {
                case "getColumnCount":
                    return labels.length;
                case "getColumnLabel":
                    return labels[(Integer) a[0] - 1];
                default:
                    throw new UnsupportedOperationException(m.getName());
            }
        });
        ResultSet results = (ResultSet) Proxy.newProxyInstance(getClass().getClassLoader(), new Class<?>[] {ResultSet.class}, (p, m, a) -> {
            switch (m.getName()) {
                case "getMetaData":
                    return meta;
                case "next":
                    return ++at[0] < data.length;
                case "getObject":
                    return data[at[0]][(Integer) a[0] - 1];
                default:
                    throw new UnsupportedOperationException(m.getName());
            }
        });
        String csv = new String(engine().template(FIXTURES.resolve("09-large/report.jrxml")).rows(results).format(Format.CSV).render(), StandardCharsets.UTF_8);
        assertTrue(csv, csv.contains("Ana Rodríguez") && csv.contains("María Gómez") && csv.contains("100,743.02"));
    }

    @Test
    public void talksToAServer() throws Exception {
        String bin = System.getenv("FERROREPORT_BIN");
        assumeTrue("set FERROREPORT_BIN to run the server test", bin != null && !bin.isEmpty());
        int port;
        try (java.net.ServerSocket probe = new java.net.ServerSocket(0)) {
            port = probe.getLocalPort();
        }
        Process server = new ProcessBuilder(bin, "serve", "--port", String.valueOf(port), "--token", "s3cret", "--templates", FIXTURES.resolve("02-invoice").toString(), "--max-rows", "200000")
            .redirectErrorStream(true).redirectOutput(ProcessBuilder.Redirect.DISCARD).start();
        try {
            Ferroreport remote = Ferroreport.server(URI.create("http://127.0.0.1:" + port + "/"), "s3cret");
            byte[] pdf = null;
            for (int attempt = 0; pdf == null; attempt++) {
                try {
                    pdf = invoice(remote).render();
                } catch (FerroreportException e) {
                    if (attempt > 300 || !e.getMessage().contains("cannot reach")) {
                        throw e;
                    }
                    Thread.sleep(100);
                }
            }
            assertEquals("%PDF-", new String(pdf, 0, 5, StandardCharsets.US_ASCII));
            byte[] named = filled(remote.named("report")).format(Format.HTML).render();
            assertTrue(new String(named, StandardCharsets.UTF_8).contains("<svg class=\"page\""));
            FerroreportException bad = assertThrows(FerroreportException.class, () -> remote.source("<jasperReport").render());
            assertTrue(bad.getMessage(), bad.getMessage().contains("XML"));
            // Java's own HttpClient offers HTTP/2 on its first request; the server must answer it.
            java.net.http.HttpResponse<String> plain = java.net.http.HttpClient.newHttpClient().send(
                java.net.http.HttpRequest.newBuilder(URI.create("http://127.0.0.1:" + port + "/inspect"))
                    .timeout(Duration.ofSeconds(10))
                    .header("Authorization", "Bearer s3cret")
                    .POST(java.net.http.HttpRequest.BodyPublishers.ofString("{\"template\":\"<jasperReport name=\\\"t\\\"/>\"}"))
                    .build(),
                java.net.http.HttpResponse.BodyHandlers.ofString());
            assertEquals(plain.body(), 200, plain.statusCode());
            // Rows that are never in memory together: made one by one here, read one by one there.
            Path large = FIXTURES.resolve("09-large/report.jrxml");
            int[] lines = {0};
            java.io.OutputStream counting = new java.io.OutputStream() {
                @Override
                public void write(int b) {
                    lines[0] += b == '\n' ? 1 : 0;
                }
            };
            remote.template(large).rows(people(200_000, -1)).format(Format.CSV).renderTo(counting);
            assertTrue("lines: " + lines[0], lines[0] > 200_000);
            // One row more than the server's --max-rows: its limit, answered before the document.
            FerroreportException tooMany = assertThrows(FerroreportException.class, () -> remote.template(large).rows(people(200_001, -1)).format(Format.CSV).render());
            assertTrue(tooMany.getMessage(), tooMany.getMessage().contains("reads more than 200000 rows"));
            // Rows that stop half way: the error is the rows', and no document comes back.
            FerroreportException broken = assertThrows(FerroreportException.class, () -> remote.template(large).rows(people(200_000, 50_000)).render());
            assertTrue(broken.getMessage(), broken.getMessage().contains("cannot read the rows: row 50000 failed"));
            // An answer that comes before the rows have all been sent is still the server's answer.
            FerroreportException early = assertThrows(FerroreportException.class, () -> remote.source("<jasperReport").rows(people(200_000, -1)).render());
            assertTrue(early.getMessage(), early.getMessage().contains("XML"));
            FerroreportException denied = assertThrows(FerroreportException.class, () -> invoice(Ferroreport.server(URI.create("http://127.0.0.1:" + port), "wrong")).render());
            assertTrue(denied.getMessage(), denied.getMessage().contains("token"));
        } finally {
            server.destroy();
        }
    }
}
