# JasperReports' own samples, rendered by ferroreport

The 171 templates of the `demo/samples` folder of JasperReports 7.1.1, rendered by both engines from
the same rows and parameters and compared page by page. Made by `tools/samples.py`; nothing here is written by hand.

| Result | Templates | Meaning |
|---|---|---|
| same | 77 | every page is the same picture |
| differs | 16 | both engines render it, the pictures differ |
| not rendered | 8 | ferroreport refuses the template or fails on it |
| no reference | 70 | JasperReports cannot render it from rows alone (it needs the sample's Java classes, a database connection inside the report, or other files), so there is nothing to compare with |

**77 of the 101 templates that can be compared come out the same.** JasperReports runs with the
Java runtime's logical fonts (SansSerif, Serif, Monospaced) on DejaVu, as on a stock Linux server (`tools/oracle/fonts.conf`):
text that names no font is laid out with them and written in PDF's built-in Helvetica, Times or Courier, by both engines.

These samples were written to show every feature of JasperReports, one per sample: charts of every kind, maps, forms,
Java scriptlets, database connections inside the report. They are a much harder test than the reports people usually
write, which is what `fixtures/` holds (all pixel-identical). Use `ferroreport doctor your.jrxml` to see where yours stands.

| Template | Result | Pages | Why |
|---|---|---|---|
| `accessible/reports/AccessibleReport.jrxml` | differs | 2/2 | worst page differs in 12.3% of its ink |
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
| `book/reports/BookReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `book/reports/OrdersReport.jrxml` | same | 1/1 |  |
| `book/reports/PieChartReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `book/reports/TocReport.jrxml` | same | 1/1 |  |
| `chartcustomizers/reports/ChartCustomizersReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `charts/reports/AreaChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/Bar3DChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/BarChartReport.jrxml` | differs | 13/13 | worst page differs in 5.6% of its ink: chart customizers (customizerClass) are ignored |
| `charts/reports/BubbleChartReport.jrxml` | same | 19/19 |  |
| `charts/reports/CandlestickChartReport.jrxml` | same | 18/18 |  |
| `charts/reports/GanttChartReport.jrxml` | same | 1/1 |  |
| `charts/reports/HighLowChartReport.jrxml` | same | 18/18 |  |
| `charts/reports/LineChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/MeterChartReport.jrxml` | differs | 16/16 | worst page differs in 0.4% of its ink |
| `charts/reports/MultipleAxisChartReport.jrxml` | same | 24/24 |  |
| `charts/reports/Pie3DChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/PieChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/ScatterChartReport.jrxml` | same | 19/19 |  |
| `charts/reports/StackedAreaChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/StackedBar3DChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/StackedBarChartReport.jrxml` | same | 13/13 |  |
| `charts/reports/SubDatasetChartReport.jrxml` | same | 7/7 |  |
| `charts/reports/ThermometerChartReport.jrxml` | same | 16/16 |  |
| `charts/reports/TimeSeriesChartReport.jrxml` | same | 19/19 |  |
| `charts/reports/XYAreaChartReport.jrxml` | same | 19/19 |  |
| `charts/reports/XYBarChartReport.jrxml` | same | 19/19 |  |
| `charts/reports/XYBarChartTimePeriodReport.jrxml` | differs | 19/19 | worst page differs in 0.2% of its ink |
| `charts/reports/XYBarChartTimeSeriesReport.jrxml` | same | 19/19 |  |
| `charts/reports/XYLineChartReport.jrxml` | same | 19/19 |  |
| `chartthemes/reports/AllChartsReport.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `crosstabs/reports/LateOrdersReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport: error evaluating expression: variable `DeliveryDelay` cannot be used in crosstab buckets or measures |
| `crosstabs/reports/OrdersReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `crosstabs/reports/ProductsReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `crosstabs/reports/ShipmentsReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `csvdatasource/reports/CsvDataSourceReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `csvdatasource/reports/CsvQueryExecuterReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `datasource/reports/DataSourceReport.jrxml` | same | 1/1 |  |
| `daterange/reports/DateRangeReport.jrxml` | same | 1/1 |  |
| `ejbql/reports/JRMDbCastSubreport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `ejbql/reports/JRMDbReport.jrxml` | no reference |  | its rows come from a ejbql query; ferroreport renders it |
| `ejbql/reports/JRMDbVariaSubreport.jrxml` | no reference |  | its rows come from a ejbql query; ferroreport renders it |
| `ejbql-j2ee/reports/JRMDbCastSubreport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `ejbql-j2ee/reports/JRMDbReport.jrxml` | no reference |  | its rows come from a ejbql query; ferroreport renders it |
| `ejbql-j2ee/reports/JRMDbVariaSubreport.jrxml` | no reference |  | its rows come from a ejbql query; ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsDataAdapterReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsQeDataAdapterReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsxDataAdapterReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `exceldataadapter/reports/ExcelXlsxQeDataAdapterReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `exceldatasource/reports/ExcelDataSourceReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `fastexceldatasource/reports/FastExcelDataSourceReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `fonts/reports/FontsReport.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `forms/reports/FormsReport.jrxml` | differs | 1/1 | worst page differs in 33.5% of its ink |
| `functions/reports/FunctionsReport.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `genericelement/reports/GenericElementReport.jrxml` | same | 1/1 |  |
| `groovy/reports/GroovyReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `hibernate/reports/AddressesReport.jrxml` | no reference |  | its rows come from a hql query; ferroreport renders it |
| `hibernate/reports/DocumentsReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `hibernate/reports/HibernateQueryReport.jrxml` | no reference |  | its rows come from a hql query; ferroreport renders it |
| `hibernate-j2ee/reports/AddressesReport.jrxml` | no reference |  | its rows come from a hql query; ferroreport renders it |
| `hibernate-j2ee/reports/DocumentsReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `hibernate-j2ee/reports/HibernateQueryReport.jrxml` | no reference |  | its rows come from a hql query; ferroreport renders it |
| `horizontal/reports/HorizontalReport.jrxml` | same | 20/20 |  |
| `httpdataadapter/reports/JsonHttpDataAdapterReport.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `httpdataadapter/reports/XmlHttpDataAdapterReport.jrxml` | no reference |  | its rows come from a xPath query; ferroreport renders it |
| `hyperlink/reports/HyperlinkReport.jrxml` | same | 3/3 |  |
| `i18n/reports/I18nReport.jrxml` | differs | 1/1 | worst page differs in 9.6% of its ink |
| `iconlabel/reports/IconLabelReport.jrxml` | same | 2/2 |  |
| `images/reports/ImagesReport.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport: unsupported feature: image format (only PNG, JPEG, GIF and SVG are supported) |
| `images/reports/scale/ClipNarrowShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/ClipNarrowTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/ClipWideShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/ClipWideTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/FillFrameNarrowShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/FillFrameNarrowTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/FillFrameWideShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/FillFrameWideTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealHeightNarrowShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealHeightNarrowTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealHeightWideShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealHeightWideTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealSizeNarrowShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealSizeNarrowTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealSizeWideShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RealSizeWideTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RetainShapeNarrowShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RetainShapeNarrowTallImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RetainShapeWideShortImagesReport.jrxml` | same | 1/1 |  |
| `images/reports/scale/RetainShapeWideTallImagesReport.jrxml` | same | 1/1 |  |
| `jasper/reports/FirstJasper.jrxml` | same | 9/9 |  |
| `java1.5/reports/Java5Report.jrxml` | same | 1/1 |  |
| `javascript/reports/JavaScriptReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `jfreechart/reports/JFreeChartReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `jsondatasource/reports/JsonCustomersReport.jrxml` | no reference |  | its rows come from a json query; ferroreport renders it |
| `jsondatasource/reports/JsonOrdersReport.jrxml` | no reference |  | its rows come from a json query; ferroreport renders it |
| `jsonqldatasource/reports/NorthwindOrdersReport.jrxml` | no reference |  | its rows come from a jsonql query; ferroreport renders it |
| `landscape/reports/LandscapeReport.jrxml` | same | 1/1 |  |
| `list/reports/HorizontalListReport.jrxml` | same | 2/2 |  |
| `list/reports/ListReport.jrxml` | same | 1/1 |  |
| `markup/reports/MarkupReport.jrxml` | same | 1/1 |  |
| `maven/src/main/reports/com/bar/BarReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method SomeBarClass.isToPrint() |
| `maven/src/main/reports/com/foo/FooReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method SomeFooClass.isToPrint() |
| `maven/src/test/reports/com/bar/BarTestReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method BarTestClass.isToPrint() |
| `maven/src/test/reports/com/foo/FooTestReport.jrxml` | not rendered |  | error evaluating expression: unsupported static method FooTestClass.isToPrint() |
| `mondrian/reports/AdventureWorks-Olap4jXMLA-XJoinColumnsReport.jrxml` | no reference |  | its rows come from a olap4j query; ferroreport renders it |
| `mondrian/reports/AdventureWorks-Olap4jXMLAReport.jrxml` | no reference |  | its rows come from a olap4j query; ferroreport renders it |
| `mondrian/reports/AdventureWorks-XMLA-XJoinColumnsReport.jrxml` | no reference |  | its rows come from a mdx query; ferroreport renders it |
| `mondrian/reports/AdventureWorks-XMLAReport.jrxml` | no reference |  | its rows come from a mdx query; ferroreport renders it |
| `mondrian/reports/FoodMartSales-XMLAReport.jrxml` | no reference |  | its rows come from a mdx query; ferroreport renders it |
| `mondrian/reports/FoodMartSalesWeekly-XMLAReport.jrxml` | no reference |  | its rows come from a mdx query; ferroreport renders it |
| `mondrian/reports/MondrianFoodMartSalesOlap4jReport.jrxml` | no reference |  | its rows come from a olap4j query; ferroreport renders it |
| `mondrian/reports/MondrianFoodMartSalesReport.jrxml` | no reference |  | its rows come from a mdx query; ferroreport renders it |
| `mondrian/reports/MondrianReport.jrxml` | no reference |  | its rows come from a mdx query; ferroreport renders it |
| `nopagebreak/reports/NoPageBreakReport.jrxml` | same | 9/9 |  |
| `osgi/reports/ExcelXlsDataAdapterReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `osgi/reports/ExcelXlsQeDataAdapterReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `osgi/reports/GroovyReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `osgi/reports/ImagesReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport: invalid data: `dukesign.jpg` not found in the resource directories |
| `osgi/reports/JavaScriptReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `paragraphs/reports/ParagraphsReport.jrxml` | same | 1/1 |  |
| `pdfa/reports/PdfA1Report.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `pdfa/reports/PdfA2Report.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `pdfa/reports/PdfA3Report.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `pdfa/reports/TabularPdfA1Report.jrxml` | differs | 1/1 | worst page differs in 1.6% of its ink |
| `pdfa/reports/TaggedPdfA1Report.jrxml` | differs | 2/2 | worst page differs in 13.0% of its ink |
| `pdfencrypt/reports/PdfEncryptReport.jrxml` | same | 1/1 |  |
| `query/reports/QueryReport.jrxml` | same | 1/1 |  |
| `rotation/reports/RotationReport.jrxml` | differs | 1/1 | worst page differs in 1.3% of its ink |
| `scriptlet/reports/ScriptletReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `shapes/reports/ShapesReport.jrxml` | same | 1/1 |  |
| `spiderchartcomponent/reports/SpiderChart.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `springboot/reports/AllChartsReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `springboot/reports/ExcelXlsDataAdapterReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `springboot/reports/ExcelXlsQeDataAdapterReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `springboot/reports/ImagesReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport: unsupported feature: image format (only PNG, JPEG, GIF and SVG are supported) |
| `springboot/reports/StretchReport.jrxml` | no reference |  | runs SQL through a live database connection inside the report; ferroreport renders it |
| `stretch/reports/StretchReport.jrxml` | differs | 14/14 | worst page differs in 12.2% of its ink |
| `styledtext/reports/StyledTextReport.jrxml` | differs | 1/1 | worst page differs in 2.8% of its ink |
| `subreport/reports/AddressReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `subreport/reports/MasterReport.jrxml` | same | 1/1 |  |
| `subreport/reports/ProductReport.jrxml` | same | 1/1 |  |
| `table/reports/TableReport.jrxml` | differs | 2/2 | worst page differs in 3.6% of its ink |
| `tableofcontents/reports/TableOfContentsReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `tableofcontents/reports/TablePart.jrxml` | same | 27/27 |  |
| `tableofcontents/reports/TocPart.jrxml` | same | 1/1 |  |
| `tabular/reports/TabularReport.jrxml` | differs | 2/2 | worst page differs in 0.7% of its ink |
| `templates/reports/StylesReport.jrxml` | same | 1/1 |  |
| `text/reports/TextReport.jrxml` | same | 2/2 |  |
| `unicode/reports/NumberFormatReport.jrxml` | differs | 1/1 | worst page differs in 25.8% of its ink |
| `unicode/reports/UnicodeReport.jrxml` | differs | 2/2 | worst page differs in 64.9% of its ink |
| `virtualizer/reports/VirtualizerReport.jrxml` | same | 27/27 |  |
| `webapp/src/main/webapp/reports/WebappReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport: error evaluating expression: unsupported constructor new File() |
| `xchart/reports/XChartReport.jrxml` | no reference |  | needs Java classes that are not at hand (the sample's own, or libraries it adds); ferroreport renders it |
| `xchartcomponent/reports/XYChart.jrxml` | no reference |  | JasperReports fails on it without the sample's own program; ferroreport renders it |
| `xlsfeatures/reports/ExcelNames.jrxml` | differs | 1/1 | worst page differs in 8.0% of its ink |
| `xlsfeatures/reports/FormatPatternsMap.jrxml` | same | 1/1 |  |
| `xlsfeatures/reports/MacroReport.jrxml` | same | 1/1 |  |
| `xlsfeatures/reports/XlsFeaturesReport.jrxml` | same | 1/1 |  |
| `xlsformula/reports/XlsFormulaReport.jrxml` | same | 1/1 |  |
| `xmldatasource/reports/CustomersReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
| `xmldatasource/reports/OrdersReport.jrxml` | no reference |  | JasperReports draws nothing for it without the sample's own program; ferroreport renders it |
