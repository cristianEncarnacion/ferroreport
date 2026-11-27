//! The functions JasperReports offers in every expression (`net.sf.jasperreports.functions.
//! standard`), as Jaspersoft Studio's expression editor lists them: `IF`, `SUM`, `DATEFORMAT`,
//! `DATERANGE`... Each gives what JasperReports 6.20.6 gives, quirks included: `SUM` is always a
//! Double, `FIND` counts from 0, `MID("abcdefg", 3)` is `"cde"`, `YEAR("2026-03-05")` is null.

use super::eval::EvalEnv;
use crate::error::{Error, Result};
use crate::format::date::format_date;
use crate::format::locale::Locale;
use crate::value::{java_double_to_string, java_float_to_string, DateRange, Value};
use chrono::{Datelike, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday};
use rust_decimal::{Decimal, RoundingStrategy};
use std::sync::Arc;

/// Every function, sorted (for the binary search in [`is_function`]).
const NAMES: &[&str] = &[
    "ABS",
    "AND",
    "BASE",
    "CEIL",
    "CHAR",
    "CLEAN",
    "CODE",
    "CONCATENATE",
    "DATE",
    "DATEFORMAT",
    "DATERANGE",
    "DATEVALUE",
    "DAY",
    "DAYS",
    "DAYSINMONTH",
    "DAYSINYEAR",
    "DOUBLE_VALUE",
    "EDATE",
    "EQUALS",
    "EXACT",
    "FACT",
    "FALSE",
    "FIND",
    "FLOAT_VALUE",
    "FLOOR",
    "HOUR",
    "IF",
    "INTEGER_VALUE",
    "ISEVEN",
    "ISLEAPYEAR",
    "ISODD",
    "LEFT",
    "LEN",
    "LONG_VALUE",
    "LOWER",
    "LTRIM",
    "MAX",
    "MID",
    "MIN",
    "MINUTE",
    "MONTH",
    "MONTHS",
    "NETWORKDAYS",
    "NOT",
    "NOW",
    "OR",
    "PRODUCT",
    "PROPER",
    "RAND",
    "RANDBETWEEN",
    "REPLACE",
    "REPT",
    "RIGHT",
    "ROUND_CEILING",
    "ROUND_DOWN",
    "ROUND_FLOOR",
    "ROUND_HALF_DOWN",
    "ROUND_HALF_EVEN",
    "ROUND_HALF_UP",
    "ROUND_UP",
    "RTRIM",
    "SEARCH",
    "SECOND",
    "SIGN",
    "SQRT",
    "SUBSTITUTE",
    "SUM",
    "T",
    "TIME",
    "TODAY",
    "TRIM",
    "TRUE",
    "UPPER",
    "WEEKDAY",
    "WEEKNUM",
    "WEEKS",
    "WEEKSINYEAR",
    "WORKDAY",
    "YEAR",
    "YEARS",
];

/// Whether `name` is one of JasperReports' standard functions.
pub fn is_function(name: &str) -> bool {
    NAMES.binary_search(&name).is_ok()
}

/// The classes the functions live in, for calls written `MathFunctions.SUM(...)`.
pub fn is_function_class(class: &str) -> bool {
    matches!(class, "DateTimeFunctions" | "MathFunctions" | "TextFunctions" | "LogicalFunctions")
}

fn fail(name: &str, message: impl std::fmt::Display) -> Error {
    Error::eval(format!("{name}: {message}"))
}

