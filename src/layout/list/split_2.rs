use super::*;

pub(in crate::layout) fn ethiopic_group_text(group: i32) -> &'static str {
    const TENS: [&str; 10] = ["", "", "፳", "፴", "፵", "፶", "፷", "፸", "፹", "፺"];
    const UNITS: [&str; 10] = ["", "፩", "፪", "፫", "፬", "፭", "፮", "፯", "፰", "፱"];
    match (group / 10, group % 10) {
        (0, unit) => UNITS[unit as usize],
        (1, unit) => match unit {
            0 => "፲",
            1 => "፲፩",
            2 => "፲፪",
            3 => "፲፫",
            4 => "፲፬",
            5 => "፲፭",
            6 => "፲፮",
            7 => "፲፯",
            8 => "፲፰",
            9 => "፲፱",
            _ => "",
        },
        (ten, 0) => TENS[ten as usize],
        (ten, unit) => match (TENS[ten as usize], UNITS[unit as usize]) {
            ("፳", "፩") => "፳፩",
            ("፳", "፪") => "፳፪",
            ("፳", "፫") => "፳፫",
            ("፳", "፬") => "፳፬",
            ("፳", "፭") => "፳፭",
            ("፳", "፮") => "፳፮",
            ("፳", "፯") => "፳፯",
            ("፳", "፰") => "፳፰",
            ("፳", "፱") => "፳፱",
            ("፴", "፩") => "፴፩",
            ("፴", "፪") => "፴፪",
            ("፴", "፫") => "፴፫",
            ("፴", "፬") => "፴፬",
            ("፴", "፭") => "፴፭",
            ("፴", "፮") => "፴፮",
            ("፴", "፯") => "፴፯",
            ("፴", "፰") => "፴፰",
            ("፴", "፱") => "፴፱",
            ("፵", "፩") => "፵፩",
            ("፵", "፪") => "፵፪",
            ("፵", "፫") => "፵፫",
            ("፵", "፬") => "፵፬",
            ("፵", "፭") => "፵፭",
            ("፵", "፮") => "፵፮",
            ("፵", "፯") => "፵፯",
            ("፵", "፰") => "፵፰",
            ("፵", "፱") => "፵፱",
            ("፶", "፩") => "፶፩",
            ("፶", "፪") => "፶፪",
            ("፶", "፫") => "፶፫",
            ("፶", "፬") => "፶፬",
            ("፶", "፭") => "፶፭",
            ("፶", "፮") => "፶፮",
            ("፶", "፯") => "፶፯",
            ("፶", "፰") => "፶፰",
            ("፶", "፱") => "፶፱",
            ("፷", "፩") => "፷፩",
            ("፷", "፪") => "፷፪",
            ("፷", "፫") => "፷፫",
            ("፷", "፬") => "፷፬",
            ("፷", "፭") => "፷፭",
            ("፷", "፮") => "፷፮",
            ("፷", "፯") => "፷፯",
            ("፷", "፰") => "፷፰",
            ("፷", "፱") => "፷፱",
            ("፸", "፩") => "፸፩",
            ("፸", "፪") => "፸፪",
            ("፸", "፫") => "፸፫",
            ("፸", "፬") => "፸፬",
            ("፸", "፭") => "፸፭",
            ("፸", "፮") => "፸፮",
            ("፸", "፯") => "፸፯",
            ("፸", "፰") => "፸፰",
            ("፸", "፱") => "፸፱",
            ("፹", "፩") => "፹፩",
            ("፹", "፪") => "፹፪",
            ("፹", "፫") => "፹፫",
            ("፹", "፬") => "፹፬",
            ("፹", "፭") => "፹፭",
            ("፹", "፮") => "፹፮",
            ("፹", "፯") => "፹፯",
            ("፹", "፰") => "፹፰",
            ("፹", "፱") => "፹፱",
            ("፺", "፩") => "፺፩",
            ("፺", "፪") => "፺፪",
            ("፺", "፫") => "፺፫",
            ("፺", "፬") => "፺፬",
            ("፺", "፭") => "፺፭",
            ("፺", "፮") => "፺፮",
            ("፺", "፯") => "፺፯",
            ("፺", "፰") => "፺፰",
            ("፺", "፱") => "፺፱",
            _ => "",
        },
    }
}

