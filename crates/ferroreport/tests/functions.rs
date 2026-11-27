//! JasperReports' function library: each expression with the text JasperReports 6.20.6 printed
//! for it (fixtures/49-functions is the same list, compared with JasperReports page by page).

use ferroreport::expr::{eval, parse_expr, EvalEnv, Scope};
use ferroreport::format::{format_value, locale::EN_US};
use ferroreport::value::Value;

struct Empty;

impl Scope for Empty {
    fn field(&self, n: &str) -> ferroreport::Result<Value> {
        Err(ferroreport::Error::Eval(format!("no field {n}")))
    }
    fn param(&self, n: &str) -> ferroreport::Result<Value> {
        Err(ferroreport::Error::Eval(format!("no parameter {n}")))
    }
    fn variable(&self, n: &str) -> ferroreport::Result<Value> {
        Err(ferroreport::Error::Eval(format!("no variable {n}")))
    }
}

const JASPER: &[(&str, &str)] = &[
    ("DATE(2026,3,5)", "3/5/26, 12:00 AM"),
    ("YEAR(DATE(2026,3,5))", "2026"),
    ("MONTH(DATE(2026,3,5))", "3"),
    ("DAY(DATE(2026,3,5))", "5"),
    ("WEEKDAY(DATE(2026,3,5))", "4"),
    ("WEEKDAY(DATE(2026,3,5), true)", "5"),
    ("WEEKDAY(DATE(2026,3,5), false)", "4"),
    ("HOUR(DATE(2026,3,5))", "0"),
    ("DAYSINMONTH(DATE(2026,2,5))", "28"),
    ("DAYSINYEAR(DATE(2028,2,5))", "366"),
    ("ISLEAPYEAR(DATE(2028,1,1))", "true"),
    ("EDATE(DATE(2026,1,31), 1)", "2/28/26, 12:00 AM"),
    ("EDATE(DATE(2026,3,5), -14)", "1/5/25, 12:00 AM"),
    ("WORKDAY(DATE(2026,3,5), 3)", "3/10/26, 12:00 AM"),
    ("WORKDAY(DATE(2026,3,6), -1)", "3/5/26, 12:00 AM"),
    ("NETWORKDAYS(DATE(2026,3,1), DATE(2026,3,31))", "21"),
    ("DAYS(DATE(2026,1,1), DATE(2026,3,5))", "63"),
    ("DAYS(DATE(2026,3,5), DATE(2026,1,1))", "-63"),
    ("WEEKS(DATE(2026,1,1), DATE(2026,3,5))", "9"),
    ("MONTHS(DATE(2026,1,31), DATE(2026,3,5))", "1"),
    ("YEARS(DATE(2020,6,1), DATE(2026,3,5))", "5"),
    ("DATEFORMAT(DATE(2026,3,5), \"dd/MM/yyyy\")", "05/03/2026"),
    ("DATEFORMAT(DATE(2026,3,5), \"EEEE d 'de' MMMM\")", "Thursday 5 de March"),
    ("DATEVALUE(DATE(2026,3,5))", "1772668800000"),
    ("DATE(2026,3,5).getTime()", "1772668800000"),
    ("TIME(10, 30, 15)", "10:30:15 AM UTC"),
    ("TIME(10, 30, 15, \"hh:mm a\")", "10:30 AM"),
    ("MINUTE(DATE(2026,3,5))", "0"),
    ("SECOND(DATE(2026,3,5))", "0"),
    ("YEAR(\"2026-03-05\")", "null"),
    ("DAYS(\"2026-01-01\", \"2026-03-05\")", "null"),
    ("ABS(-3)", "3"),
    ("ABS(-3.5)", "3.5"),
    ("ABS(new java.math.BigDecimal(\"-2.50\"))", "2.5"),
    ("FACT(5)", "120"),
    ("ISEVEN(4)", "true"),
    ("ISODD(4)", "false"),
    ("PRODUCT(2, 3, 4)", "24.0"),
    ("PRODUCT(2, 3.5)", "7.0"),
    ("SIGN(-7)", "-1"),
    ("SIGN(0.0)", "0"),
    ("SQRT(16)", "4.0"),
    ("SUM(1, 2, 3)", "6.0"),
    ("SUM(1, 2.5)", "3.5"),
    ("SUM(new java.math.BigDecimal(\"1.10\"), 2)", "3.1"),
    ("MAX(1, 7, 3)", "7"),
    ("MAX(1, 7.5)", "7.5"),
    ("MIN(4, 2, 9)", "2"),
    ("FLOOR(2.7)", "2.0"),
    ("FLOOR(-2.5)", "-3.0"),
    ("CEIL(2.1)", "3.0"),
    ("AND(true, true, false)", "false"),
    ("OR(false, true)", "true"),
    ("NOT(true)", "false"),
    ("IF(1 > 2, \"yes\", \"no\")", "no"),
    ("IF(true, 5, \"x\")", "5"),
    ("EQUALS(1, 1)", "true"),
    ("EQUALS(1, 1L)", "false"),
    ("EQUALS(null, null)", "true"),
    ("EQUALS(\"a\", null)", "false"),
    ("TRUE()", "true"),
    ("ROUND_HALF_UP(2.345, 2)", "2.35"),
    ("ROUND_HALF_UP(2.5, 0)", "3"),
    ("ROUND_HALF_EVEN(2.5, 0)", "2"),
    ("ROUND_HALF_EVEN(2.345, 2)", "2.34"),
    ("ROUND_HALF_DOWN(2.345, 2)", "2.34"),
    ("ROUND_UP(2.341, 2)", "2.35"),
    ("ROUND_DOWN(2.349, 2)", "2.34"),
    ("ROUND_CEILING(-2.341, 2)", "-2.34"),
    ("ROUND_FLOOR(-2.341, 2)", "-2.35"),
    ("ROUND_HALF_UP(new java.math.BigDecimal(\"2.345\"), 2)", "2.35"),
    ("ISEVEN(3.7)", "false"),
    ("ISODD(-3)", "false"),
    ("SQRT(2)", "1.4142135623730951"),
    ("SQRT(new java.math.BigDecimal(\"2.25\"))", "1.5"),
    ("ABS(-3L)", "3"),
    ("ABS(-3.5f)", "3.5"),
    ("MAX(1L, 7)", "7.0"),
    ("MAX(new java.math.BigDecimal(\"2.5\"), 1)", "2.5"),
    ("MIN(2.5f, 3)", "2.5"),
    ("PRODUCT(new java.math.BigDecimal(\"1.5\"), 2)", "3.0"),
    ("SUM(1L, 2L)", "3.0"),
    ("FACT(0)", "1"),
    ("FACT(20)", "2432902008176640000"),
    ("CEIL(-2.1)", "-2.0"),
    ("FLOOR(new java.math.BigDecimal(\"2.9\"))", "2.0"),
    ("SIGN(new java.math.BigDecimal(\"-0.5\"))", "-1"),
    ("EXACT(\"Abc\", \"abc\")", "false"),
    ("EXACT(\"Abc\", \"Abc\")", "true"),
    ("DOUBLE_VALUE(\"3.25\")", "3.25"),
    ("FLOAT_VALUE(\"3.25\")", "3.25"),
    ("INTEGER_VALUE(\"42\")", "42"),
    ("LONG_VALUE(\"42\")", "42"),
    ("CODE(\"A\")", "65"),
    ("CODE(\"ñ\")", "241"),
    ("CHAR(65)", "A"),
    ("CHAR(241)", "ñ"),
    ("FIND(\"b\", \"abcb\")", "-1"),
    ("FIND(\"b\", \"abcb\", 3)", "-1"),
    ("FIND(\"B\", \"abcb\")", "-1"),
    ("FIND(\"z\", \"abcb\")", "-1"),
    ("SEARCH(\"B\", \"abcb\")", "1"),
    ("SEARCH(\"b?\", \"abcb\")", "-1"),
    ("SEARCH(\"c*\", \"abcb\")", "-1"),
    ("SEARCH(\"b\", \"abcb\", 3)", "3"),
    ("LEN(\"ñandú\")", "5"),
    ("LEFT(\"hello\")", "h"),
    ("LEFT(\"hello\", 3)", "hel"),
    ("RIGHT(\"hello\", 2)", "lo"),
    ("RIGHT(\"hello\")", "o"),
    ("MID(\"hello\", 2)", "ell"),
    ("MID(\"hello\", 2, 3)", "ell"),
    ("LOWER(\"ÁBC\")", "ábc"),
    ("UPPER(\"ñandú\")", "ÑANDÚ"),
    ("PROPER(\"hello WORLD foo-bar o'neil 3rd\")", "Hello World Foo-bar O'neil 3rd"),
    ("TRIM(\"  a   b  \")", "a   b"),
    ("LTRIM(\"  a  \")", "a  "),
    ("RTRIM(\"  a  \")", "  a"),
    ("CLEAN(\"a\\tb\\nc\")", "abc"),
    ("CONCATENATE(\"a\", \"b\", \"c\")", "abc"),
    ("CONCATENATE(\"a\", null, \"c\")", "anullc"),
    ("REPLACE(\"abcdef\", 2, 3, \"XY\")", "aXYef"),
    ("REPT(\"ab\", 3)", "ababab"),
    ("REPT(\"ab\", 0)", ""),
    ("SUBSTITUTE(\"a-b-c\", \"-\", \"+\")", "a+b+c"),
    ("SUBSTITUTE(\"a-b-c\", \"-\", \"+\", 2)", "a-b+c"),
    ("SUBSTITUTE(\"a-b-c\", \"-\", \"+\", 5)", "null"),
    ("T(\"text\")", "text"),
    ("T(5)", ""),
    ("T(null)", ""),
    ("BASE(255, 16)", "ff"),
    ("BASE(5, 2, 8)", "00000101"),
    ("BASE(255, 36)", "73"),
    ("LEN(null)", "null"),
    ("UPPER(null)", "null"),
    ("LEFT(null, 2)", "null"),
    ("TRIM(null)", "null"),
    ("ABS(new java.math.BigDecimal(\"-2.50\")).getClass().getSimpleName()", "Double"),
    ("ABS(-3.5f).getClass().getSimpleName()", "Float"),
    ("ABS(-3L).getClass().getSimpleName()", "Long"),
    ("MAX(1, 7, 3).getClass().getSimpleName()", "Integer"),
    ("MAX(1L, 7L).getClass().getSimpleName()", "Long"),
    ("MAX(1.5f, 2.5f).getClass().getSimpleName()", "Float"),
    ("MAX(new java.math.BigDecimal(\"2.5\"), new java.math.BigDecimal(\"3.5\")).getClass().getSimpleName()", "Double"),
    ("MAX(new java.math.BigDecimal(\"2.5\"), 1).getClass().getSimpleName()", "Double"),
    ("MIN(2.5f, 3).getClass().getSimpleName()", "Double"),
    ("MIN(2.5, 3.5).getClass().getSimpleName()", "Double"),
    ("SUM(1, 2).getClass().getSimpleName()", "Double"),
    ("SUM(new java.math.BigDecimal(\"1.10\"), new java.math.BigDecimal(\"2.20\")).getClass().getSimpleName()", "Double"),
    ("SUM(new java.math.BigDecimal(\"1.10\"), new java.math.BigDecimal(\"2.20\"))", "3.3000000000000003"),
    ("PRODUCT(new java.math.BigDecimal(\"1.5\"), new java.math.BigDecimal(\"2\")).getClass().getSimpleName()", "Double"),
    ("PRODUCT(new java.math.BigDecimal(\"1.5\"), new java.math.BigDecimal(\"2\"))", "3.0"),
    ("SQRT(16).getClass().getSimpleName()", "Double"),
    ("SQRT(new java.math.BigDecimal(\"2.25\")).getClass().getSimpleName()", "Double"),
    ("ROUND_HALF_UP(2.5, 0).getClass().getSimpleName()", "BigDecimal"),
    ("ROUND_HALF_UP(2.345, 2).toPlainString()", "2.35"),
    ("ROUND_HALF_UP(0.1 + 0.2, 20)", "0.30000000000000004000"),
    ("ROUND_HALF_UP(2.675, 2)", "2.68"),
    ("FIND(\"abcb\", \"b\")", "1"),
    ("FIND(\"abcb\", \"b\", 3)", "3"),
    ("FIND(\"abcb\", \"B\")", "-1"),
    ("FIND(\"abcb\", \"z\")", "-1"),
    ("FIND(\"abcb\", \"\")", "0"),
    ("SEARCH(\"abcb\", \"B\")", "-1"),
    ("SEARCH(\"abcb\", \"b\", 3)", "-1"),
    ("SEARCH(\"b\", \"ABCB\")", "1"),
    ("SEARCH(\"bc\", \"abcb\", 0)", "1"),
    ("SUM(1, null)", "null"),
    ("MAX(null, 2)", "null"),
    ("ABS(null)", "null"),
    ("SIGN(null)", "null"),
    ("SUM()", "null"),
    ("YEAR(1772668800000L)", "2026"),
    ("YEAR(new java.sql.Timestamp(0L))", "1970"),
    ("YEAR(null)", "null"),
    ("DAYS(DATE(2026,3,5), DATE(2026,3,5))", "0"),
    ("NETWORKDAYS(DATE(2026,3,2), DATE(2026,3,6))", "4"),
    ("NETWORKDAYS(DATE(2026,3,2), DATE(2026,3,2))", "0"),
    ("NETWORKDAYS(DATE(2026,3,1), DATE(2026,3,1))", "0"),
    ("NETWORKDAYS(DATE(2026,3,6), DATE(2026,3,9))", "1"),
    ("NETWORKDAYS(DATE(2026,3,9), DATE(2026,3,6))", "1"),
    ("NETWORKDAYS(DATE(2026,3,7), DATE(2026,3,8))", "0"),
    ("WORKDAY(DATE(2026,3,7), 1)", "3/10/26, 12:00 AM"),
    ("WORKDAY(DATE(2026,3,7), 0)", "3/7/26, 12:00 AM"),
    ("WORKDAY(DATE(2026,3,8), -1)", "3/5/26, 12:00 AM"),
    ("WORKDAY(DATE(2026,3,6), 5)", "3/13/26, 12:00 AM"),
    ("MONTHS(DATE(2026,1,31), DATE(2026,2,28))", "0"),
    ("MONTHS(DATE(2026,3,5), DATE(2026,1,1))", "-2"),
    ("MONTHS(DATE(2026,1,5), DATE(2026,3,4))", "1"),
    ("YEARS(DATE(2020,3,5), DATE(2026,3,5))", "6"),
    ("YEARS(DATE(2020,3,6), DATE(2026,3,5))", "5"),
    ("WEEKS(DATE(2026,1,1), DATE(2026,1,7))", "0"),
    ("WEEKS(DATE(2026,1,1), DATE(2026,1,8))", "1"),
    ("WEEKS(DATE(2026,1,4), DATE(2026,1,5))", "0"),
    ("WEEKNUM(DATE(2026,1,1))", "1"),
    ("WEEKNUM(DATE(2026,3,5))", "10"),
    ("WEEKNUM(DATE(2026,12,31))", "53"),
    ("WEEKNUM(DATE(2027,1,1))", "1"),
    ("WEEKSINYEAR(DATE(2026,3,5))", "53"),
    ("WEEKSINYEAR(DATE(2020,3,5))", "53"),
    // Aligned weeks (days 1-7 are week 1) and ISO's 52 or 53 weeks, whatever the locale.
    ("WEEKNUM(DATE(2026,1,4))", "1"),
    ("WEEKNUM(DATE(2026,1,7))", "1"),
    ("WEEKNUM(DATE(2026,1,8))", "2"),
    ("WEEKNUM(DATE(2024,12,31))", "53"),
    ("WEEKSINYEAR(DATE(2025,6,1))", "52"),
    ("WEEKSINYEAR(DATE(2024,6,1))", "52"),
    ("WEEKSINYEAR(DATE(2015,6,1))", "53"),
    ("EDATE(DATE(2024,2,29), 12)", "2/28/25, 12:00 AM"),
    ("EDATE(DATE(2026,3,31), -1)", "2/28/26, 12:00 AM"),
    ("DATEFORMAT(null, \"yyyy\")", "null"),
    ("DATEFORMAT(DATE(2026,3,5), null)", "null"),
    ("TIME(10, 30, 15).length()", "15"),
    ("DAYSINMONTH(DATE(2024,2,1))", "29"),
    ("HOUR(new java.util.Date(3600000L * 5))", "5"),
    ("DATE(2026, 3, 5).getClass().getName()", "java.util.Date"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 00:00:00.000 / 2017-05-01 00:00:00.000"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01+3\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01+3\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 00:00:00.000 / 2017-05-01 00:00:00.000"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01-3\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01-3\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 00:00:00.000 / 2017-05-01 00:00:00.000"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01 14:25:48\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01 14:25:48\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 14:25:48.000 / 2017-05-01 14:25:48.000"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01 14:25\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01 14:25\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 00:00:00.000 / 2017-05-01 00:00:00.000"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01T14:25:48\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01T14:25:48\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 00:00:00.000 / 2017-05-01 00:00:00.000"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01 14:25:48.123\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01 14:25:48.123\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 14:25:48.000 / 2017-05-01 14:25:48.000"),
    ("DATEFORMAT(DATERANGE(\"2017-05-01 14\").getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(\"2017-05-01 14\").getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2017-05-01 00:00:00.000 / 2017-05-01 00:00:00.000"),
    ("DATEFORMAT(DATERANGE(DATE(2026,3,5)).getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(DATE(2026,3,5)).getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2026-03-05 00:00:00.000 / 2026-03-05 00:00:00.000"),
    ("DATEFORMAT(DATERANGE(new java.util.Date(1772715600000L)).getStart(), \"yyyy-MM-dd HH:mm:ss.SSS\") + \" / \" + DATEFORMAT(DATERANGE(new java.util.Date(1772715600000L)).getEnd(), \"yyyy-MM-dd HH:mm:ss.SSS\")", "2026-03-05 13:00:00.000 / 2026-03-05 13:00:00.000"),
    ("DATERANGE(\"2017-05-01\")", "2017-05-01"),
    ("DATERANGE(\"2017-05-01\").getClass().getName()", "net.sf.jasperreports.types.date.FixedDate"),
    ("DATERANGE(DATE(2026,3,5)).getClass().getName()", "net.sf.jasperreports.types.date.FixedDate"),
    ("MID(\"abcdefg\", 3)", "cde"),
    ("MID(\"abcdefg\", 1)", "abcdefg"),
    ("MID(\"ab\", 2)", ""),
    ("DAYS(new java.util.Date(1772704800000L), new java.util.Date(1772784000000L))", "1"),
    ("DAYS(new java.util.Date(1772784000000L), new java.util.Date(1772704800000L))", "-1"),
    ("MONTHS(new java.util.Date(1772704800000L), new java.util.Date(1775390400000L))", "1"),
    ("DATEFORMAT(EDATE(new java.util.Date(1772704800000L), 1), \"yyyy-MM-dd HH:mm:ss\")", "2026-04-05 10:00:00"),
    ("DATEFORMAT(WORKDAY(new java.util.Date(1772704800000L), 1), \"yyyy-MM-dd HH:mm:ss\")", "2026-03-06 10:00:00"),
    ("NETWORKDAYS(new java.util.Date(1772704800000L), new java.util.Date(1772784000000L))", "1"),
    ("FACT(-1)", "null"),
    ("REPT(\"ab\", -1)", ""),
    ("DOUBLE_VALUE(null)", "null"),
    ("DOUBLE_VALUE(\" 2.5 \")", "2.5"),
    ("EXACT(null, \"a\")", "false"),
    ("EXACT(null, null)", "null"),
    ("AND(true, null)", "null"),
    ("IF(null, 1, 2)", "null"),
    ("NOT(null)", "null"),
    ("BASE(-255, 16)", "-ff"),
    ("BASE(5, 2, 2)", "101"),
    ("LTRIM(\"\\t\\n a\")", "a"),
    ("CLEAN(\"a\\u0007b\\u007fc\\u0085d\")", "abcd"),
    ("DATEVALUE(1772704800000L)", "1772704800000"),
    ("DAY(0)", "null"),
    ("DAY(5L)", "1"),
    ("YEAR(DATE(1,1,1))", "1"),
    ("SUBSTITUTE(\"aaa\", \"a\", \"b\", 0)", "null"),
    ("SUBSTITUTE(\"aaa\", \"\", \"b\")", "bababab"),
    ("SUBSTITUTE(null, \"a\", \"b\")", "null"),
    ("SEARCH(\"b\", null)", "null"),
    ("FIND(null, \"b\")", "null"),
    ("LEFT(\"hello\", 0)", ""),
    ("REPLACE(\"abc\", 2, 0, \"Z\")", "aZbc"),
    ("TIME(0, 0, 0)", "12:00:00 AM UTC"),
    ("TIME(23, 59, 59, \"HH:mm\")", "23:59"),
    ("WEEKDAY(null)", "null"),
    ("CONCATENATE()", "null"),
    ("SQRT(-1)", "NaN"),
    ("FACT(21)", "-4249290049419214848"),
    ("ISEVEN(-4)", "true"),
    ("SIGN(-0.0)", "0"),
    ("MAX(1)", "1"),
    ("MAX(1, 2.0f)", "2.0"),
    ("ABS(Integer.MIN_VALUE)", "-2147483648"),
];

#[test]
fn every_function_gives_what_jasperreports_gives() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    let env = EvalEnv { locale: &EN_US, groovy: false, now };
    let mut wrong = Vec::new();
    for (src, want) in JASPER {
        let got = parse_expr(src, false)
            .map_err(|e| e.to_string())
            .and_then(|e| eval(&e, &Empty, &env).map_err(|e| e.to_string()))
            .map(|v| if v.is_null() { "null".to_string() } else { format_value(&v, None, &EN_US) });
        if got.as_deref() != Ok(*want) {
            wrong.push(format!("{src}: JasperReports {want:?}, ferroreport {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{} of {} differ:\n{}", wrong.len(), JASPER.len(), wrong.join("\n"));
}

/// What depends on the locale, as Java 21.0.13 has it: `TIME()` without a pattern
/// (`DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG)`) and the first day of
/// `DATERANGE("WEEK")` (`Calendar.getFirstDayOfWeek()`). Week numbers do not depend on it.
#[test]
fn time_week_start_and_week_numbers_by_locale() {
    // A Monday.
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    let run = |src: &str, tag: &str| {
        let locale = ferroreport::format::Locale::from_tag(tag);
        let env = EvalEnv { locale, groovy: false, now };
        let v = eval(&parse_expr(src, false).unwrap(), &Empty, &env).unwrap();
        format_value(&v, None, locale)
    };
    let week_start = "DATEFORMAT(DATERANGE(\"WEEK\").getStart(), \"yyyy-MM-dd\")";
    for (tag, time, start) in [
        ("fr", "10:30:15 UTC", "2026-11-22"),
        ("fr_FR", "10:30:15 UTC", "2026-11-23"),
        ("fr_BE", "10:30:15 UTC", "2026-11-23"),
        ("fr_CH", "10:30:15 UTC", "2026-11-23"),
        ("fr_CA", "10 h 30 min 15 s UTC", "2026-11-22"),
        ("es_ES", "10:30:15 UTC", "2026-11-23"),
        ("en_US", "10:30:15\u{202F}AM UTC", "2026-11-22"),
    ] {
        assert_eq!(run("TIME(10, 30, 15)", tag), time, "{tag}");
        assert_eq!(run(week_start, tag), start, "{tag}");
        assert_eq!(run("WEEKNUM(DATE(2026,1,4))", tag), "1", "{tag}");
        assert_eq!(run("WEEKNUM(DATE(2027,1,1))", tag), "1", "{tag}");
    }
}

/// `TIME()` without a pattern and the first day of `DATERANGE("WEEK")` in the French regions
/// (Java 21.0.13: `DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG)`,
/// `Calendar.getFirstDayOfWeek()`): a 12-hour clock in Algeria, Djibouti, Syria, Mauritania,
/// Tunisia, Vanuatu and Chad, weeks starting on Saturday in the first three.
#[test]
fn french_regions_time_and_week_start() {
    // A Monday.
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    let h24 = ("10:30:15 UTC", "23:05:09 UTC");
    let h12 = ("10:30:15\u{202F}AM UTC", "11:05:09\u{202F}PM UTC");
    for (tag, (morning, evening), start) in [
        ("fr_LU", h24, "2026-11-23"),
        ("fr-LU", h24, "2026-11-23"),
        ("fr_MA", h24, "2026-11-23"),
        ("fr_SN", h24, "2026-11-23"),
        ("fr-CI", h24, "2026-11-23"),
        ("fr_CM", h24, "2026-11-23"),
        ("fr_ML", h24, "2026-11-23"),
        ("fr_MC", h24, "2026-11-23"),
        ("fr_TD", h12, "2026-11-23"),
        ("fr_MR", h12, "2026-11-23"),
        ("fr_TN", h12, "2026-11-23"),
        ("fr_VU", h12, "2026-11-23"),
        ("fr_DZ", h12, "2026-11-21"),
        ("fr-DJ", h12, "2026-11-21"),
        ("fr_SY", h12, "2026-11-21"),
    ] {
        let locale = ferroreport::format::Locale::from_tag(tag);
        let env = EvalEnv { locale, groovy: false, now };
        let run = |src: &str| format_value(&eval(&parse_expr(src, false).unwrap(), &Empty, &env).unwrap(), None, locale);
        assert_eq!(run("TIME(10, 30, 15)"), morning, "{tag}");
        assert_eq!(run("TIME(23, 5, 9)"), evening, "{tag}");
        assert_eq!(run("DATEFORMAT(DATERANGE(\"WEEK\").getStart(), \"yyyy-MM-dd\")"), start, "{tag}");
    }
}

/// `TIME()` without a pattern in Spanish, as Java 21.0.13's
/// `DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG)` prints it.
#[test]
fn spanish_time_without_a_pattern() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    for (tag, morning, evening) in [
        ("es_DO", "10:30:15\u{202F}a.\u{A0}m. UTC", "11:05:09\u{202F}p.\u{A0}m. UTC"),
        ("es_MX", "10:30:15 UTC", "23:05:09 UTC"),
        ("es_ES", "10:30:15 UTC", "23:05:09 UTC"),
    ] {
        let locale = ferroreport::format::Locale::from_tag(tag);
        let env = EvalEnv { locale, groovy: false, now };
        let run = |src: &str| format_value(&eval(&parse_expr(src, false).unwrap(), &Empty, &env).unwrap(), None, locale);
        assert_eq!(run("TIME(10, 30, 15)"), morning, "{tag}");
        assert_eq!(run("TIME(23, 5, 9)"), evening, "{tag}");
    }
}

/// `TIME()` without a pattern in Portuguese (Java 21.0.13,
/// `DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG)`).
#[test]
fn portuguese_time_without_a_pattern() {
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    for tag in ["pt", "pt_BR", "pt_PT"] {
        let locale = ferroreport::format::Locale::from_tag(tag);
        let env = EvalEnv { locale, groovy: false, now };
        let run = |src: &str| format_value(&eval(&parse_expr(src, false).unwrap(), &Empty, &env).unwrap(), None, locale);
        assert_eq!(run("TIME(10, 30, 15)"), "10:30:15 UTC", "{tag}");
        assert_eq!(run("TIME(23, 5, 9)"), "23:05:09 UTC", "{tag}");
    }
}

/// `TIME()` without a pattern and the first day of `DATERANGE("WEEK")` in Italian (Java
/// 21.0.13: `DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG)`, `Calendar.getFirstDayOfWeek()`).
#[test]
fn italian_time_and_week_start() {
    // A Monday.
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    for (tag, start) in [("it", "2026-11-22"), ("it_IT", "2026-11-23"), ("it_CH", "2026-11-23")] {
        let locale = ferroreport::format::Locale::from_tag(tag);
        let env = EvalEnv { locale, groovy: false, now };
        let run = |src: &str| format_value(&eval(&parse_expr(src, false).unwrap(), &Empty, &env).unwrap(), None, locale);
        assert_eq!(run("TIME(10, 30, 15)"), "10:30:15 UTC", "{tag}");
        assert_eq!(run("TIME(23, 5, 9)"), "23:05:09 UTC", "{tag}");
        assert_eq!(run("DATEFORMAT(DATERANGE(\"WEEK\").getStart(), \"yyyy-MM-dd\")"), start, "{tag}");
    }
}

/// `TIME()` without a pattern and the first day of `DATERANGE("WEEK")` in German (Java
/// 21.0.13: `DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG)`, `Calendar.getFirstDayOfWeek()`).
#[test]
fn german_time_and_week_start() {
    // A Monday.
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    for (tag, start) in [("de", "2026-11-22"), ("de_DE", "2026-11-23"), ("de_AT", "2026-11-23"), ("de_CH", "2026-11-23")] {
        let locale = ferroreport::format::Locale::from_tag(tag);
        let env = EvalEnv { locale, groovy: false, now };
        let run = |src: &str| format_value(&eval(&parse_expr(src, false).unwrap(), &Empty, &env).unwrap(), None, locale);
        assert_eq!(run("TIME(10, 30, 15)"), "10:30:15 UTC", "{tag}");
        assert_eq!(run("TIME(23, 5, 9)"), "23:05:09 UTC", "{tag}");
        assert_eq!(run("DATEFORMAT(DATERANGE(\"WEEK\").getStart(), \"yyyy-MM-dd\")"), start, "{tag}");
    }
}

/// `TIME()` without a pattern and the first day of `DATERANGE("WEEK")` for the Spanish locales
/// (Java 21.0.13: `DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG)`,
/// `Calendar.getFirstDayOfWeek()`): `H:mm:ss z` in Spain, a 12-hour clock in Colombia, Venezuela,
/// the Philippines, Panama, Puerto Rico, the United States and es_DO, `HH:mm:ss z` elsewhere.
#[test]
fn spanish_time_and_week_start() {
    // A Monday.
    let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 54, 18).unwrap();
    let spain = ("9:05:00 UTC", "23:05:09 UTC");
    let h24 = ("09:05:00 UTC", "23:05:09 UTC");
    let h12 = ("9:05:00\u{202F}a.\u{A0}m. UTC", "11:05:09\u{202F}p.\u{A0}m. UTC");
    let monday = "2026-11-23";
    let sunday = "2026-11-22";
    for (tag, (morning, evening), start) in [
        ("es", spain, sunday),
        ("es_ES", spain, monday),
        ("es_EA", spain, monday),
        ("es_IC", spain, monday),
        ("es_GQ", spain, monday),
        ("es_AR", h24, monday),
        ("es-AR", h24, monday),
        ("es_EC", h24, monday),
        ("es_CL", h24, monday),
        ("es_UY", h24, monday),
        ("es_BO", h24, monday),
        ("es_CR", h24, monday),
        ("es_CU", h24, monday),
        ("es_419", h24, monday),
        ("es-419", h24, monday),
        ("es_PY", h24, sunday),
        ("es_HN", h24, sunday),
        ("es_NI", h24, sunday),
        ("es_SV", h24, sunday),
        ("es_BZ", h24, sunday),
        ("es_BR", h24, sunday),
        ("es_GT", h24, sunday),
        ("es_PE", h24, sunday),
        ("es-PE", h24, sunday),
        ("es_MX", h24, sunday),
        ("es_DO", h12, sunday),
        ("es_VE", h12, sunday),
        ("es_PH", h12, sunday),
        ("es_PA", h12, sunday),
        ("es_PR", h12, sunday),
        ("es_US", h12, sunday),
        ("es-US", h12, sunday),
        ("es_CO", ("9:05:00\u{202F}a.\u{202F}m. UTC", "11:05:09\u{202F}p.\u{202F}m. UTC"), sunday),
    ] {
        let locale = ferroreport::format::Locale::from_tag(tag);
        let env = EvalEnv { locale, groovy: false, now };
        let run = |src: &str| format_value(&eval(&parse_expr(src, false).unwrap(), &Empty, &env).unwrap(), None, locale);
        assert_eq!(run("TIME(9, 5, 0)"), morning, "{tag}");
        assert_eq!(run("TIME(23, 5, 9)"), evening, "{tag}");
        assert_eq!(run("DATEFORMAT(DATERANGE(\"WEEK\").getStart(), \"yyyy-MM-dd\")"), start, "{tag}");
    }
}