/// Calls the function `name`.
pub fn call(name: &str, args: Vec<Value>, env: &EvalEnv) -> Result<Value> {
    let count = |min: usize, max: usize| -> Result<()> {
        if args.len() < min || args.len() > max {
            Err(fail(name, format!("takes {min} to {max} arguments, not {}", args.len())))
        } else {
            Ok(())
        }
    };
    let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    match name {
        // Logical.
        "TRUE" => Ok(Value::Bool(true)),
        "FALSE" => Ok(Value::Bool(false)),
        "AND" | "OR" => {
            let mut result = name == "AND";
            for v in &args {
                match v {
                    Value::Null => return Ok(Value::Null),
                    Value::Bool(b) if name == "AND" => result &= *b,
                    Value::Bool(b) => result |= *b,
                    other => return Err(fail(name, format!("{} is not a Boolean", other.type_name()))),
                }
            }
            Ok(Value::Bool(result))
        }
        "NOT" => {
            count(1, 1)?;
            match a(0) {
                Value::Null => Ok(Value::Null),
                Value::Bool(b) => Ok(Value::Bool(!b)),
                other => Err(fail(name, format!("{} is not a Boolean", other.type_name()))),
            }
        }
        "IF" => {
            count(3, 3)?;
            match a(0) {
                Value::Null => Ok(Value::Null),
                Value::Bool(true) => Ok(a(1)),
                Value::Bool(false) => Ok(a(2)),
                other => Err(fail(name, format!("{} is not a Boolean", other.type_name()))),
            }
        }
        "EQUALS" => {
            count(2, 2)?;
            Ok(Value::Bool(strict_equals(&a(0), &a(1))))
        }
        // Math.
        "ABS" => {
            count(1, 1)?;
            Ok(match number(name, &a(0))? {
                None => Value::Null,
                Some(Value::Int(n)) => Value::Int(n.wrapping_abs()),
                Some(Value::Long(n)) => Value::Long(n.wrapping_abs()),
                Some(Value::Float(f)) => Value::Float(f.abs()),
                Some(v) => Value::Double(f64_of(&v).abs()),
            })
        }
        "SQRT" | "FLOOR" | "CEIL" => {
            count(1, 1)?;
            Ok(match number(name, &a(0))? {
                None => Value::Null,
                Some(v) => {
                    let x = f64_of(&v);
                    Value::Double(match name {
                        "SQRT" => x.sqrt(),
                        "FLOOR" => x.floor(),
                        _ => x.ceil(),
                    })
                }
            })
        }
        "SIGN" => {
            count(1, 1)?;
            Ok(match number(name, &a(0))? {
                None => Value::Null,
                Some(v) => {
                    let x = f64_of(&v);
                    Value::Int(if x > 0.0 {
                        1
                    } else if x < 0.0 {
                        -1
                    } else {
                        0
                    })
                }
            })
        }
        "ISEVEN" | "ISODD" => {
            count(1, 1)?;
            Ok(match number(name, &a(0))? {
                None => Value::Null,
                // Number.intValue(), and Java's remainder: -3 % 2 is -1, so ISODD(-3) is false.
                Some(v) => {
                    let n = int_value(&v);
                    Value::Bool(if name == "ISEVEN" { n % 2 == 0 } else { n % 2 == 1 })
                }
            })
        }
        "FACT" => {
            count(1, 1)?;
            Ok(match whole(name, &a(0))? {
                None => Value::Null,
                Some(n) if n < 0 => Value::Null,
                // A long overflows as it does in Java; from 66! on, it has wrapped to 0.
                Some(n) if n >= 66 => Value::Long(0),
                Some(n) => Value::Long((1..=n).fold(1i64, |p, k| p.wrapping_mul(k))),
            })
        }
        "SUM" | "PRODUCT" => {
            let Some(values) = numbers(name, &args)? else { return Ok(Value::Null) };
            let xs = values.iter().map(f64_of);
            Ok(Value::Double(if name == "SUM" { xs.sum() } else { xs.product() }))
        }
        "MAX" | "MIN" => {
            let Some(values) = numbers(name, &args)? else { return Ok(Value::Null) };
            let pick = |a: f64, b: f64| if name == "MAX" { a.max(b) } else { a.min(b) };
            let best = values.iter().map(f64_of).fold(if name == "MAX" { f64::NEG_INFINITY } else { f64::INFINITY }, pick);
            // The type of the numbers when they all have the same one (but not BigDecimal),
            // a Double otherwise.
            let same = |f: fn(&Value) -> bool| values.iter().all(f);
            Ok(if same(|v| matches!(v, Value::Int(_))) {
                Value::Int(best as i32)
            } else if same(|v| matches!(v, Value::Long(_))) {
                let longs = values.iter().filter_map(Value::as_i64);
                Value::Long(if name == "MAX" { longs.max().unwrap_or(0) } else { longs.min().unwrap_or(0) })
            } else if same(|v| matches!(v, Value::Float(_))) {
                Value::Float(best as f32)
            } else {
                Value::Double(best)
            })
        }
        "RAND" => {
            count(0, 0)?;
            Ok(Value::Double(random_unit()))
        }
        "RANDBETWEEN" => {
            count(2, 2)?;
            let (Some(low), Some(high)) = (whole(name, &a(0))?, whole(name, &a(1))?) else { return Ok(Value::Null) };
            if high < low {
                return Err(fail(name, "the upper bound is below the lower one"));
            }
            Ok(Value::Int((low + (random_unit() * (high - low + 1) as f64) as i64) as i32))
        }
        "ROUND_UP" | "ROUND_DOWN" | "ROUND_CEILING" | "ROUND_FLOOR" | "ROUND_HALF_UP" | "ROUND_HALF_DOWN" | "ROUND_HALF_EVEN" => {
            count(2, 2)?;
            let (Some(v), Some(scale)) = (number(name, &a(0))?, whole(name, &a(1))?) else { return Ok(Value::Null) };
            Ok(Value::Decimal(round(name, &v, scale)?))
        }
        // Text.
        "LEN" => text_fn(name, &a(0), |s| Value::Int(utf16_len(s))),
        "LOWER" => text_fn(name, &a(0), |s| Value::str(s.to_lowercase())),
        "UPPER" => text_fn(name, &a(0), |s| Value::str(s.to_uppercase())),
        // String.trim(): everything up to a space, control characters included.
        "TRIM" => text_fn(name, &a(0), |s| Value::str(s.trim_matches(|c: char| c <= ' '))),
        "LTRIM" => text_fn(name, &a(0), |s| Value::str(s.trim_start())),
        "RTRIM" => text_fn(name, &a(0), |s| Value::str(s.trim_end())),
        "CLEAN" => text_fn(name, &a(0), |s| Value::str(s.chars().filter(|c| *c >= ' ' && *c != '\u{7f}').collect::<String>())),
        "PROPER" => text_fn(name, &a(0), |s| Value::str(proper(s))),
        "T" => {
            count(1, 1)?;
            Ok(match a(0) {
                Value::Str(s) => Value::Str(s),
                _ => Value::str(""),
            })
        }
        "CODE" => {
            count(1, 1)?;
            match text(&a(0)) {
                None => Ok(Value::Null),
                Some(s) => {
                    let unit = s.encode_utf16().next().ok_or_else(|| fail(name, "StringIndexOutOfBoundsException"))?;
                    Ok(Value::Int(unit as i32))
                }
            }
        }
        "CHAR" => {
            count(1, 1)?;
            Ok(match whole(name, &a(0))? {
                None => Value::Null,
                Some(n) => Value::str(char::from_u32((n as u16) as u32).unwrap_or('\u{fffd}').to_string()),
            })
        }
        "CONCATENATE" => {
            if args.is_empty() {
                return Ok(Value::Null);
            }
            Ok(Value::str(args.iter().map(Value::to_java_string).collect::<String>()))
        }
        "EXACT" => {
            count(2, 2)?;
            Ok(match (text(&a(0)), text(&a(1))) {
                (None, None) => Value::Null,
                (Some(x), Some(y)) => Value::Bool(x == y),
                _ => Value::Bool(false),
            })
        }
        "DOUBLE_VALUE" | "FLOAT_VALUE" | "INTEGER_VALUE" | "LONG_VALUE" => {
            count(1, 1)?;
            let Some(s) = text(&a(0)) else { return Ok(Value::Null) };
            let bad = || fail(name, format!("NumberFormatException: For input string: \"{s}\""));
            // Double.valueOf trims the text; Integer.valueOf does not.
            Ok(match name {
                "DOUBLE_VALUE" => Value::Double(java_float_text(&s).ok_or_else(bad)?),
                "FLOAT_VALUE" => Value::Float(java_float_text(&s).ok_or_else(bad)? as f32),
                "INTEGER_VALUE" => Value::Int(s.strip_prefix('+').unwrap_or(&s).parse().map_err(|_| bad())?),
                _ => Value::Long(s.strip_prefix('+').unwrap_or(&s).parse().map_err(|_| bad())?),
            })
        }
        "LEFT" | "RIGHT" => {
            count(1, 2)?;
            let Some(s) = text(&a(0)) else { return Ok(Value::Null) };
            let n = if args.len() > 1 { whole(name, &a(1))?.unwrap_or(1) } else { 1 };
            let chars: Vec<char> = s.chars().collect();
            if n < 0 || n as usize > chars.len() {
                return Err(fail(name, "StringIndexOutOfBoundsException"));
            }
            let n = n as usize;
            Ok(Value::str(if name == "LEFT" {
                chars[..n].iter().collect::<String>()
            } else {
                chars[chars.len() - n..].iter().collect()
            }))
        }
        "MID" => {
            count(2, 3)?;
            let Some(s) = text(&a(0)) else { return Ok(Value::Null) };
            let chars: Vec<char> = s.chars().collect();
            let start = whole(name, &a(1))?.unwrap_or(1) - 1;
            // With no length, JasperReports takes as many characters off the end as it skipped
            // at the start: MID("abcdefg", 3) is "cde".
            let end = if args.len() > 2 { start + whole(name, &a(2))?.unwrap_or(0) } else { chars.len() as i64 - start };
            if start < 0 || end < start || end as usize > chars.len() {
                return Err(fail(name, "StringIndexOutOfBoundsException"));
            }
            Ok(Value::str(chars[start as usize..end as usize].iter().collect::<String>()))
        }
        "REPLACE" => {
            count(4, 4)?;
            let Some(s) = text(&a(0)) else { return Ok(Value::Null) };
            let chars: Vec<char> = s.chars().collect();
            let start = whole(name, &a(1))?.unwrap_or(1) - 1;
            let n = whole(name, &a(2))?.unwrap_or(0);
            if start < 0 || start as usize > chars.len() || n < 0 {
                return Err(fail(name, "StringIndexOutOfBoundsException"));
            }
            let end = ((start + n) as usize).min(chars.len());
            let mut out: String = chars[..start as usize].iter().collect();
            out.push_str(&a(3).to_java_string());
            out.extend(&chars[end..]);
            Ok(Value::str(out))
        }
        "REPT" => {
            count(2, 2)?;
            let Some(s) = text(&a(0)) else { return Ok(Value::Null) };
            let n = whole(name, &a(1))?.unwrap_or(0);
            if n.saturating_mul(s.len() as i64) > 64 << 20 {
                return Err(fail(name, "the text would be larger than 64 MB"));
            }
            Ok(Value::str(s.repeat(n.max(0) as usize)))
        }
        "FIND" | "SEARCH" => {
            count(2, 3)?;
            // FIND(text, what[, from]) and SEARCH(what, text[, from]), case-insensitive; both
            // count from 0 and give -1 when it is not there.
            let (haystack, needle) = if name == "FIND" { (a(0), a(1)) } else { (a(1), a(0)) };
            let (Some(mut haystack), Some(mut needle)) = (text(&haystack), text(&needle)) else { return Ok(Value::Null) };
            if name == "SEARCH" {
                haystack = haystack.to_lowercase();
                needle = needle.to_lowercase();
            }
            let from = if args.len() > 2 { whole(name, &a(2))?.unwrap_or(0) } else { 0 };
            Ok(Value::Int(index_of(&haystack, &needle, from)))
        }
        "SUBSTITUTE" => {
            count(3, 4)?;
            let (Some(s), Some(old), Some(new)) = (text(&a(0)), text(&a(1)), text(&a(2))) else { return Ok(Value::Null) };
            if args.len() < 4 {
                return Ok(Value::str(s.replace(&old, &new)));
            }
            let Some(which) = whole(name, &a(3))? else { return Ok(Value::Null) };
            if which <= 0 || old.is_empty() {
                return Ok(Value::Null);
            }
            match s.match_indices(&old).nth(which as usize - 1) {
                Some((at, _)) => Ok(Value::str(format!("{}{new}{}", &s[..at], &s[at + old.len()..]))),
                None => Ok(Value::Null),
            }
        }
        "BASE" => {
            count(2, 3)?;
            let (Some(n), Some(radix)) = (whole(name, &a(0))?, whole(name, &a(1))?) else { return Ok(Value::Null) };
            let mut digits = radix_string(n as i32 as i64, if (2..=36).contains(&radix) { radix as u32 } else { 10 });
            if let Some(min) = if args.len() > 2 { whole(name, &a(2))? } else { None } {
                if min > 1 << 20 {
                    return Err(fail(name, "the minimum length is over a million characters"));
                }
                let missing = (min.max(0) as usize).saturating_sub(digits.len());
                digits.insert_str(0, &"0".repeat(missing));
            }
            Ok(Value::str(digits))
        }
        // Dates.
        "NOW" | "TODAY" => {
            count(0, 0)?;
            Ok(Value::Date(env.now))
        }
        "DATE" => {
            count(3, 3)?;
            let (Some(y), Some(m), Some(d)) = (whole(name, &a(0))?, whole(name, &a(1))?, whole(name, &a(2))?) else {
                return Ok(Value::Null);
            };
            let date = NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)
                .ok_or_else(|| fail(name, format!("DateTimeException: {y}-{m}-{d} is not a date")))?;
            Ok(Value::Date(date.and_time(NaiveTime::MIN)))
        }
        "TIME" => {
            count(3, 4)?;
            let (Some(h), Some(m), Some(s)) = (whole(name, &a(0))?, whole(name, &a(1))?, whole(name, &a(2))?) else {
                return Ok(Value::Null);
            };
            let time = NaiveTime::from_hms_opt(h as u32, m as u32, s as u32)
                .ok_or_else(|| fail(name, format!("DateTimeException: {h}:{m}:{s} is not a time")))?;
            let at = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default().and_time(time);
            // DateTimeFormatter.ofLocalizedTime(FormatStyle.LONG) when no pattern is given.
            let pattern = match text(&a(3)) {
                Some(p) => p,
                None if matches!(env.locale.tag, "es_DO" | "es_CO" | "es_VE" | "es_PH" | "es_PA" | "es_PR" | "es_US") => {
                    "h:mm:ss\u{202F}a z".to_string()
                }
                None if matches!(env.locale.tag, "es" | "es_ES" | "es_EA" | "es_IC" | "es_GQ") => "H:mm:ss z".to_string(),
                // es_MX, es_419, es_AR and the other Spanish-speaking countries.
                None if env.locale.tag.starts_with("es") => "HH:mm:ss z".to_string(),
                None if env.locale.tag == "fr_CA" => "HH 'h' mm 'min' ss 's' z".to_string(),
                None if matches!(env.locale.tag, "fr_DJ" | "fr_DZ" | "fr_MR" | "fr_SY" | "fr_TD" | "fr_TN" | "fr_VU") => {
                    "h:mm:ss\u{202F}a z".to_string()
                }
                None if env.locale.tag == "fr" || env.locale.tag.starts_with("fr_") => "HH:mm:ss z".to_string(),
                None if ["pt", "it", "de"].iter().any(|p| env.locale.tag.starts_with(p)) => "HH:mm:ss z".to_string(),
                None => "h:mm:ss\u{202F}a z".to_string(),
            };
            date_format(name, &at, &pattern, env.locale)
        }
        "DATEFORMAT" => {
            count(2, 2)?;
            match (a(0), text(&a(1))) {
                (Value::Date(d), Some(p)) => date_format(name, &d, &p, env.locale),
                (Value::Null, _) | (_, None) => Ok(Value::Null),
                (other, _) => Err(fail(name, format!("{} is not a Date", other.type_name()))),
            }
        }
        "DATEVALUE" => {
            count(1, 1)?;
            Ok(date(&a(0)).map(|d| Value::Long(d.and_utc().timestamp_millis())).unwrap_or(Value::Null))
        }
        "YEAR" | "MONTH" | "DAY" | "HOUR" | "MINUTE" | "SECOND" | "DAYSINMONTH" | "DAYSINYEAR" | "WEEKNUM" | "WEEKSINYEAR" => {
            count(1, 1)?;
            let Some(d) = date(&a(0)) else { return Ok(Value::Null) };
            Ok(Value::Int(match name {
                "YEAR" => d.year(),
                "MONTH" => d.month() as i32,
                "DAY" => d.day() as i32,
                "HOUR" => d.hour() as i32,
                "MINUTE" => d.minute() as i32,
                "SECOND" => d.second() as i32,
                "DAYSINMONTH" => days_in_month(d.year(), d.month()),
                "DAYSINYEAR" => {
                    if is_leap(d.year()) {
                        366
                    } else {
                        365
                    }
                }
                // ChronoField.ALIGNED_WEEK_OF_YEAR: days 1-7 of the year are week 1, whatever the
                // locale, as JasperReports' WEEKNUM reads it.
                "WEEKNUM" => d.ordinal0() as i32 / 7 + 1,
                // JasperReports' WEEKSINYEAR: 53 when the year starts on a Thursday, or on a
                // Wednesday in a leap year (as ISO weeks have it), 52 otherwise.
                _ => {
                    let jan1 = NaiveDate::from_ymd_opt(d.year(), 1, 1).unwrap_or_default().weekday();
                    if jan1 == Weekday::Thu || (jan1 == Weekday::Wed && is_leap(d.year())) {
                        53
                    } else {
                        52
                    }
                }
            }))
        }
        "ISLEAPYEAR" => {
            count(1, 1)?;
            Ok(date(&a(0)).map(|d| Value::Bool(is_leap(d.year()))).unwrap_or(Value::Null))
        }
        "WEEKDAY" => {
            count(1, 2)?;
            let Some(d) = date(&a(0)) else { return Ok(Value::Null) };
            let sunday_first = matches!(a(1), Value::Bool(true));
            Ok(Value::Int(if sunday_first {
                d.weekday().number_from_sunday() as i32
            } else {
                d.weekday().number_from_monday() as i32
            }))
        }
        "DAYS" | "WEEKS" | "MONTHS" | "YEARS" => {
            count(2, 2)?;
            let (Some(from), Some(to)) = (date(&a(0)), date(&a(1))) else { return Ok(Value::Null) };
            let (from, to) = (from.date(), to.date());
            let days = (to - from).num_days();
            Ok(Value::Int(match name {
                "DAYS" => days as i32,
                "WEEKS" => (days / 7) as i32,
                "MONTHS" => months_between(from, to),
                _ => months_between(from, to) / 12,
            }))
        }
        "EDATE" => {
            count(2, 2)?;
            let (Some(d), Some(n)) = (date(&a(0)), whole(name, &a(1))?) else { return Ok(Value::Null) };
            let moved = if n >= 0 {
                d.checked_add_months(Months::new(n as u32))
            } else {
                d.checked_sub_months(Months::new(n.unsigned_abs() as u32))
            };
            Ok(Value::Date(moved.ok_or_else(|| fail(name, "the date is out of range"))?))
        }
        "WORKDAY" => {
            count(2, 2)?;
            let (Some(mut d), Some(n)) = (date(&a(0)), whole(name, &a(1))?) else { return Ok(Value::Null) };
            // A working day is counted before moving on, as JasperReports does: one working day
            // from a Saturday is the Tuesday.
            let step = if n < 0 { -1 } else { 1 };
            let mut left = n.unsigned_abs();
            if left > 1_000_000 {
                return Err(fail(name, "too many working days"));
            }
            while left > 0 {
                if !is_weekend(&d) {
                    left -= 1;
                }
                d += Duration::days(step);
            }
            Ok(Value::Date(d))
        }
        "NETWORKDAYS" => {
            count(2, 2)?;
            let (Some(a), Some(b)) = (date(&a(0)), date(&a(1))) else { return Ok(Value::Null) };
            // The weekdays from the earlier date up to the later one, which is not counted.
            let (from, to) = if a <= b { (a.date(), b.date()) } else { (b.date(), a.date()) };
            let days = (to - from).num_days();
            let mut count = (days / 7) * 5;
            let mut d = from + Duration::days(days / 7 * 7);
            while d < to {
                if !matches!(d.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun) {
                    count += 1;
                }
                d += Duration::days(1);
            }
            Ok(Value::Int(count as i32))
        }
        "DATERANGE" => {
            count(1, 1)?;
            match a(0) {
                Value::Date(d) => {
                    Ok(Value::DateRange(Arc::new(DateRange { start: d, end: d, text: Value::Date(d).to_java_string() })))
                }
                Value::Str(s) => date_range_from_text(&s, env.now, env.locale),
                other => Err(fail(name, format!("cannot make a date range from {}", other.type_name()))),
            }
        }
        _ => Err(Error::eval(format!("unknown function {name}()"))),
    }
}