pub(in crate::layout) fn decimal_leading_zero_marker(index: i32) -> String {
    let sign = if index < 0 { "-" } else { "" };
    let value = i64::from(index).abs().to_string();
    if value.chars().count() >= 2 {
        format!("{sign}{value}")
    } else {
        format!("{sign}0{value}")
    }
}

pub(in crate::layout) fn numeric_marker_i32(index: i32, digits: &[&str; 10]) -> String {
    let sign = if index < 0 { "-" } else { "" };
    let value = i64::from(index).abs().to_string();
    let mut output = String::new();
    output.push_str(sign);
    for digit in value.bytes() {
        let index = (digit - b'0') as usize;
        output.push_str(digits[index]);
    }
    output
}

pub(in crate::layout) fn alpha_marker_i32(index: i32, uppercase: bool) -> String {
    if index <= 0 {
        return index.to_string();
    }
    alpha_marker(index as usize, uppercase)
}

pub(in crate::layout) fn alphabetic_marker_i32(index: i32, symbols: &[&str]) -> String {
    if index <= 0 {
        return index.to_string();
    }
    let base = symbols.len();
    let mut value = index as usize;
    let mut output = Vec::new();
    while value > 0 {
        value -= 1;
        output.push(symbols[value % base]);
        value /= base;
    }
    output.iter().rev().copied().collect::<String>()
}

pub(in crate::layout) fn fixed_marker_i32(index: i32, symbols: &[&str]) -> String {
    if index <= 0 {
        return index.to_string();
    }
    let Ok(index) = usize::try_from(index) else {
        return index.to_string();
    };
    symbols
        .get(index - 1)
        .map(|symbol| (*symbol).to_string())
        .unwrap_or_else(|| index.to_string())
}

pub(in crate::layout) fn additive_marker_i32(
    index: i32,
    symbols: &[(i32, &str)],
    range: (i32, i32),
) -> String {
    if index < range.0 || index > range.1 {
        return index.to_string();
    }
    let mut value = index;
    let mut output = String::new();
    for (weight, symbol) in symbols {
        while value >= *weight {
            output.push_str(symbol);
            value -= *weight;
        }
    }
    if value == 0 {
        output
    } else {
        index.to_string()
    }
}

pub(in crate::layout) fn roman_marker_i32(index: i32, uppercase: bool) -> String {
    if !(1..=3999).contains(&index) {
        return index.to_string();
    }
    let mut value = index;
    let mut output = String::new();
    for (number, numeral) in [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while value >= number {
            output.push_str(numeral);
            value -= number;
        }
    }
    if uppercase {
        output.to_uppercase()
    } else {
        output
    }
}

pub(in crate::layout) fn numeric_digits(style: NumericCounterStyle) -> &'static [&'static str; 10] {
    match style {
        NumericCounterStyle::ArabicIndic => &["٠", "١", "٢", "٣", "٤", "٥", "٦", "٧", "٨", "٩"],
        NumericCounterStyle::Bengali => &["০", "১", "২", "৩", "৪", "৫", "৬", "৭", "৮", "৯"],
        NumericCounterStyle::Cambodian => &["០", "១", "២", "៣", "៤", "៥", "៦", "៧", "៨", "៩"],
        NumericCounterStyle::CjkDecimal => {
            &["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"]
        }
        NumericCounterStyle::Devanagari => &["०", "१", "२", "३", "४", "५", "६", "७", "८", "९"],
        NumericCounterStyle::Gujarati => &["૦", "૧", "૨", "૩", "૪", "૫", "૬", "૭", "૮", "૯"],
        NumericCounterStyle::Gurmukhi => &["੦", "੧", "੨", "੩", "੪", "੫", "੬", "੭", "੮", "੯"],
        NumericCounterStyle::Kannada => &["೦", "೧", "೨", "೩", "೪", "೫", "೬", "೭", "೮", "೯"],
        NumericCounterStyle::Lao => &["໐", "໑", "໒", "໓", "໔", "໕", "໖", "໗", "໘", "໙"],
        NumericCounterStyle::Malayalam => &["൦", "൧", "൨", "൩", "൪", "൫", "൬", "൭", "൮", "൯"],
        NumericCounterStyle::Mongolian => &["᠐", "᠑", "᠒", "᠓", "᠔", "᠕", "᠖", "᠗", "᠘", "᠙"],
        NumericCounterStyle::Myanmar => &["၀", "၁", "၂", "၃", "၄", "၅", "၆", "၇", "၈", "၉"],
        NumericCounterStyle::Oriya => &["୦", "୧", "୨", "୩", "୪", "୫", "୬", "୭", "୮", "୯"],
        NumericCounterStyle::Persian => &["۰", "۱", "۲", "۳", "۴", "۵", "۶", "۷", "۸", "۹"],
        NumericCounterStyle::Tamil => &["௦", "௧", "௨", "௩", "௪", "௫", "௬", "௭", "௮", "௯"],
        NumericCounterStyle::Telugu => &["౦", "౧", "౨", "౩", "౪", "౫", "౬", "౭", "౮", "౯"],
        NumericCounterStyle::Thai => &["๐", "๑", "๒", "๓", "๔", "๕", "๖", "๗", "๘", "๙"],
        NumericCounterStyle::Tibetan => &["༠", "༡", "༢", "༣", "༤", "༥", "༦", "༧", "༨", "༩"],
    }
}

