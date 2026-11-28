# JasperReports' own samples, rendered by ferroreport

The 140 templates of the `demo/samples` folder of JasperReports 6.20.6, rendered by both engines from
the same rows and parameters and compared page by page. Made by `tools/samples.py`; nothing here is written by hand.

| Result | Templates | Meaning |
|---|---|---|
| same | 43 | every page is the same picture |
| differs | 39 | both engines render it, the pictures differ |
| not rendered | 6 | ferroreport refuses the template or fails on it |
| no reference | 52 | JasperReports cannot render it from rows alone (it needs the sample's Java classes, a database connection inside the report, or other files), so there is nothing to compare with |

**43 of the 88 templates that can be compared come out the same.** JasperReports runs with the
Java runtime's logical fonts (SansSerif, Serif, Monospaced) on DejaVu, as on a stock Linux server (`tools/oracle/fonts.conf`):
text that names no font is laid out with them and written in PDF's built-in Helvetica, Times or Courier, by both engines.

These samples were written to show every feature of JasperReports, one per sample: charts of every kind, maps, forms,
Java scriptlets, database connections inside the report. They are a much harder test than the reports people usually
write, which is what `fixtures/` holds (all pixel-identical). Use `ferroreport doctor your.jrxml` to see where yours stands.

| Template | Result | Pages | Why |
|---|---|---|---|
| `accessible/reports/AccessibleReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `alterdesign/reports/AlterDesignReport.jrxml` | same | 1/1 |  |
| `antcompile/reports/com/bar/BarReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method SomeBarClass.isToPrint() |
| `antcompile/reports/com/foo/FooReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method SomeFooClass.isToPrint() |
| `antupdate/reports/com/bar/BarReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method SomeBarClass.isToPrint() |
| `antupdate/reports/com/foo/FooReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method SomeFooClass.isToPrint() |
| `barbecue/reports/BarbecueReport.jrxml` | same | 1/1 |  |
| `barcode4j/reports/Barcode4JReport.jrxml` | same | 1/1 |  |
| `batchexport/reports/Report1.jrxml` | same | 2/2 |  |
| `batchexport/reports/Report2.jrxml` | same | 2/2 |  |
| `batchexport/reports/Report3.jrxml` | same | 2/2 |  |
| `book/reports/BookReport.jrxml` | differs | 1/1 | worst page differs in 99.3% of its ink |
| `book/reports/OrdersReport.jrxml` | same | 1/1 |  |
| `book/reports/PieChartReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `book/reports/TOCPart.jrxml` | same | 1/1 |  |
| `chartcustomizers/reports/ChartCustomizersReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `charts/reports/AreaChartReport.jrxml` | differs | 13/13 | worst page differs in 2.9% of its ink |
| `charts/reports/Bar3DChartReport.jrxml` | differs | 13/13 | worst page differs in 9.0% of its ink |
| `charts/reports/BarChartReport.jrxml` | differs | 13/13 | worst page differs in 7.8% of its ink: chart customizers (customizerClass) are ignored |
| `charts/reports/BubbleChartReport.jrxml` | differs | 19/19 | worst page differs in 8.1% of its ink |
| `charts/reports/CandlestickChartReport.jrxml` | differs | 18/18 | worst page differs in 3.7% of its ink |
| `charts/reports/GanttChartReport.jrxml` | differs | 1/1 | worst page differs in 8.0% of its ink |
| `charts/reports/HighLowChartReport.jrxml` | differs | 18/18 | worst page differs in 11.2% of its ink |
| `charts/reports/LineChartReport.jrxml` | differs | 13/13 | worst page differs in 3.2% of its ink |
| `charts/reports/MeterChartReport.jrxml` | differs | 16/16 | worst page differs in 1.5% of its ink |
| `charts/reports/MultipleAxisChartReport.jrxml` | differs | 24/24 | worst page differs in 9.2% of its ink |
| `charts/reports/Pie3DChartReport.jrxml` | differs | 13/13 | worst page differs in 8.6% of its ink |
| `charts/reports/PieChartReport.jrxml` | differs | 13/13 | worst page differs in 5.0% of its ink |
| `charts/reports/ScatterChartReport.jrxml` | differs | 19/19 | worst page differs in 10.7% of its ink |
| `charts/reports/StackedAreaChartReport.jrxml` | differs | 13/13 | worst page differs in 2.7% of its ink |
| `charts/reports/StackedBar3DChartReport.jrxml` | differs | 13/13 | worst page differs in 7.3% of its ink |
| `charts/reports/StackedBarChartReport.jrxml` | differs | 13/13 | worst page differs in 2.7% of its ink |
| `charts/reports/SubDatasetChartReport.jrxml` | differs | 7/7 | worst page differs in 18.9% of its ink |
| `charts/reports/ThermometerChartReport.jrxml` | differs | 16/16 | worst page differs in 0.7% of its ink |
| `charts/reports/TimeSeriesChartReport.jrxml` | differs | 19/19 | worst page differs in 11.8% of its ink |
| `charts/reports/XYAreaChartReport.jrxml` | differs | 19/19 | worst page differs in 6.2% of its ink |
| `charts/reports/XYBarChartReport.jrxml` | differs | 19/19 | worst page differs in 9.3% of its ink |
| `charts/reports/XYBarChartTimePeriodReport.jrxml` | differs | 19/19 | worst page differs in 11.4% of its ink |
| `charts/reports/XYBarChartTimeSeriesReport.jrxml` | differs | 19/19 | worst page differs in 11.3% of its ink |
| `charts/reports/XYLineChartReport.jrxml` | differs | 19/19 | worst page differs in 10.8% of its ink |
| `chartthemes/reports/AllChartsReport.jrxml` | differs | 10/10 | worst page differs in 94.4% of its ink |
| `crosstabs/reports/LateOrdersReport.jrxml` | not rendered |  | error evaluating expression: variable `DeliveryDelay` cannot be used in crosstab buckets or measures |
| `crosstabs/reports/OrdersReport.jrxml` | differs | 1/1 | worst page differs in 19.2% of its ink |
| `crosstabs/reports/ProductsReport.jrxml` | differs | 3/3 | worst page differs in 6.4% of its ink |
| `crosstabs/reports/ShipmentsReport.jrxml` | same | 2/2 |  |
| `csvdatasource/reports/CsvDataSourceReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `csvdatasource/reports/CsvQueryExecuterReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `customvisualization/reports/D3Rectangle.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `customvisualization/reports/Figures.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `customvisualization/reports/LeafletMarkers.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `customvisualization/reports/RadialProgress.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `customvisualization/reports/SimpleRectangle.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `customvisualization/reports/Sparkline.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `datasource/reports/DataSourceReport.jrxml` | same | 1/1 |  |
| `daterange/reports/DateRangeReport.jrxml` | same | 1/1 |  |
| `ejbql/reports/JRMDbCastSubreport.jrxml` | same | 1/1 |  |
| `ejbql/reports/JRMDbReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `ejbql/reports/JRMDbVariaSubreport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsDataAdapterReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsQeDataAdapterReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsxDataAdapterReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsxQeDataAdapterReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `exceldatasource/reports/ExcelDataSourceReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `fastexceldatasource/reports/FastExcelDataSourceReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `fonts/reports/FontsReport.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `forms/reports/FormsReport.jrxml` | differs | 1/1 | worst page differs in 33.6% of its ink |
| `functions/reports/FunctionsReport.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport renders it |
| `genericelement/reports/GenericElementReport.jrxml` | same | 1/1 |  |
| `groovy/reports/GroovyReport.jrxml` | same | 1/1 |  |
| `hibernate/reports/AddressesReport.jrxml` | same | 1/1 |  |
| `hibernate/reports/DocumentsReport.jrxml` | same | 1/1 |  |
| `hibernate/reports/HibernateQueryReport.jrxml` | same | 1/1 |  |
| `horizontal/reports/HorizontalReport.jrxml` | differs | 20/20 | worst page differs in 3.8% of its ink |
| `htmlcomponent/reports/HtmlComponentReport.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `httpdataadapter/reports/JsonHttpDataAdapterReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `httpdataadapter/reports/XmlHttpDataAdapterReport.jrxml` | no reference |  | its rows come from a xPath query; ferroreport renders it |
| `hyperlink/reports/HyperlinkReport.jrxml` | same | 3/3 |  |
| `i18n/reports/I18nReport.jrxml` | differs | 1/1 | worst page differs in 9.6% of its ink |
| `iconlabel/reports/IconLabelReport.jrxml` | same | 2/2 |  |
| `images/reports/ImagesReport.jrxml` | not rendered |  | unsupported feature: image format (only PNG, JPEG, GIF and SVG are supported) |
| `jasper/reports/FirstJasper.jrxml` | same | 9/9 |  |
| `java1.5/reports/Java5Report.jrxml` | same | 1/1 |  |
| `javascript/reports/JavaScriptReport.jrxml` | differs | 1/1 | worst page differs in 3.4% of its ink |
| `jfreechart/reports/JFreeChartReport.jrxml` | differs | 1/1 | worst page differs in 29.5% of its ink: scriptletClass (a Java scriptlet) is not run: expressions that call it fail |
| `jsondatasource/reports/JsonCustomersReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `jsondatasource/reports/JsonOrdersReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `jsonqldatasource/reports/NorthwindOrdersReport.jrxml` | no reference |  | its rows come from a jsonql query; ferroreport renders it |
| `landscape/reports/LandscapeReport.jrxml` | same | 1/1 |  |
| `list/reports/HorizontalListReport.jrxml` | same | 2/2 |  |
| `list/reports/ListReport.jrxml` | same | 1/1 |  |
| `map/reports/MapReport.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `map/reports/MapReport1.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `map/reports/MapReport2.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `map/reports/MapReport3.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `map/reports/MapReport4.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `map/reports/MapReport5.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `map/reports/MapReport6.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `map/reports/MapReport7.jrxml` | no reference |  | needs files that the sample's program provides; ferroreport: error evaluating expression: parameter `REPORT_CONTEXT` is not declared |
| `markup/reports/MarkupReport.jrxml` | same | 1/1 |  |
| `mondrian/reports/AdventureWorks-Olap4jXMLA-XJoinColumnsReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/AdventureWorks-Olap4jXMLAReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/AdventureWorks-XMLA-XJoinColumnsReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/AdventureWorks-XMLAReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/FoodMartSales-XMLAReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/FoodMartSalesWeekly-XMLAReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/MondrianFoodMartSalesOlap4jReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/MondrianFoodMartSalesReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `mondrian/reports/MondrianReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `nopagebreak/reports/NoPageBreakReport.jrxml` | same | 9/9 |  |
| `paragraphs/reports/ParagraphsReport.jrxml` | same | 1/1 |  |
| `pdfencrypt/reports/PdfEncryptReport.jrxml` | same | 1/1 |  |
| `query/reports/QueryReport.jrxml` | same | 1/1 |  |
| `rotation/reports/RotationReport.jrxml` | differs | 1/1 | worst page differs in 1.3% of its ink |
| `scriptlet/reports/ScriptletReport.jrxml` | differs | 2/2 | worst page differs in 1.7% of its ink: scriptletClass (a Java scriptlet) is not run: expressions that call it fail |
| `shapes/reports/ShapesReport.jrxml` | same | 1/1 |  |
| `spiderchartcomponent/reports/SpiderChart.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `stretch/reports/StretchReport.jrxml` | differs | 14/14 | worst page differs in 12.2% of its ink |
| `styledtext/reports/StyledTextReport.jrxml` | differs | 1/1 | worst page differs in 2.8% of its ink |
| `subreport/reports/AddressReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `subreport/reports/MasterReport.jrxml` | same | 1/1 |  |
| `subreport/reports/ProductReport.jrxml` | same | 1/1 |  |
| `table/reports/TableReport.jrxml` | same | 1/1 |  |
| `tableofcontents/reports/TableOfContentsReport.jrxml` | differs | 1/2 | worst page differs in 100.0% of its ink: a different number of pages |
| `tableofcontents/reports/TablePart.jrxml` | same | 27/27 |  |
| `tableofcontents/reports/TocPart.jrxml` | same | 1/1 |  |
| `tabular/reports/TabularReport.jrxml` | same | 2/2 |  |
| `templates/reports/StylesReport.jrxml` | same | 1/1 |  |
| `text/reports/TextReport.jrxml` | same | 2/2 |  |
| `unicode/reports/UnicodeReport.jrxml` | differs | 2/2 | worst page differs in 65.7% of its ink |
| `virtualizer/reports/VirtualizerReport.jrxml` | same | 27/27 |  |
| `webapp/web/reports/WebappReport.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport: error evaluating expression: unsupported constructor new File() |
| `xchart/reports/XChartReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `xchartcomponent/reports/XYChart.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `xlsfeatures/reports/ExcelNames.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `xlsfeatures/reports/FormatPatternsMap.jrxml` | same | 1/1 |  |
| `xlsfeatures/reports/MacroReport.jrxml` | same | 1/1 |  |
| `xlsfeatures/reports/XlsFeaturesReport.jrxml` | same | 1/1 |  |
| `xlsformula/reports/XlsFormulaReport.jrxml` | same | 1/1 |  |
| `xmldatasource/reports/CustomersReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `xmldatasource/reports/OrdersReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