/// `DATERANGE("MONTH-1")`, `DATERANGE("2026-03-05")`: a span relative to `now` (DAY, WEEK,
/// MONTH, QUARTER, SEMI or YEAR, moved by a number of them), or a fixed date and time.
pub fn date_range_from_text(text: &str, now: NaiveDateTime, locale: &Locale) -> Result<Value> {
    let bad = || {
        Error::eval(format!(
            "DATERANGE: `{text}` is neither a relative range (DAY, WEEK-1, MONTH+2...) nor a date (yyyy-MM-dd [HH:mm:ss])"
        ))
    };
    let range =
        |start: NaiveDateTime, end: NaiveDateTime| Value::DateRange(Arc::new(DateRange { start, end, text: text.to_string() }));
    // Relative: the unit, in capitals, then an optional signed count.
    let units = ["DAY", "WEEK", "MONTH", "QUARTER", "SEMI", "YEAR"];
    if let Some(unit) = units.iter().find(|u| text.starts_with(**u)) {
        let rest = &text[unit.len()..];
        let shift: i64 = match rest {
            "" => 0,
            _ if (rest.starts_with('+') || rest.starts_with('-'))
                && rest.len() > 1
                && rest[1..].bytes().all(|b| b.is_ascii_digit()) =>
            {
                rest.parse().map_err(|_| bad())?
            }
            _ => return Err(bad()),
        };
        let today = now.date();
        let (start, end) = match *unit {
            "DAY" => {
                let d = today + Duration::days(shift);
                (d, d + Duration::days(1))
            }
            "WEEK" => {
                let back = (today.weekday().num_days_from_sunday() as i64 - first_day_of_week(locale) as i64).rem_euclid(7);
                let d = today - Duration::days(back) + Duration::weeks(shift);
                (d, d + Duration::weeks(1))
            }
            _ => {
                let months = match *unit {
                    "MONTH" => 1,
                    "QUARTER" => 3,
                    "SEMI" => 6,
                    _ => 12,
                };
                let first = (today.month0() as i64 / months) * months;
                let index = today.year() as i64 * 12 + first + shift * months;
                let d =
                    NaiveDate::from_ymd_opt(index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1, 1).ok_or_else(bad)?;
                (d, d.checked_add_months(Months::new(months as u32)).ok_or_else(bad)?)
            }
        };
        let start = start.and_time(NaiveTime::MIN);
        return Ok(range(start, end.and_time(NaiveTime::MIN) - Duration::milliseconds(1)));
    }
    // Fixed: yyyy-MM-dd HH:mm:ss, else yyyy-MM-dd; anything after it is ignored, as
    // SimpleDateFormat.parse ignores it.
    let fixed = parse_prefix(text).ok_or_else(bad)?;
    Ok(range(fixed, fixed))
}