pub(in crate::layout) fn additive_symbols(
    style: AdditiveCounterStyle,
) -> &'static [(i32, &'static str)] {
    match style {
        AdditiveCounterStyle::Armenian => ARMENIAN_ADDITIVE,
        AdditiveCounterStyle::LowerArmenian => LOWER_ARMENIAN_ADDITIVE,
        AdditiveCounterStyle::Georgian => GEORGIAN_ADDITIVE,
        AdditiveCounterStyle::Hebrew => HEBREW_ADDITIVE,
    }
}

pub(in crate::layout) fn additive_range(style: AdditiveCounterStyle) -> (i32, i32) {
    match style {
        AdditiveCounterStyle::Armenian | AdditiveCounterStyle::LowerArmenian => (1, 9999),
        AdditiveCounterStyle::Georgian => (1, 19999),
        AdditiveCounterStyle::Hebrew => (1, 10999),
    }
}

pub(in crate::layout) const ARMENIAN_ADDITIVE: &[(i32, &str)] = &[
    (9000, "Ք"),
    (8000, "Փ"),
    (7000, "Ւ"),
    (6000, "Ց"),
    (5000, "Ր"),
    (4000, "Տ"),
    (3000, "Վ"),
    (2000, "Ս"),
    (1000, "Ռ"),
    (900, "Ջ"),
    (800, "Պ"),
    (700, "Չ"),
    (600, "Ո"),
    (500, "Շ"),
    (400, "Ն"),
    (300, "Յ"),
    (200, "Մ"),
    (100, "Ճ"),
    (90, "Ղ"),
    (80, "Ձ"),
    (70, "Հ"),
    (60, "Կ"),
    (50, "Ծ"),
    (40, "Խ"),
    (30, "Լ"),
    (20, "Ի"),
    (10, "Ժ"),
    (9, "Թ"),
    (8, "Ը"),
    (7, "Է"),
    (6, "Զ"),
    (5, "Ե"),
    (4, "Դ"),
    (3, "Գ"),
    (2, "Բ"),
    (1, "Ա"),
];
pub(in crate::layout) const LOWER_ARMENIAN_ADDITIVE: &[(i32, &str)] = &[
    (9000, "ք"),
    (8000, "փ"),
    (7000, "ւ"),
    (6000, "ց"),
    (5000, "ր"),
    (4000, "տ"),
    (3000, "վ"),
    (2000, "ս"),
    (1000, "ռ"),
    (900, "ջ"),
    (800, "պ"),
    (700, "չ"),
    (600, "ո"),
    (500, "շ"),
    (400, "ն"),
    (300, "յ"),
    (200, "մ"),
    (100, "ճ"),
    (90, "ղ"),
    (80, "ձ"),
    (70, "հ"),
    (60, "կ"),
    (50, "ծ"),
    (40, "խ"),
    (30, "լ"),
    (20, "ի"),
    (10, "ժ"),
    (9, "թ"),
    (8, "ը"),
    (7, "է"),
    (6, "զ"),
    (5, "ե"),
    (4, "դ"),
    (3, "գ"),
    (2, "բ"),
    (1, "ա"),
];
pub(in crate::layout) const GEORGIAN_ADDITIVE: &[(i32, &str)] = &[
    (10000, "ჵ"),
    (9000, "ჰ"),
    (8000, "ჯ"),
    (7000, "ჴ"),
    (6000, "ხ"),
    (5000, "ჭ"),
    (4000, "წ"),
    (3000, "ძ"),
    (2000, "ც"),
    (1000, "ჩ"),
    (900, "შ"),
    (800, "ყ"),
    (700, "ღ"),
    (600, "ქ"),
    (500, "ფ"),
    (400, "ჳ"),
    (300, "ტ"),
    (200, "ს"),
    (100, "რ"),
    (90, "ჟ"),
    (80, "პ"),
    (70, "ო"),
    (60, "ჲ"),
    (50, "ნ"),
    (40, "მ"),
    (30, "ლ"),
    (20, "კ"),
    (10, "ი"),
    (9, "თ"),
    (8, "ჱ"),
    (7, "ზ"),
    (6, "ვ"),
    (5, "ე"),
    (4, "დ"),
    (3, "გ"),
    (2, "ბ"),
    (1, "ა"),
];
pub(in crate::layout) const HEBREW_ADDITIVE: &[(i32, &str)] = &[
    (10000, "י׳"),
    (9000, "ט׳"),
    (8000, "ח׳"),
    (7000, "ז׳"),
    (6000, "ו׳"),
    (5000, "ה׳"),
    (4000, "ד׳"),
    (3000, "ג׳"),
    (2000, "ב׳"),
    (1000, "א׳"),
    (400, "ת"),
    (300, "ש"),
    (200, "ר"),
    (100, "ק"),
    (90, "צ"),
    (80, "פ"),
    (70, "ע"),
    (60, "ס"),
    (50, "נ"),
    (40, "מ"),
    (30, "ל"),
    (20, "כ"),
    (19, "יט"),
    (18, "יח"),
    (17, "יז"),
    (16, "טז"),
    (15, "טו"),
    (10, "י"),
    (9, "ט"),
    (8, "ח"),
    (7, "ז"),
    (6, "ו"),
    (5, "ה"),
    (4, "ד"),
    (3, "ג"),
    (2, "ב"),
    (1, "א"),
];

