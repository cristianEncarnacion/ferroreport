//! Locale data needed by the number and date formatters (a small CLDR subset).

/// Symbols and names for one locale.
#[derive(Debug, Clone)]
pub struct Locale {
    pub tag: &'static str,
    pub decimal_separator: char,
    pub grouping_separator: char,
    pub currency_symbol: &'static str,
    pub months: [&'static str; 12],
    pub short_months: [&'static str; 12],
    /// Sunday first, like `java.util.Calendar`.
    pub weekdays: [&'static str; 7],
    pub short_weekdays: [&'static str; 7],
    pub am_pm: [&'static str; 2],
}

const EN_MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const EN_SHORT_MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const EN_WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const EN_SHORT_WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

const ES_MONTHS: [&str; 12] =
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
const ES_SHORT_MONTHS: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];
const ES_WEEKDAYS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];
const ES_SHORT_WEEKDAYS: [&str; 7] = ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"];

pub const EN_US: Locale = Locale {
    tag: "en_US",
    decimal_separator: '.',
    grouping_separator: ',',
    currency_symbol: "$",
    months: EN_MONTHS,
    short_months: EN_SHORT_MONTHS,
    weekdays: EN_WEEKDAYS,
    short_weekdays: EN_SHORT_WEEKDAYS,
    am_pm: ["AM", "PM"],
};

/// Spanish with the Latin-American number symbols used in DO, MX and US: a point for decimals
/// and a comma between groups.
pub const ES_DO: Locale = Locale {
    tag: "es_DO",
    decimal_separator: '.',
    grouping_separator: ',',
    currency_symbol: "RD$",
    months: ES_MONTHS,
    short_months: ES_SHORT_MONTHS,
    weekdays: ES_WEEKDAYS,
    short_weekdays: ES_SHORT_WEEKDAYS,
    // A no-break space (U+00A0), as Java 21.0.13's CLDR data has it for es, es_DO, es_MX and es_ES.
    am_pm: ["a.\u{A0}m.", "p.\u{A0}m."],
};

pub const ES_MX: Locale = Locale { tag: "es_MX", currency_symbol: "$", ..ES_DO };

/// Spanish (Spain): comma decimal separator, dot grouping.
pub const ES_ES: Locale =
    Locale { tag: "es_ES", decimal_separator: ',', grouping_separator: '.', currency_symbol: "€", ..ES_DO };

/// Spanish without a country, as Java 21 has it: Spain's symbols with the generic currency sign
/// `¤`, as no country says which currency, and weeks starting on Sunday.
pub const ES: Locale = Locale { tag: "es", currency_symbol: "\u{A4}", ..ES_ES };

/// Spanish (Ceuta and Melilla): Spain's symbols with the generic currency sign `¤`, as Java 21
/// has it.
pub const ES_EA: Locale = Locale { tag: "es_EA", currency_symbol: "\u{A4}", ..ES_ES };
/// Spanish (Canary Islands): as es_EA.
pub const ES_IC: Locale = Locale { tag: "es_IC", ..ES_EA };
/// Spanish (Equatorial Guinea): Spain's symbols and clock with the CFA franc `FCFA`.
pub const ES_GQ: Locale = Locale { tag: "es_GQ", currency_symbol: "FCFA", ..ES_ES };

/// Spanish for Latin America, and the Spanish tags Java does not have: es_DO's number symbols
/// with the generic currency sign `¤`, and the 24-hour clock Java 21 gives it in the short date
/// and time and in `TIME()`.
pub const ES_419: Locale = Locale { tag: "es_419", currency_symbol: "\u{A4}", ..ES_DO };

const ES_SETIEMBRE_MONTHS: [&str; 12] =
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "setiembre", "octubre", "noviembre", "diciembre"];
const ES_SET_SHORT_MONTHS: [&str; 12] =
    ["ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "set.", "oct.", "nov.", "dic."];
const ES_DOTTED_SHORT_MONTHS: [&str; 12] =
    ["ene.", "feb.", "mar.", "abr.", "may.", "jun.", "jul.", "ago.", "sept.", "oct.", "nov.", "dic."];

/// Spanish (Argentina): Spain's separators (a comma for decimals, a point between groups) with
/// `$`, a 24-hour clock with two-digit hours (`d/M/yy, HH:mm`, `TIME()` `HH:mm:ss z`) and weeks
/// starting on Monday, as in the countries below.
pub const ES_AR: Locale = Locale { tag: "es_AR", currency_symbol: "$", ..ES_ES };
/// Spanish (Ecuador): as es_AR.
pub const ES_EC: Locale = Locale { tag: "es_EC", ..ES_AR };
/// Spanish (Chile): as es_AR, with `dd-MM-yy, HH:mm` for the short date and time.
pub const ES_CL: Locale = Locale { tag: "es_CL", ..ES_AR };
/// Spanish (Uruguay): as es_AR, with "setiembre" and short month names with a point (`set.`).
pub const ES_UY: Locale = Locale { tag: "es_UY", months: ES_SETIEMBRE_MONTHS, short_months: ES_SET_SHORT_MONTHS, ..ES_AR };
/// Spanish (Bolivia): as es_AR, with the boliviano `Bs`.
pub const ES_BO: Locale = Locale { tag: "es_BO", currency_symbol: "Bs", ..ES_AR };
/// Spanish (Costa Rica): as es_AR, with a no-break space (U+00A0) between groups and the colón `₡`.
pub const ES_CR: Locale = Locale { tag: "es_CR", grouping_separator: '\u{A0}', currency_symbol: "\u{20A1}", ..ES_AR };
/// Spanish (Paraguay): as es_AR, with the guaraní `Gs.`, short month names with a point and
/// weeks starting on Sunday.
pub const ES_PY: Locale = Locale { tag: "es_PY", currency_symbol: "Gs.", short_months: ES_DOTTED_SHORT_MONTHS, ..ES_AR };

/// Spanish (Cuba): es_419's separators and clock with `$`, weeks starting on Monday.
pub const ES_CU: Locale = Locale { tag: "es_CU", currency_symbol: "$", ..ES_419 };
/// Spanish (Honduras): es_419's separators and clock with the lempira `L`, weeks starting on
/// Sunday, as in the countries below.
pub const ES_HN: Locale = Locale { tag: "es_HN", currency_symbol: "L", ..ES_419 };
/// Spanish (Nicaragua): as es_HN, with the córdoba `C$`.
pub const ES_NI: Locale = Locale { tag: "es_NI", currency_symbol: "C$", ..ES_419 };
/// Spanish (El Salvador): as es_HN, with `C`, the sign Java 21 has for the country's currency.
pub const ES_SV: Locale = Locale { tag: "es_SV", currency_symbol: "C", ..ES_419 };
/// Spanish (Belize): as es_HN, with `$`.
pub const ES_BZ: Locale = Locale { tag: "es_BZ", currency_symbol: "$", ..ES_419 };
/// Spanish (Brazil): as es_HN, with the real `R$`.
pub const ES_BR: Locale = Locale { tag: "es_BR", currency_symbol: "R$", ..ES_419 };
/// Spanish (Guatemala): as es_HN, with the quetzal `Q` and `d/MM/yy, HH:mm` for the short date
/// and time.
pub const ES_GT: Locale = Locale { tag: "es_GT", currency_symbol: "Q", ..ES_419 };
/// Spanish (Peru): as es_GT, with the sol `S/`, "setiembre" and short month names with a point
/// (`set.`).
pub const ES_PE: Locale =
    Locale { tag: "es_PE", currency_symbol: "S/", months: ES_SETIEMBRE_MONTHS, short_months: ES_SET_SHORT_MONTHS, ..ES_419 };

/// Spanish (Colombia): Spain's separators with `$`, a.m./p.m. with a narrow no-break space
/// (U+202F), and a 12-hour clock (`d/MM/yy, h:mm a`, `TIME()` `h:mm:ss a z`) with weeks
/// starting on Sunday, as in the countries below and es_DO.
pub const ES_CO: Locale = Locale { tag: "es_CO", currency_symbol: "$", am_pm: ["a.\u{202F}m.", "p.\u{202F}m."], ..ES_ES };
/// Spanish (Venezuela): Spain's separators with the bolívar `Bs.S`, short month names with a point
/// and es_DO's clock (`d/M/yy, h:mm a`).
pub const ES_VE: Locale = Locale { tag: "es_VE", currency_symbol: "Bs.S", short_months: ES_DOTTED_SHORT_MONTHS, ..ES_ES };
/// Spanish (Philippines): Spain's separators with the peso `₱` and es_DO's clock.
pub const ES_PH: Locale = Locale { tag: "es_PH", currency_symbol: "\u{20B1}", ..ES_ES };
/// Spanish (Panama): es_DO's separators with the balboa `B/.`, a 12-hour clock and the month
/// first in the short date (`MM/dd/yy, h:mm a`).
pub const ES_PA: Locale = Locale { tag: "es_PA", currency_symbol: "B/.", ..ES_DO };
/// Spanish (Puerto Rico): as es_PA, with `$`.
pub const ES_PR: Locale = Locale { tag: "es_PR", currency_symbol: "$", ..ES_DO };
/// Spanish (United States): es_DO's separators with `$`, a 12-hour clock and a four-digit year in
/// the short date (`d/M/y, h:mm a`).
pub const ES_US: Locale = Locale { tag: "es_US", currency_symbol: "$", ..ES_DO };

const FR_MONTHS: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const FR_SHORT_MONTHS: [&str; 12] =
    ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
const FR_WEEKDAYS: [&str; 7] = ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"];
const FR_SHORT_WEEKDAYS: [&str; 7] = ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."];

/// French without a country, as Java 21 has it: a narrow no-break space between groups, and
/// the generic currency sign `¤`, as no country says which currency.
pub const FR: Locale = Locale {
    tag: "fr",
    decimal_separator: ',',
    grouping_separator: '\u{202F}',
    currency_symbol: "\u{A4}",
    months: FR_MONTHS,
    short_months: FR_SHORT_MONTHS,
    weekdays: FR_WEEKDAYS,
    short_weekdays: FR_SHORT_WEEKDAYS,
    am_pm: ["AM", "PM"],
};

pub const FR_FR: Locale = Locale { tag: "fr_FR", currency_symbol: "€", ..FR };

pub const FR_BE: Locale = Locale { tag: "fr_BE", currency_symbol: "€", ..FR };

/// French (Canada): a no-break space between groups, `juill.` for July and a.m./p.m.
pub const FR_CA: Locale = Locale {
    tag: "fr_CA",
    grouping_separator: '\u{A0}',
    currency_symbol: "$",
    short_months: ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juill.", "août", "sept.", "oct.", "nov.", "déc."],
    am_pm: ["a.m.", "p.m."],
    ..FR
};

/// French (Switzerland): amounts in a currency pattern take a point as their decimal
/// separator (see [`Locale::monetary_decimal_separator`]).
pub const FR_CH: Locale = Locale { tag: "fr_CH", currency_symbol: "CHF", ..FR };

/// French (Luxembourg): a point between groups.
pub const FR_LU: Locale = Locale { tag: "fr_LU", grouping_separator: '.', ..FR_FR };

/// French (Morocco): a point between groups, the dirham `MAD`, its own short month names
/// (`jan.`, `fév.`, `jui.` for June) and a.m./p.m.
pub const FR_MA: Locale = Locale {
    tag: "fr_MA",
    grouping_separator: '.',
    currency_symbol: "MAD",
    short_months: ["jan.", "fév.", "mar.", "avr.", "mai", "jui.", "juil.", "août", "sept.", "oct.", "nov.", "déc."],
    am_pm: ["a.m.", "p.m."],
    ..FR
};

/// French (Senegal), and the other West African CFA franc countries below: `F CFA`, with a
/// narrow no-break space (U+202F).
pub const FR_SN: Locale = Locale { tag: "fr_SN", currency_symbol: "F\u{202F}CFA", ..FR };
/// French (Côte d'Ivoire): as fr_SN.
pub const FR_CI: Locale = Locale { tag: "fr_CI", ..FR_SN };
/// French (Burkina Faso): as fr_SN.
pub const FR_BF: Locale = Locale { tag: "fr_BF", ..FR_SN };
/// French (Benin): as fr_SN.
pub const FR_BJ: Locale = Locale { tag: "fr_BJ", ..FR_SN };
/// French (Niger): as fr_SN.
pub const FR_NE: Locale = Locale { tag: "fr_NE", ..FR_SN };
/// French (Togo): as fr_SN.
pub const FR_TG: Locale = Locale { tag: "fr_TG", ..FR_SN };
/// French (Mali): as fr_SN, with a comma after the date in the short date and time.
pub const FR_ML: Locale = Locale { tag: "fr_ML", ..FR_SN };

/// French (Gabon), and the other Central African CFA franc countries below: `FCFA`.
pub const FR_GA: Locale = Locale { tag: "fr_GA", currency_symbol: "FCFA", ..FR };
/// French (Congo): as fr_GA.
pub const FR_CG: Locale = Locale { tag: "fr_CG", ..FR_GA };
/// French (Central African Republic): as fr_GA.
pub const FR_CF: Locale = Locale { tag: "fr_CF", ..FR_GA };
/// French (Equatorial Guinea): as fr_GA.
pub const FR_GQ: Locale = Locale { tag: "fr_GQ", ..FR_GA };
/// French (Cameroon): as fr_GA, with "matin" / "soir" for AM/PM.
pub const FR_CM: Locale = Locale { tag: "fr_CM", am_pm: ["matin", "soir"], ..FR_GA };
/// French (Chad): as fr_GA, with a 12-hour clock in the short date and time and in `TIME()`.
pub const FR_TD: Locale = Locale { tag: "fr_TD", ..FR_GA };

/// French (New Caledonia), and French Polynesia and Wallis and Futuna: the CFP franc `FCFP`.
pub const FR_NC: Locale = Locale { tag: "fr_NC", currency_symbol: "FCFP", ..FR };
/// French (French Polynesia): as fr_NC.
pub const FR_PF: Locale = Locale { tag: "fr_PF", ..FR_NC };
/// French (Wallis and Futuna): as fr_NC.
pub const FR_WF: Locale = Locale { tag: "fr_WF", ..FR_NC };

/// French (Burundi): the Burundian franc `FBu`.
pub const FR_BI: Locale = Locale { tag: "fr_BI", currency_symbol: "FBu", ..FR };
/// French (Congo-Kinshasa): the Congolese franc `FC`.
pub const FR_CD: Locale = Locale { tag: "fr_CD", currency_symbol: "FC", ..FR };
/// French (Guinea): the Guinean franc `FG`.
pub const FR_GN: Locale = Locale { tag: "fr_GN", currency_symbol: "FG", ..FR };
/// French (Haiti): the gourde `G`.
pub const FR_HT: Locale = Locale { tag: "fr_HT", currency_symbol: "G", ..FR };
/// French (Comoros): the Comorian franc `CF`.
pub const FR_KM: Locale = Locale { tag: "fr_KM", currency_symbol: "CF", ..FR };
/// French (Madagascar): the ariary `Ar`.
pub const FR_MG: Locale = Locale { tag: "fr_MG", currency_symbol: "Ar", ..FR };
/// French (Mauritius): the Mauritian rupee `Rs`.
pub const FR_MU: Locale = Locale { tag: "fr_MU", currency_symbol: "Rs", ..FR };
/// French (Rwanda): the Rwandan franc `RF`.
pub const FR_RW: Locale = Locale { tag: "fr_RW", currency_symbol: "RF", ..FR };
/// French (Seychelles): the Seychellois rupee `SR`.
pub const FR_SC: Locale = Locale { tag: "fr_SC", currency_symbol: "SR", ..FR };

/// French (Algeria): the dinar `DA`, a 12-hour clock in the short date and time and in `TIME()`,
/// and weeks starting on Saturday in `DATERANGE("WEEK")`.
pub const FR_DZ: Locale = Locale { tag: "fr_DZ", currency_symbol: "DA", ..FR };
/// French (Djibouti): the Djiboutian franc `Fdj`, with fr_DZ's clock and weeks.
pub const FR_DJ: Locale = Locale { tag: "fr_DJ", currency_symbol: "Fdj", ..FR };
/// French (Syria): the Syrian pound `LS`, with fr_DZ's clock and weeks.
pub const FR_SY: Locale = Locale { tag: "fr_SY", currency_symbol: "LS", ..FR };
/// French (Mauritania): the ouguiya `UM`, with a 12-hour clock as in fr_DZ.
pub const FR_MR: Locale = Locale { tag: "fr_MR", currency_symbol: "UM", ..FR };
/// French (Tunisia): the dinar `DT`, with a 12-hour clock as in fr_DZ.
pub const FR_TN: Locale = Locale { tag: "fr_TN", currency_symbol: "DT", ..FR };
/// French (Vanuatu): the vatu `VT`, with a 12-hour clock as in fr_DZ.
pub const FR_VU: Locale = Locale { tag: "fr_VU", currency_symbol: "VT", ..FR };

const PT_MONTHS: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
const PT_SHORT_MONTHS: [&str; 12] =
    ["jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.", "dez."];
const PT_WEEKDAYS: [&str; 7] =
    ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"];

/// Portuguese without a country, as Java 21 has it: Brazil's symbols with the generic currency
/// sign `¤`.
pub const PT: Locale = Locale {
    tag: "pt",
    decimal_separator: ',',
    grouping_separator: '.',
    currency_symbol: "\u{A4}",
    months: PT_MONTHS,
    short_months: PT_SHORT_MONTHS,
    weekdays: PT_WEEKDAYS,
    short_weekdays: ["dom.", "seg.", "ter.", "qua.", "qui.", "sex.", "sáb."],
    am_pm: ["AM", "PM"],
};

pub const PT_BR: Locale = Locale { tag: "pt_BR", currency_symbol: "R$", ..PT };

/// Portuguese (Portugal): a no-break space between groups, short day names without the
/// "-feira", and "da manhã" / "da tarde" for AM/PM.
pub const PT_PT: Locale = Locale {
    tag: "pt_PT",
    grouping_separator: '\u{A0}',
    currency_symbol: "€",
    short_weekdays: ["domingo", "segunda", "terça", "quarta", "quinta", "sexta", "sábado"],
    am_pm: ["da manhã", "da tarde"],
    ..PT
};

/// Italian without a country, as Java 21 has it: Italy's symbols with the generic currency sign
/// `¤`.
pub const IT: Locale = Locale {
    tag: "it",
    decimal_separator: ',',
    grouping_separator: '.',
    currency_symbol: "\u{A4}",
    months: [
        "gennaio",
        "febbraio",
        "marzo",
        "aprile",
        "maggio",
        "giugno",
        "luglio",
        "agosto",
        "settembre",
        "ottobre",
        "novembre",
        "dicembre",
    ],
    short_months: ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"],
    weekdays: ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"],
    short_weekdays: ["dom", "lun", "mar", "mer", "gio", "ven", "sab"],
    am_pm: ["AM", "PM"],
};

pub const IT_IT: Locale = Locale { tag: "it_IT", currency_symbol: "€", ..IT };

/// Italian (Switzerland): a point for decimals and a right single quote (U+2019) between groups.
pub const IT_CH: Locale =
    Locale { tag: "it_CH", decimal_separator: '.', grouping_separator: '\u{2019}', currency_symbol: "SFr.", ..IT };

const DE_MONTHS: [&str; 12] =
    ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];

/// German without a country, as Java 21 has it: Germany's symbols with the generic currency
/// sign `¤`.
pub const DE: Locale = Locale {
    tag: "de",
    decimal_separator: ',',
    grouping_separator: '.',
    currency_symbol: "\u{A4}",
    months: DE_MONTHS,
    short_months: ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."],
    weekdays: ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
    short_weekdays: ["So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."],
    am_pm: ["AM", "PM"],
};

pub const DE_DE: Locale = Locale { tag: "de_DE", currency_symbol: "€", ..DE };

/// German (Austria): "Jänner" for January, a no-break space between groups (a point in amounts
/// with a currency sign, see [`Locale::monetary_grouping_separator`]).
pub const DE_AT: Locale = Locale {
    tag: "de_AT",
    grouping_separator: '\u{A0}',
    currency_symbol: "€",
    months: [
        "Jänner",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
    short_months: ["Jän.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."],
    ..DE
};

/// German (Switzerland): a point for decimals and a right single quote (U+2019) between groups.
pub const DE_CH: Locale =
    Locale { tag: "de_CH", decimal_separator: '.', grouping_separator: '\u{2019}', currency_symbol: "CHF", ..DE };

impl Locale {
    /// The decimal separator of a pattern with a currency sign (`¤`), Java's
    /// `DecimalFormatSymbols.getMonetaryDecimalSeparator`: a point in French Switzerland, the
    /// decimal separator everywhere else.
    pub(crate) fn monetary_decimal_separator(&self) -> char {
        match self.tag {
            "fr_CH" => '.',
            _ => self.decimal_separator,
        }
    }

    /// The grouping separator of a pattern with a currency sign (`¤`), Java's
    /// `DecimalFormatSymbols.getMonetaryGroupingSeparator`: a point in Austria, where other
    /// numbers group with a no-break space, the grouping separator everywhere else.
    pub(crate) fn monetary_grouping_separator(&self) -> char {
        match self.tag {
            "de_AT" => '.',
            _ => self.grouping_separator,
        }
    }

    /// Resolves a Java-style locale tag (`en_US`, `es-DO`, `es`). Unknown tags fall back to `en_US`.
    pub fn from_tag(tag: &str) -> &'static Locale {
        let norm = tag.replace('-', "_");
        match norm.as_str() {
            "es_DO" => &ES_DO,
            "es_MX" => &ES_MX,
            "es" => &ES,
            "es_ES" => &ES_ES,
            "es_EA" => &ES_EA,
            "es_IC" => &ES_IC,
            "es_GQ" => &ES_GQ,
            "es_AR" => &ES_AR,
            "es_EC" => &ES_EC,
            "es_CL" => &ES_CL,
            "es_UY" => &ES_UY,
            "es_BO" => &ES_BO,
            "es_CR" => &ES_CR,
            "es_PY" => &ES_PY,
            "es_CU" => &ES_CU,
            "es_HN" => &ES_HN,
            "es_NI" => &ES_NI,
            "es_SV" => &ES_SV,
            "es_BZ" => &ES_BZ,
            "es_BR" => &ES_BR,
            "es_GT" => &ES_GT,
            "es_PE" => &ES_PE,
            "es_CO" => &ES_CO,
            "es_VE" => &ES_VE,
            "es_PH" => &ES_PH,
            "es_PA" => &ES_PA,
            "es_PR" => &ES_PR,
            "es_US" => &ES_US,
            _ if norm == "es_419" || norm.starts_with("es_") => &ES_419,
            "fr" => &FR,
            "fr_BE" => &FR_BE,
            "fr_CA" => &FR_CA,
            "fr_CH" => &FR_CH,
            "fr_LU" => &FR_LU,
            "fr_MA" => &FR_MA,
            "fr_SN" => &FR_SN,
            "fr_CI" => &FR_CI,
            "fr_BF" => &FR_BF,
            "fr_BJ" => &FR_BJ,
            "fr_NE" => &FR_NE,
            "fr_TG" => &FR_TG,
            "fr_ML" => &FR_ML,
            "fr_GA" => &FR_GA,
            "fr_CG" => &FR_CG,
            "fr_CF" => &FR_CF,
            "fr_GQ" => &FR_GQ,
            "fr_CM" => &FR_CM,
            "fr_TD" => &FR_TD,
            "fr_NC" => &FR_NC,
            "fr_PF" => &FR_PF,
            "fr_WF" => &FR_WF,
            "fr_BI" => &FR_BI,
            "fr_CD" => &FR_CD,
            "fr_GN" => &FR_GN,
            "fr_HT" => &FR_HT,
            "fr_KM" => &FR_KM,
            "fr_MG" => &FR_MG,
            "fr_MU" => &FR_MU,
            "fr_RW" => &FR_RW,
            "fr_SC" => &FR_SC,
            "fr_DZ" => &FR_DZ,
            "fr_DJ" => &FR_DJ,
            "fr_SY" => &FR_SY,
            "fr_MR" => &FR_MR,
            "fr_TN" => &FR_TN,
            "fr_VU" => &FR_VU,
            // France's overseas departments and Monaco write numbers and dates as France does.
            _ if norm == "fr_FR" || norm.starts_with("fr_") => &FR_FR,
            "pt" => &PT,
            "pt_BR" => &PT_BR,
            // Portugal's conventions are those of the other Portuguese-speaking countries.
            _ if norm == "pt_PT" || norm.starts_with("pt_") => &PT_PT,
            "it" => &IT,
            "it_CH" => &IT_CH,
            _ if norm == "it_IT" || norm.starts_with("it_") => &IT_IT,
            "de" => &DE,
            "de_AT" => &DE_AT,
            // Liechtenstein writes numbers as Switzerland does.
            "de_CH" | "de_LI" => &DE_CH,
            _ if norm == "de_DE" || norm.starts_with("de_") => &DE_DE,
            _ => &EN_US,
        }
    }
}

/// The language and country of a Java locale tag (`es_DO`, `es-DO`, `en`, `es_419`), or None
/// when `tag` is not one.
pub(crate) fn split_tag(tag: &str) -> Option<(String, String)> {
    let mut parts = tag.split(['_', '-']);
    let language = parts.next()?;
    let country = parts.next().unwrap_or("");
    let language_ok = (2..=3).contains(&language.len()) && language.chars().all(|c| c.is_ascii_alphabetic());
    let country_ok = country.is_empty()
        || (country.len() == 2 && country.chars().all(|c| c.is_ascii_alphabetic()))
        || (country.len() == 3 && country.chars().all(|c| c.is_ascii_digit()));
    (language_ok && country_ok && parts.next().is_none()).then(|| (language.to_ascii_lowercase(), country.to_ascii_uppercase()))
}

/// `Locale.getDisplayLanguage(in)`: the name of `tag`'s language in the language of `display`
/// (English, Spanish, or the language itself; English for others).
pub(crate) fn display_language(tag: &str, display: &str) -> String {
    let Some((language, _)) = split_tag(tag) else { return String::new() };
    let shown = split_tag(display).map(|d| d.0).unwrap_or_default();
    match super::locale_names::LANGUAGES.iter().find(|l| l.0 == language) {
        Some(l) if shown == language => l.3.to_string(),
        Some(l) if shown == "es" => l.2.to_string(),
        Some(l) => l.1.to_string(),
        None => language,
    }
}

/// `Locale.getDisplayCountry(in)`: the name of `tag`'s country in the language of `display`.
pub(crate) fn display_country(tag: &str, display: &str) -> String {
    let Some((language, country)) = split_tag(tag) else { return String::new() };
    if country.is_empty() {
        return country;
    }
    let shown = split_tag(display).map(|d| d.0).unwrap_or_default();
    if shown == language {
        let own = format!("{language}_{country}");
        if let Some(c) = super::locale_names::OWN_COUNTRIES.iter().find(|c| c.0 == own) {
            return c.1.to_string();
        }
    }
    match super::locale_names::COUNTRIES.iter().find(|c| c.0 == country) {
        Some(c) if shown == "es" => c.2.to_string(),
        Some(c) => c.1.to_string(),
        None => country,
    }
}

/// `Locale.getDisplayName(in)`: `language (country)`, as Java writes it.
pub(crate) fn display_name(tag: &str, display: &str) -> String {
    let language = display_language(tag, display);
    let country = display_country(tag, display);
    match (language.is_empty(), country.is_empty()) {
        (_, true) => language,
        (true, false) => country,
        (false, false) => format!("{language} ({country})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Java 21 prints for French (`DecimalFormat`, `SimpleDateFormat` and the short date
    /// and time of `DateFormat.getDateTimeInstance(SHORT, SHORT)`), for -1234567.5 and
    /// 5 July 2026 14:07:09.
    #[test]
    fn french_is_javas() {
        use crate::format::format_value;
        use crate::value::Value;
        let when = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap().and_hms_opt(14, 7, 9).unwrap();
        let cases = [
            (
                "fr",
                [
                    "-1\u{202F}234\u{202F}567,50",
                    "-\u{A4} 1\u{202F}234\u{202F}567,50",
                    "-1\u{202F}234\u{202F}567,50 \u{A4}",
                    "dimanche 5 juillet 2026",
                    "dim. 5 juil. 26",
                    "2:07 PM",
                    "05/07/2026 14:07",
                ],
            ),
            (
                "fr_FR",
                [
                    "-1\u{202F}234\u{202F}567,50",
                    "-\u{20AC} 1\u{202F}234\u{202F}567,50",
                    "-1\u{202F}234\u{202F}567,50 \u{20AC}",
                    "dimanche 5 juillet 2026",
                    "dim. 5 juil. 26",
                    "2:07 PM",
                    "05/07/2026 14:07",
                ],
            ),
            (
                "fr_BE",
                [
                    "-1\u{202F}234\u{202F}567,50",
                    "-\u{20AC} 1\u{202F}234\u{202F}567,50",
                    "-1\u{202F}234\u{202F}567,50 \u{20AC}",
                    "dimanche 5 juillet 2026",
                    "dim. 5 juil. 26",
                    "2:07 PM",
                    "5/07/26 14:07",
                ],
            ),
            (
                "fr_CA",
                [
                    "-1\u{A0}234\u{A0}567,50",
                    "-$ 1\u{A0}234\u{A0}567,50",
                    "-1\u{A0}234\u{A0}567,50 $",
                    "dimanche 5 juillet 2026",
                    "dim. 5 juill. 26",
                    "2:07 p.m.",
                    "2026-07-05 14 h 07",
                ],
            ),
            (
                "fr_CH",
                [
                    "-1\u{202F}234\u{202F}567,50",
                    "-CHF 1\u{202F}234\u{202F}567.50",
                    "-1\u{202F}234\u{202F}567.50 CHF",
                    "dimanche 5 juillet 2026",
                    "dim. 5 juil. 26",
                    "2:07 PM",
                    "05.07.26 14:07",
                ],
            ),
        ];
        let patterns =
            ["#,##0.00", "\u{A4} #,##0.00", "#,##0.00 \u{A4};-#,##0.00 \u{A4}", "EEEE d MMMM yyyy", "EEE d MMM yy", "h:mm a"];
        for (tag, expected) in cases {
            for alias in [tag.to_string(), tag.replace('_', "-")] {
                let locale = Locale::from_tag(&alias);
                assert_eq!(locale.tag, tag);
                for (i, pattern) in patterns.iter().enumerate() {
                    let value = if i < 3 { Value::Double(-1234567.5) } else { Value::Date(when) };
                    assert_eq!(format_value(&value, Some(pattern), locale), expected[i], "{tag} {pattern}");
                }
                assert_eq!(format_value(&Value::Date(when), None, locale), expected[6], "{tag} short");
            }
        }
        // Monaco and the overseas departments take France's symbols.
        assert_eq!(Locale::from_tag("fr_MC").tag, "fr_FR");
        assert_eq!(Locale::from_tag("fr-RE").tag, "fr_FR");
        // A currency pattern keeps the decimal separator where it is the monetary one too.
        assert_eq!(format_value(&Value::Double(1234.5), Some("\u{A4} #,##0.00"), Locale::from_tag("es_ES")), "€ 1.234,50");
    }

    /// What Java 21.0.13 prints for the French regions that differ from France, for -1234567.5
    /// and 5 July 2026 14:07:09 (the patterns of `french_is_javas`) and a date without a pattern.
    #[test]
    fn french_regions_are_javas() {
        use crate::format::format_value;
        use crate::value::Value;
        let when = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap().and_hms_opt(14, 7, 9).unwrap();
        // fr_FR's strings with another currency, grouping separator, AM/PM or short date and time.
        let french = |currency: &str, group: char, pm: &str, short: &str| -> [String; 7] {
            let number = format!("1{group}234{group}567,50");
            [
                format!("-{number}"),
                format!("-{currency} {number}"),
                format!("-{number} {currency}"),
                "dimanche 5 juillet 2026".to_string(),
                "dim. 5 juil. 26".to_string(),
                format!("2:07 {pm}"),
                short.to_string(),
            ]
        };
        let nnbsp = '\u{202F}';
        let short = "05/07/2026 14:07";
        // A 12-hour clock in the short date and time.
        let short_12 = "05/07/2026 2:07\u{202F}PM";
        let cases = [
            ("fr_LU", french("\u{20AC}", '.', "PM", short)),
            ("fr_MA", french("MAD", '.', "p.m.", short)),
            // West African CFA franc.
            ("fr_SN", french("F\u{202F}CFA", nnbsp, "PM", short)),
            ("fr_CI", french("F\u{202F}CFA", nnbsp, "PM", short)),
            ("fr_BF", french("F\u{202F}CFA", nnbsp, "PM", short)),
            ("fr_BJ", french("F\u{202F}CFA", nnbsp, "PM", short)),
            ("fr_NE", french("F\u{202F}CFA", nnbsp, "PM", short)),
            ("fr_TG", french("F\u{202F}CFA", nnbsp, "PM", short)),
            ("fr_ML", french("F\u{202F}CFA", nnbsp, "PM", "05/07/2026, 14:07")),
            // Central African CFA franc.
            ("fr_GA", french("FCFA", nnbsp, "PM", short)),
            ("fr_CG", french("FCFA", nnbsp, "PM", short)),
            ("fr_CF", french("FCFA", nnbsp, "PM", short)),
            ("fr_GQ", french("FCFA", nnbsp, "PM", short)),
            ("fr_CM", french("FCFA", nnbsp, "soir", short)),
            ("fr_TD", french("FCFA", nnbsp, "PM", short_12)),
            // CFP franc.
            ("fr_NC", french("FCFP", nnbsp, "PM", short)),
            ("fr_PF", french("FCFP", nnbsp, "PM", short)),
            ("fr_WF", french("FCFP", nnbsp, "PM", short)),
            ("fr_BI", french("FBu", nnbsp, "PM", short)),
            ("fr_CD", french("FC", nnbsp, "PM", short)),
            ("fr_GN", french("FG", nnbsp, "PM", short)),
            ("fr_HT", french("G", nnbsp, "PM", short)),
            ("fr_KM", french("CF", nnbsp, "PM", short)),
            ("fr_MG", french("Ar", nnbsp, "PM", short)),
            ("fr_MU", french("Rs", nnbsp, "PM", short)),
            ("fr_RW", french("RF", nnbsp, "PM", short)),
            ("fr_SC", french("SR", nnbsp, "PM", short)),
            ("fr_DZ", french("DA", nnbsp, "PM", short_12)),
            ("fr_DJ", french("Fdj", nnbsp, "PM", short_12)),
            ("fr_SY", french("LS", nnbsp, "PM", short_12)),
            ("fr_MR", french("UM", nnbsp, "PM", short_12)),
            ("fr_TN", french("DT", nnbsp, "PM", short_12)),
            ("fr_VU", french("VT", nnbsp, "PM", short_12)),
        ];
        // The strings Java printed for the first ones, written out.
        assert_eq!(
            cases[1].1,
            [
                "-1.234.567,50",
                "-MAD 1.234.567,50",
                "-1.234.567,50 MAD",
                "dimanche 5 juillet 2026",
                "dim. 5 juil. 26",
                "2:07 p.m.",
                "05/07/2026 14:07"
            ]
        );
        assert_eq!(cases[2].1[1], "-F\u{202F}CFA 1\u{202F}234\u{202F}567,50");
        let patterns =
            ["#,##0.00", "\u{A4} #,##0.00", "#,##0.00 \u{A4};-#,##0.00 \u{A4}", "EEEE d MMMM yyyy", "EEE d MMM yy", "h:mm a"];
        for (tag, expected) in &cases {
            for alias in [tag.to_string(), tag.replace('_', "-")] {
                let locale = Locale::from_tag(&alias);
                assert_eq!(locale.tag, *tag);
                for (i, pattern) in patterns.iter().enumerate() {
                    let value = if i < 3 { Value::Double(-1234567.5) } else { Value::Date(when) };
                    assert_eq!(format_value(&value, Some(pattern), locale), expected[i], "{tag} {pattern}");
                }
                assert_eq!(format_value(&Value::Date(when), None, locale), expected[6], "{tag} short");
            }
        }
        // Morocco's short month names.
        let june = Value::Date(chrono::NaiveDate::from_ymd_opt(2026, 6, 5).unwrap().and_hms_opt(0, 0, 0).unwrap());
        let february = Value::Date(chrono::NaiveDate::from_ymd_opt(2026, 2, 5).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(format_value(&june, Some("MMM"), Locale::from_tag("fr_MA")), "jui.");
        assert_eq!(format_value(&february, Some("MMM"), Locale::from_tag("fr_MA")), "fév.");
        assert_eq!(format_value(&june, Some("MMM"), Locale::from_tag("fr_LU")), "juin");
    }

    /// What Java 21.0.13 prints for Spanish, for -1234567.5 and 5 July 2026 14:07:09 (the
    /// patterns of `french_is_javas`) and a date without a pattern.
    #[test]
    fn spanish_is_javas() {
        use crate::format::format_value;
        use crate::value::Value;
        let when = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap().and_hms_opt(14, 7, 9).unwrap();
        let cases = [
            (
                "es_DO",
                [
                    "-1,234,567.50",
                    "-RD$ 1,234,567.50",
                    "-1,234,567.50 RD$",
                    "domingo 5 julio 2026",
                    "dom 5 jul 26",
                    "2:07 p.\u{A0}m.",
                    "5/7/26, 2:07\u{202F}p.\u{A0}m.",
                ],
            ),
            (
                "es_MX",
                [
                    "-1,234,567.50",
                    "-$ 1,234,567.50",
                    "-1,234,567.50 $",
                    "domingo 5 julio 2026",
                    "dom 5 jul 26",
                    "2:07 p.\u{A0}m.",
                    "05/07/26, 14:07",
                ],
            ),
            (
                "es_ES",
                [
                    "-1.234.567,50",
                    "-\u{20AC} 1.234.567,50",
                    "-1.234.567,50 \u{20AC}",
                    "domingo 5 julio 2026",
                    "dom 5 jul 26",
                    "2:07 p.\u{A0}m.",
                    "5/7/26, 14:07",
                ],
            ),
        ];
        let patterns =
            ["#,##0.00", "\u{A4} #,##0.00", "#,##0.00 \u{A4};-#,##0.00 \u{A4}", "EEEE d MMMM yyyy", "EEE d MMM yy", "h:mm a"];
        for (tag, expected) in cases {
            let locale = Locale::from_tag(tag);
            for (i, pattern) in patterns.iter().enumerate() {
                let value = if i < 3 { Value::Double(-1234567.5) } else { Value::Date(when) };
                assert_eq!(format_value(&value, Some(pattern), locale), expected[i], "{tag} {pattern}");
            }
            assert_eq!(format_value(&Value::Date(when), None, locale), expected[6], "{tag} short");
        }
    }

    /// What Java 21.0.13 prints for the other Spanish locales, for -1234567.5 and 5 July 2026
    /// 14:07:09 (the patterns of `french_is_javas`) and a date without a pattern.
    #[test]
    fn spanish_regions_are_javas() {
        use crate::format::format_value;
        use crate::value::Value;
        let when = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap().and_hms_opt(14, 7, 9).unwrap();
        // -1234567.5 with Spain's separators, with es_DO's, and Costa Rica's.
        let spain = "1.234.567,50";
        let dominican = "1,234,567.50";
        let costa_rica = "1\u{A0}234\u{A0}567,50";
        let spanish = |number: &str, currency: &str, july: &str, pm: &str, short: &str| -> [String; 7] {
            [
                format!("-{number}"),
                format!("-{currency} {number}"),
                format!("-{number} {currency}"),
                "domingo 5 julio 2026".to_string(),
                format!("dom 5 {july} 26"),
                format!("2:07 {pm}"),
                short.to_string(),
            ]
        };
        let pm = "p.\u{A0}m.";
        let cases = [
            ("es", spanish(spain, "\u{A4}", "jul", pm, "5/7/26, 14:07")),
            ("es_EA", spanish(spain, "\u{A4}", "jul", pm, "5/7/26, 14:07")),
            ("es_IC", spanish(spain, "\u{A4}", "jul", pm, "5/7/26, 14:07")),
            ("es_GQ", spanish(spain, "FCFA", "jul", pm, "5/7/26, 14:07")),
            ("es_AR", spanish(spain, "$", "jul", pm, "5/7/26, 14:07")),
            ("es_EC", spanish(spain, "$", "jul", pm, "5/7/26, 14:07")),
            ("es_BO", spanish(spain, "Bs", "jul", pm, "5/7/26, 14:07")),
            ("es_UY", spanish(spain, "$", "jul.", pm, "5/7/26, 14:07")),
            ("es_CL", spanish(spain, "$", "jul", pm, "05-07-26, 14:07")),
            ("es_PY", spanish(spain, "Gs.", "jul.", pm, "5/7/26, 14:07")),
            ("es_CR", spanish(costa_rica, "\u{20A1}", "jul", pm, "5/7/26, 14:07")),
            ("es_419", spanish(dominican, "\u{A4}", "jul", pm, "5/7/26, 14:07")),
            ("es_CU", spanish(dominican, "$", "jul", pm, "5/7/26, 14:07")),
            ("es_HN", spanish(dominican, "L", "jul", pm, "5/7/26, 14:07")),
            ("es_NI", spanish(dominican, "C$", "jul", pm, "5/7/26, 14:07")),
            ("es_SV", spanish(dominican, "C", "jul", pm, "5/7/26, 14:07")),
            ("es_BZ", spanish(dominican, "$", "jul", pm, "5/7/26, 14:07")),
            ("es_BR", spanish(dominican, "R$", "jul", pm, "5/7/26, 14:07")),
            ("es_GT", spanish(dominican, "Q", "jul", pm, "5/07/26, 14:07")),
            ("es_PE", spanish(dominican, "S/", "jul.", pm, "5/07/26, 14:07")),
            ("es_CO", spanish(spain, "$", "jul", "p.\u{202F}m.", "5/07/26, 2:07\u{202F}p.\u{202F}m.")),
            ("es_VE", spanish(spain, "Bs.S", "jul.", pm, "5/7/26, 2:07\u{202F}p.\u{A0}m.")),
            ("es_PH", spanish(spain, "\u{20B1}", "jul", pm, "5/7/26, 2:07\u{202F}p.\u{A0}m.")),
            ("es_PA", spanish(dominican, "B/.", "jul", pm, "07/05/26, 2:07\u{202F}p.\u{A0}m.")),
            ("es_PR", spanish(dominican, "$", "jul", pm, "07/05/26, 2:07\u{202F}p.\u{A0}m.")),
            ("es_US", spanish(dominican, "$", "jul", pm, "5/7/2026, 2:07\u{202F}p.\u{A0}m.")),
        ];
        // One of them written out, as Java printed it.
        assert_eq!(
            cases[4].1,
            [
                "-1.234.567,50",
                "-$ 1.234.567,50",
                "-1.234.567,50 $",
                "domingo 5 julio 2026",
                "dom 5 jul 26",
                "2:07 p.\u{A0}m.",
                "5/7/26, 14:07"
            ]
        );
        let patterns =
            ["#,##0.00", "\u{A4} #,##0.00", "#,##0.00 \u{A4};-#,##0.00 \u{A4}", "EEEE d MMMM yyyy", "EEE d MMM yy", "h:mm a"];
        for (tag, expected) in &cases {
            for alias in [tag.to_string(), tag.replace('_', "-")] {
                let locale = Locale::from_tag(&alias);
                assert_eq!(locale.tag, *tag);
                for (i, pattern) in patterns.iter().enumerate() {
                    let value = if i < 3 { Value::Double(-1234567.5) } else { Value::Date(when) };
                    assert_eq!(format_value(&value, Some(pattern), locale), expected[i], "{tag} {pattern}");
                }
                assert_eq!(format_value(&Value::Date(when), None, locale), expected[6], "{tag} short");
            }
        }
        // September in Peru and Uruguay, and with a point in Paraguay and Venezuela.
        let september = Value::Date(chrono::NaiveDate::from_ymd_opt(2026, 9, 5).unwrap().and_hms_opt(0, 0, 0).unwrap());
        for (tag, want) in [
            ("es_PE", "setiembre set."),
            ("es_UY", "setiembre set."),
            ("es_PY", "septiembre sept."),
            ("es_VE", "septiembre sept."),
            ("es_AR", "septiembre sept"),
        ] {
            assert_eq!(format_value(&september, Some("MMMM MMM"), Locale::from_tag(tag)), want, "{tag}");
        }
        // Tags Java does not have take es_419's.
        assert_eq!(Locale::from_tag("es_XX").tag, "es_419");
    }

    /// What Java 21.0.13 prints for Portuguese, for -1234567.5 and 5 July 2026 14:07:09 (the
    /// patterns of `french_is_javas`) and a date without a pattern.
    #[test]
    fn portuguese_is_javas() {
        use crate::format::format_value;
        use crate::value::Value;
        let when = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap().and_hms_opt(14, 7, 9).unwrap();
        let cases = [
            (
                "pt",
                [
                    "-1.234.567,50",
                    "-\u{A4} 1.234.567,50",
                    "-1.234.567,50 \u{A4}",
                    "domingo 5 julho 2026",
                    "dom. 5 jul. 26",
                    "2:07 PM",
                    "05/07/2026 14:07",
                ],
            ),
            (
                "pt_BR",
                [
                    "-1.234.567,50",
                    "-R$ 1.234.567,50",
                    "-1.234.567,50 R$",
                    "domingo 5 julho 2026",
                    "dom. 5 jul. 26",
                    "2:07 PM",
                    "05/07/2026 14:07",
                ],
            ),
            (
                "pt_PT",
                [
                    "-1\u{A0}234\u{A0}567,50",
                    "-\u{20AC} 1\u{A0}234\u{A0}567,50",
                    "-1\u{A0}234\u{A0}567,50 \u{20AC}",
                    "domingo 5 julho 2026",
                    "domingo 5 jul. 26",
                    "2:07 da tarde",
                    "05/07/26, 14:07",
                ],
            ),
        ];
        let patterns =
            ["#,##0.00", "\u{A4} #,##0.00", "#,##0.00 \u{A4};-#,##0.00 \u{A4}", "EEEE d MMMM yyyy", "EEE d MMM yy", "h:mm a"];
        for (tag, expected) in cases {
            for alias in [tag.to_string(), tag.replace('_', "-")] {
                let locale = Locale::from_tag(&alias);
                assert_eq!(locale.tag, tag);
                for (i, pattern) in patterns.iter().enumerate() {
                    let value = if i < 3 { Value::Double(-1234567.5) } else { Value::Date(when) };
                    assert_eq!(format_value(&value, Some(pattern), locale), expected[i], "{tag} {pattern}");
                }
                assert_eq!(format_value(&Value::Date(when), None, locale), expected[6], "{tag} short");
            }
        }
        // Angola, Mozambique and the others write numbers and dates as Portugal does.
        assert_eq!(Locale::from_tag("pt_AO").tag, "pt_PT");
    }

    /// What Java 21.0.13 prints for Italian, for -1234567.5 and 5 July 2026 14:07:09 (the
    /// patterns of `french_is_javas`) and a date without a pattern.
    #[test]
    fn italian_is_javas() {
        use crate::format::format_value;
        use crate::value::Value;
        let when = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap().and_hms_opt(14, 7, 9).unwrap();
        let cases = [
            (
                "it",
                [
                    "-1.234.567,50",
                    "-\u{A4} 1.234.567,50",
                    "-1.234.567,50 \u{A4}",
                    "domenica 5 luglio 2026",
                    "dom 5 lug 26",
                    "2:07 PM",
                    "05/07/26, 14:07",
                ],
            ),
            (
                "it_IT",
                [
                    "-1.234.567,50",
                    "-\u{20AC} 1.234.567,50",
                    "-1.234.567,50 \u{20AC}",
                    "domenica 5 luglio 2026",
                    "dom 5 lug 26",
                    "2:07 PM",
                    "05/07/26, 14:07",
                ],
            ),
            (
                "it_CH",
                [
                    "-1\u{2019}234\u{2019}567.50",
                    "-SFr. 1\u{2019}234\u{2019}567.50",
                    "-1\u{2019}234\u{2019}567.50 SFr.",
                    "domenica 5 luglio 2026",
                    "dom 5 lug 26",
                    "2:07 PM",
                    "05.07.26, 14:07",
                ],
            ),
        ];
        let patterns =
            ["#,##0.00", "\u{A4} #,##0.00", "#,##0.00 \u{A4};-#,##0.00 \u{A4}", "EEEE d MMMM yyyy", "EEE d MMM yy", "h:mm a"];
        for (tag, expected) in cases {
            for alias in [tag.to_string(), tag.replace('_', "-")] {
                let locale = Locale::from_tag(&alias);
                assert_eq!(locale.tag, tag);
                for (i, pattern) in patterns.iter().enumerate() {
                    let value = if i < 3 { Value::Double(-1234567.5) } else { Value::Date(when) };
                    assert_eq!(format_value(&value, Some(pattern), locale), expected[i], "{tag} {pattern}");
                }
                assert_eq!(format_value(&Value::Date(when), None, locale), expected[6], "{tag} short");
            }
        }
        // San Marino and the Vatican take Italy's.
        assert_eq!(Locale::from_tag("it_SM").tag, "it_IT");
    }

    /// What Java 21.0.13 prints for German, for -1234567.5 and 5 July 2026 14:07:09 (the
    /// patterns of `french_is_javas`) and a date without a pattern.
    #[test]
    fn german_is_javas() {
        use crate::format::format_value;
        use crate::value::Value;
        let when = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap().and_hms_opt(14, 7, 9).unwrap();
        let cases = [
            (
                "de",
                [
                    "-1.234.567,50",
                    "-\u{A4} 1.234.567,50",
                    "-1.234.567,50 \u{A4}",
                    "Sonntag 5 Juli 2026",
                    "So. 5 Juli 26",
                    "2:07 PM",
                    "05.07.26, 14:07",
                ],
            ),
            (
                "de_DE",
                [
                    "-1.234.567,50",
                    "-\u{20AC} 1.234.567,50",
                    "-1.234.567,50 \u{20AC}",
                    "Sonntag 5 Juli 2026",
                    "So. 5 Juli 26",
                    "2:07 PM",
                    "05.07.26, 14:07",
                ],
            ),
            // Austria groups with a no-break space, and with a point in amounts with a currency.
            (
                "de_AT",
                [
                    "-1\u{A0}234\u{A0}567,50",
                    "-\u{20AC} 1.234.567,50",
                    "-1.234.567,50 \u{20AC}",
                    "Sonntag 5 Juli 2026",
                    "So. 5 Juli 26",
                    "2:07 PM",
                    "05.07.26, 14:07",
                ],
            ),
            (
                "de_CH",
                [
                    "-1\u{2019}234\u{2019}567.50",
                    "-CHF 1\u{2019}234\u{2019}567.50",
                    "-1\u{2019}234\u{2019}567.50 CHF",
                    "Sonntag 5 Juli 2026",
                    "So. 5 Juli 26",
                    "2:07 PM",
                    "05.07.26, 14:07",
                ],
            ),
        ];
        let patterns =
            ["#,##0.00", "\u{A4} #,##0.00", "#,##0.00 \u{A4};-#,##0.00 \u{A4}", "EEEE d MMMM yyyy", "EEE d MMM yy", "h:mm a"];
        for (tag, expected) in cases {
            for alias in [tag.to_string(), tag.replace('_', "-")] {
                let locale = Locale::from_tag(&alias);
                assert_eq!(locale.tag, tag);
                for (i, pattern) in patterns.iter().enumerate() {
                    let value = if i < 3 { Value::Double(-1234567.5) } else { Value::Date(when) };
                    assert_eq!(format_value(&value, Some(pattern), locale), expected[i], "{tag} {pattern}");
                }
                assert_eq!(format_value(&Value::Date(when), None, locale), expected[6], "{tag} short");
            }
        }
        let january = Value::Date(chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap().and_hms_opt(0, 0, 0).unwrap());
        assert_eq!(format_value(&january, Some("MMMM MMM"), Locale::from_tag("de_DE")), "Januar Jan.");
        assert_eq!(format_value(&january, Some("MMMM MMM"), Locale::from_tag("de_AT")), "Jänner Jän.");
        // Liechtenstein writes numbers as Switzerland does; Luxembourg and Belgium as Germany.
        assert_eq!(Locale::from_tag("de_LI").tag, "de_CH");
        assert_eq!(Locale::from_tag("de_LU").tag, "de_DE");
    }

    #[test]
    fn display_names_are_javas() {
        // Locale.getDisplayName on Java 21, for the locales of JasperReports' i18n sample.
        let cases = [
            ("en_US", "en_US", "English (United States)"),
            ("en_US", "es_DO", "inglés (Estados Unidos)"),
            ("es_DO", "en_US", "Spanish (Dominican Republic)"),
            ("es_DO", "es_DO", "español (República Dominicana)"),
            ("cs_CZ", "cs_CZ", "čeština (Česko)"),
            ("cs_CZ", "en", "Czech (Czechia)"),
            ("de_DE", "de_DE", "Deutsch (Deutschland)"),
            ("de_AT", "es", "alemán (Austria)"),
            ("fr_FR", "fr_FR", "français (France)"),
            ("pt_PT", "pt_PT", "português (Portugal)"),
            ("ro_RO", "ro_RO", "română (România)"),
            ("sk_SK", "sk_SK", "slovenčina (Slovensko)"),
            ("zh_TW", "zh_TW", "中文 (台灣)"),
            ("es-419", "es", "español (Latinoamérica)"),
            ("en", "en_US", "English"),
            ("xx_YY", "en_US", "xx (YY)"),
        ];
        for (tag, display, want) in cases {
            assert_eq!(display_name(tag, display), want, "{tag} in {display}");
        }
        assert_eq!(display_language("de_DE", "en_US"), "German");
        assert_eq!(display_country("de_DE", "es"), "Alemania");
        assert_eq!(display_country("en", "en"), "");
        assert_eq!(split_tag("hello world"), None);
    }
}