/// `yyyy-MM-dd HH:mm:ss` or `yyyy-MM-dd` at the start of `text`.
fn parse_prefix(text: &str) -> Option<NaiveDateTime> {
    let b = text.as_bytes();
    let read = |pos: &mut usize| -> Option<u32> {
        let start = *pos;
        while *pos < b.len() && b[*pos].is_ascii_digit() && *pos - start < 9 {
            *pos += 1;
        }
        if *pos == start {
            None
        } else {
            text[start..*pos].parse().ok()
        }
    };
    let skip = |pos: &mut usize, c: u8| -> Option<()> { (b.get(*pos) == Some(&c)).then(|| *pos += 1) };
    let mut pos = 0;
    let y = read(&mut pos)?;
    skip(&mut pos, b'-')?;
    let m = read(&mut pos)?;
    skip(&mut pos, b'-')?;
    let d = read(&mut pos)?;
    let date = NaiveDate::from_ymd_opt(y as i32, m, d)?;
    let time = (|| {
        skip(&mut pos, b' ')?;
        let h = read(&mut pos)?;
        skip(&mut pos, b':')?;
        let min = read(&mut pos)?;
        skip(&mut pos, b':')?;
        let sec = read(&mut pos)?;
        NaiveTime::from_hms_opt(h, min, sec)
    })();
    Some(date.and_time(time.unwrap_or(NaiveTime::MIN)))
}

