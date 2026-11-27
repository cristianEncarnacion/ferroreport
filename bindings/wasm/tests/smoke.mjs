// Smoke test of the built npm package (run after build.sh): node tests/smoke.mjs
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import assert from "node:assert/strict";

const require = createRequire(import.meta.url);
const ferroreport = require("../pkg");
const fx = new URL("../../../fixtures/17-subreports/", import.meta.url);

const template = readFileSync(new URL("report.jrxml", fx), "utf8");
const data = JSON.parse(readFileSync(new URL("data.json", fx), "utf8"));
const pdf = ferroreport.render(template, data, {}, {
  resources: { "lines.jrxml": readFileSync(new URL("lines.jrxml", fx)) },
  now: "2026-01-01T00:00:00",
});
assert.ok(pdf instanceof Uint8Array);
assert.equal(Buffer.from(pdf.subarray(0, 5)).toString(), "%PDF-");

// ferroreport's own format: converted, and rendered the same.
const ferro = ferroreport.convert(template, "ferro");
assert.ok(ferro.startsWith("report orders"), ferro.slice(0, 80));
assert.ok(ferroreport.convert(ferro, "jrxml").includes('<element kind="subreport"'));
const fromFerro = ferroreport.render(ferro, data, {}, {
  resources: { "lines.jrxml": readFileSync(new URL("lines.jrxml", fx)) },
  now: "2026-01-01T00:00:00",
  format: "csv",
});
assert.ok(Buffer.from(fromFerro).toString().length > 0);

const info = ferroreport.inspect(template);
assert.equal(info.name, "orders");
assert.deepEqual(info.fields, ["order", "customer", "lines"]);

const xlsx = ferroreport.render(template, data, {}, {
  resources: { "lines.jrxml": readFileSync(new URL("lines.jrxml", fx)) },
  now: "2026-01-01T00:00:00",
  format: "xlsx",
});
assert.equal(Buffer.from(xlsx.subarray(0, 2)).toString(), "PK");
const csv = Buffer.from(ferroreport.render(template, data, {}, {
  resources: { "lines.jrxml": readFileSync(new URL("lines.jrxml", fx)) },
  format: "csv",
})).toString();
assert.ok(csv.startsWith("Orders with line items,"), csv.slice(0, 200));

// Templates as a tree, for editors: read, change, write back as JRXML or .ferro.
const examples = new URL("../../../examples/", import.meta.url);
const invoices = readFileSync(new URL("invoices.ferro", examples), "utf8");
assert.throws(() => ferroreport.toTree(invoices)); // it imports parts.ferro
const tree = ferroreport.toTree(invoices, { resources: { "parts.ferro": readFileSync(new URL("parts.ferro", examples), "utf8") } });
assert.equal(tree.tag, "jasperReport");
assert.deepEqual(tree.attrs[0], ["name", "Invoices"]);
const title = tree.children.find((c) => c.tag === "title");
const h1 = title.children[0];
assert.equal(h1.children.find((c) => c.tag === "text").text, "Invoices");
h1.children.find((c) => c.tag === "text").text = "Facturas";
h1.attrs.push(["bold", "true"]);
const jrxml = ferroreport.fromTree(tree);
assert.ok(jrxml.includes("<![CDATA[Facturas]]>") && jrxml.includes('bold="true"'), jrxml.slice(0, 600));
const ferroText = ferroreport.fromTree(tree, "ferro");
assert.ok(ferroText.startsWith('report Invoices') && ferroText.includes("Facturas"), ferroText.slice(0, 300));
// The .ferro form may list attributes in another order; the report is the same.
const sorted = (n) => ({ ...n, attrs: [...n.attrs].sort(), children: n.children.map(sorted) });
assert.deepEqual(sorted(ferroreport.toTree(ferroText)), sorted(ferroreport.toTree(jrxml)));
const rows = JSON.parse(readFileSync(new URL("invoices.json", examples), "utf8"));
assert.equal(Buffer.from(ferroreport.render(jrxml, rows).subarray(0, 5)).toString(), "%PDF-");
assert.equal(Buffer.from(ferroreport.render(jrxml, rows, {}, { format: "pptx" }).subarray(0, 2)).toString(), "PK");
assert.deepEqual(ferroreport.toTree(template).attrs.find(([k]) => k === "name"), ["name", "orders"]);
assert.throws(() => ferroreport.fromTree({ tag: "html", attrs: [], children: [] }));
assert.throws(() => ferroreport.fromTree(tree, "pdf"));

// Limits: 9 orders and their 101 lines are 110 rows.
const limited = { resources: { "lines.jrxml": readFileSync(new URL("lines.jrxml", fx)) }, format: "csv" };
assert.ok(ferroreport.render(template, data, {}, { ...limited, maxRows: 110 }).length > 0);
assert.throws(() => ferroreport.render(template, data, {}, { ...limited, maxRows: 109 }), /reads more than 109 rows/);
assert.throws(() => ferroreport.render(template, data, {}, { ...limited, maxPages: 1 }), /more than 1 pages/);

assert.throws(() => ferroreport.render("<jasperReport", [], {}, {}));
assert.throws(() => ferroreport.render(template, data, {}, { format: "docx" }));
console.log(`ok: ${pdf.length} bytes, engine ${ferroreport.version()}`);