pub(in crate::layout) const LOWER_GREEK_SYMBOLS: &[&str] = &[
    "α", "β", "γ", "δ", "ε", "ζ", "η", "θ", "ι", "κ", "λ", "μ", "ν", "ξ", "ο", "π", "ρ", "σ", "τ",
    "υ", "φ", "χ", "ψ", "ω",
];
pub(in crate::layout) const HIRAGANA_SYMBOLS: &[&str] = &[
    "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し", "す", "せ", "そ", "た",
    "ち", "つ", "て", "と", "な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ", "ま", "み",
    "む", "め", "も", "や", "ゆ", "よ", "ら", "り", "る", "れ", "ろ", "わ", "ゐ", "ゑ", "を", "ん",
];
pub(in crate::layout) const HIRAGANA_IROHA_SYMBOLS: &[&str] = &[
    "い", "ろ", "は", "に", "ほ", "へ", "と", "ち", "り", "ぬ", "る", "を", "わ", "か", "よ", "た",
    "れ", "そ", "つ", "ね", "な", "ら", "む", "う", "ゐ", "の", "お", "く", "や", "ま", "け", "ふ",
    "こ", "え", "て", "あ", "さ", "き", "ゆ", "め", "み", "し", "ゑ", "ひ", "も", "せ", "す",
];
pub(in crate::layout) const KATAKANA_SYMBOLS: &[&str] = &[
    "ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ", "サ", "シ", "ス", "セ", "ソ", "タ",
    "チ", "ツ", "テ", "ト", "ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ", "マ", "ミ",
    "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ", "ル", "レ", "ロ", "ワ", "ヰ", "ヱ", "ヲ", "ン",
];
pub(in crate::layout) const KATAKANA_IROHA_SYMBOLS: &[&str] = &[
    "イ", "ロ", "ハ", "ニ", "ホ", "ヘ", "ト", "チ", "リ", "ヌ", "ル", "ヲ", "ワ", "カ", "ヨ", "タ",
    "レ", "ソ", "ツ", "ネ", "ナ", "ラ", "ム", "ウ", "ヰ", "ノ", "オ", "ク", "ヤ", "マ", "ケ", "フ",
    "コ", "エ", "テ", "ア", "サ", "キ", "ユ", "メ", "ミ", "シ", "ヱ", "ヒ", "モ", "セ", "ス",
];
pub(in crate::layout) const CJK_EARTHLY_BRANCH: &[&str] = &[
    "子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥",
];
pub(in crate::layout) const CJK_HEAVENLY_STEM: &[&str] =
    &["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