/// The day weeks start on (0 = Sunday) for `DATERANGE("WEEK")`, as
/// `Calendar.getInstance(locale).getFirstDayOfWeek()` says (JasperReports' RelativeDateRange).
fn first_day_of_week(locale: &Locale) -> u32 {
    match locale.tag {
        "es_ES" | "es_EA" | "es_IC" | "es_GQ" | "es_AR" | "es_EC" | "es_CL" | "es_UY" | "es_BO" | "es_CR" | "es_CU"
        | "es_419" => 1,
        "it_IT" | "it_CH" | "de_DE" | "de_AT" | "de_CH" => 1,
        // French without a country and in Canada start on Sunday, in Algeria, Djibouti and Syria
        // on Saturday, everywhere else on Monday.
        "fr_CA" => 0,
        "fr_DZ" | "fr_DJ" | "fr_SY" => 6,
        t if t.starts_with("fr_") => 1,
        _ => 0,
    }
}

/// `ChronoUnit.MONTHS.between`: whole months, toward zero.
fn months_between(from: NaiveDate, to: NaiveDate) -> i32 {
    let mut months = (to.year() - from.year()) * 12 + to.month() as i32 - from.month() as i32;
    if months > 0 && to.day() < from.day() {
        months -= 1;
    } else if months < 0 && to.day() > from.day() {
        months += 1;
    }
    months
}

