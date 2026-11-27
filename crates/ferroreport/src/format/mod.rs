//! Value formatting: Java `toString`, `DecimalFormat` and `SimpleDateFormat`.

pub mod date;
pub mod locale;
mod locale_names;
pub mod message;
pub mod number;

pub use locale::Locale;
pub use number::NumberPattern;

use crate::value::Value;

/// Formats a value for display in a text field, applying `pattern` when given.
/// `DateFormat.getDateTimeInstance(SHORT, SHORT)` on Java 21.
pub fn short_date_time(locale: &Locale) -> &'static str {
    match locale.tag {
        "fr_BE" => "d/MM/yy HH:mm",
        "fr_CA" => "y-MM-dd HH 'h' mm",
        "fr_CH" => "dd.MM.yy HH:mm",
        "fr_ML" => "dd/MM/y, HH:mm",
        "fr_DJ" | "fr_DZ" | "fr_MR" | "fr_SY" | "fr_TD" | "fr_TN" | "fr_VU" => "dd/MM/y h:mm\u{202F}a",
        t if t == "fr" || t.starts_with("fr_") => "dd/MM/y HH:mm",
        "pt" | "pt_BR" => "dd/MM/y HH:mm",
        "pt_PT" => "dd/MM/yy, HH:mm",
        "it" | "it_IT" => "dd/MM/yy, HH:mm",
        "it_CH" => "dd.MM.yy, HH:mm",
        t if t.starts_with("de") => "dd.MM.yy, HH:mm",
        "es_DO" | "es_VE" | "es_PH" => "d/M/yy, h:mm\u{202F}a",
        "es_CO" => "d/MM/yy, h:mm\u{202F}a",
        "es_PA" | "es_PR" => "MM/dd/yy, h:mm\u{202F}a",
        "es_US" => "d/M/y, h:mm\u{202F}a",
        "es_MX" => "dd/MM/yy, HH:mm",
        "es_CL" => "dd-MM-yy, HH:mm",
        "es_GT" | "es_PE" => "d/MM/yy, HH:mm",
        "es" | "es_ES" | "es_EA" | "es_IC" | "es_GQ" => "d/M/yy, H:mm",
        // es_419, es_AR and the other Spanish-speaking countries.
        t if t.starts_with("es") => "d/M/yy, HH:mm",
        _ => "M/d/yy, h:mm\u{202F}a",
    }
}

pub fn format_value(value: &Value, pattern: Option<&str>, locale: &Locale) -> String {
    match (value, pattern) {
        (Value::Date(dt), Some(p)) if !p.is_empty() => date::format_date(p, dt, locale).unwrap_or_else(|| value.to_java_string()),
        // A date with no pattern is written in the locale's short date and time, as
        // JasperReports does (DateFormat.SHORT, SHORT).
        (Value::Date(dt), _) => date::format_date(short_date_time(locale), dt, locale).unwrap_or_else(|| value.to_java_string()),
        (v, Some(p)) if !p.is_empty() && v.is_numeric() => {
            NumberPattern::parse(p, locale).format(v, locale).unwrap_or_else(|| v.to_java_string())
        }
        (v, _) => v.to_java_string(),
    }
}