fn is_weekend(d: &NaiveDateTime) -> bool {
    matches!(d.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun)
}

fn is_leap(year: i32) -> bool {
    NaiveDate::from_ymd_opt(year, 2, 29).is_some()
}

fn days_in_month(year: i32, month: u32) -> i32 {
    let next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
    match (next, NaiveDate::from_ymd_opt(year, month, 1)) {
        (Some(n), Some(f)) => (n - f).num_days() as i32,
        _ => 30,
    }
}

fn date_format(name: &str, d: &NaiveDateTime, pattern: &str, locale: &Locale) -> Result<Value> {
    format_date(pattern, d, locale).map(Value::str).ok_or_else(|| fail(name, format!("unsupported date pattern `{pattern}`")))
}

/// A date argument: a Date, or a Long of milliseconds since 1970. Anything else (a text, an
/// Integer) is no date, and the function gives null.
fn date(v: &Value) -> Option<NaiveDateTime> {
    match v {
        Value::Date(d) => Some(*d),
        Value::Long(ms) => chrono::DateTime::from_timestamp_millis(*ms).map(|d| d.naive_utc()),
        _ => None,
    }
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        other => Some(other.to_java_string()),
    }
}

fn text_fn(_name: &str, v: &Value, f: impl Fn(&str) -> Value) -> Result<Value> {
    Ok(text(v).map(|s| f(&s)).unwrap_or(Value::Null))
}

/// A numeric argument, or `None` for null.
fn number(name: &str, v: &Value) -> Result<Option<Value>> {
    match v {
        Value::Null => Ok(None),
        n if n.is_numeric() => Ok(Some(n.clone())),
        other => Err(fail(name, format!("{} is not a number", other.type_name()))),
    }
}

/// The numbers of a varargs function, or `None` when there are none or one is null.
fn numbers(name: &str, args: &[Value]) -> Result<Option<Vec<Value>>> {
    if args.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(args.len());
    for v in args {
        match number(name, v)? {
            Some(n) => out.push(n),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn whole(name: &str, v: &Value) -> Result<Option<i64>> {
    Ok(number(name, v)?.map(|n| n.as_i64().unwrap_or_else(|| f64_of(&n) as i64)))
}

fn f64_of(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// `Number.intValue()`.
fn int_value(v: &Value) -> i32 {
    match v {
        Value::Int(n) => *n,
        Value::Long(n) => *n as i32,
        other => f64_of(other) as i32,
    }
}

/// `Objects.equals`: the same class and the same value (1 and 1L differ).
fn strict_equals(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b) && a.java_equals(b)
}

/// `new BigDecimal(String.valueOf(n)).setScale(scale, mode)`.
fn round(name: &str, v: &Value, scale: i64) -> Result<Decimal> {
    let text = match v {
        Value::Double(f) => java_double_to_string(*f),
        Value::Float(f) => java_float_to_string(*f),
        other => other.to_java_string(),
    };
    let d = match v {
        Value::Decimal(d) => *d,
        _ => Decimal::from_str_exact(&text)
            .or_else(|_| Decimal::from_scientific(&text))
            .map_err(|_| fail(name, format!("cannot round {text}")))?,
    };
    let mode = match name {
        "ROUND_UP" => RoundingStrategy::AwayFromZero,
        "ROUND_DOWN" => RoundingStrategy::ToZero,
        "ROUND_CEILING" => RoundingStrategy::ToPositiveInfinity,
        "ROUND_FLOOR" => RoundingStrategy::ToNegativeInfinity,
        "ROUND_HALF_DOWN" => RoundingStrategy::MidpointTowardZero,
        "ROUND_HALF_EVEN" => RoundingStrategy::MidpointNearestEven,
        _ => RoundingStrategy::MidpointAwayFromZero,
    };
    if scale < 0 {
        // A negative scale rounds to tens, hundreds...; the result is kept with no decimals
        // (JasperReports prints it as 1.2E+3).
        let factor = Decimal::from_i128_with_scale(10i128.pow(scale.unsigned_abs().min(27) as u32), 0);
        let overflow = || fail(name, "ArithmeticException: the result has more than 28 digits");
        return (d.checked_div(factor).ok_or_else(overflow)?.round_dp_with_strategy(0, mode))
            .checked_mul(factor)
            .ok_or_else(overflow);
    }
    let scale = scale.min(28) as u32;
    let mut r = d.round_dp_with_strategy(scale, mode);
    r.rescale(scale);
    Ok(r)
}

/// `WordUtils.capitalizeFully`: each word (split at white space) with a capital first letter.
fn proper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_whitespace() {
            start = true;
            out.push(c);
        } else if start {
            out.extend(c.to_uppercase());
            start = false;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

fn utf16_len(s: &str) -> i32 {
    s.encode_utf16().count() as i32
}

/// `String.indexOf(needle, from)`, in characters.
fn index_of(haystack: &str, needle: &str, from: i64) -> i32 {
    let chars: Vec<char> = haystack.chars().collect();
    let from = from.max(0) as usize;
    if from > chars.len() {
        return if needle.is_empty() { chars.len() as i32 } else { -1 };
    }
    let rest: String = chars[from..].iter().collect();
    match rest.find(needle) {
        Some(b) => (from + rest[..b].chars().count()) as i32,
        None => -1,
    }
}

/// `Integer.toString(n, radix)`.
fn radix_string(n: i64, radix: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    let mut m = n.unsigned_abs();
    while m > 0 {
        digits.push(std::char::from_digit((m % radix as u64) as u32, radix).unwrap_or('0'));
        m /= radix as u64;
    }
    if n < 0 {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

/// `Double.valueOf(text)`: white space around it is allowed, and so are `d`/`f` suffixes.
fn java_float_text(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c <= ' ');
    let t = t.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(t);
    match t {
        "NaN" => Some(f64::NAN),
        "Infinity" | "+Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')) => t.parse().ok(),
        _ => None,
    }
}

/// A number in [0, 1), for RAND() (not for anything that needs to be unpredictable).
fn random_unit() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    (h.finish() >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_sorted() {
        assert!(NAMES.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn fixed_ranges_read_a_date_and_a_time() {
        let now = NaiveDate::from_ymd_opt(2026, 11, 23).unwrap().and_hms_opt(4, 0, 0).unwrap();
        let en = &crate::format::locale::EN_US;
        let at = |s: &str| match date_range_from_text(s, now, en).unwrap() {
            Value::DateRange(r) => (r.start.to_string(), r.end.to_string()),
            _ => unreachable!(),
        };
        assert_eq!(at("2017-05-01").0, "2017-05-01 00:00:00");
        assert_eq!(at("2017-05-01 14:25:48.123").0, "2017-05-01 14:25:48");
        assert_eq!(at("2017-05-01 14:25").0, "2017-05-01 00:00:00");
        assert_eq!(at("2017-05-01+3").0, "2017-05-01 00:00:00");
        assert_eq!(at("WEEK"), ("2026-11-22 00:00:00".into(), "2026-11-28 23:59:59.999".into()));
        assert_eq!(at("SEMI-3"), ("2025-01-01 00:00:00".into(), "2025-06-30 23:59:59.999".into()));
        assert_eq!(at("MONTH+13").0, "2027-12-01 00:00:00");
        for bad in ["2017-05", "2017", "05/01/2017", "day", "DAY - 1", " DAY "] {
            assert!(date_range_from_text(bad, now, en).is_err(), "{bad}");
        }
    }
}
