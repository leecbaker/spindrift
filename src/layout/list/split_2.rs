pub(in crate::layout) const CJK_DECIMAL_DIGITS: &[&str; 10] =
    &["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

pub(in crate::layout) fn numeric_marker_i32(index: i32, digits: &[&str; 10]) -> String {
    let sign = if index < 0 { "-" } else { "" };
    let value = i64::from(index).abs().to_string();
    let mut output = String::from(sign);
    for digit in value.bytes() {
        output.push_str(digits[(digit - b'0') as usize]);
    }
    output
}

pub(in crate::layout) fn ethiopic_group_text(group: i32) -> String {
    const TENS: [&str; 10] = ["", "፲", "፳", "፴", "፵", "፶", "፷", "፸", "፹", "፺"];
    const UNITS: [&str; 10] = ["", "፩", "፪", "፫", "፬", "፭", "፮", "፯", "፰", "፱"];

    let tens = (group / 10) as usize;
    let units = (group % 10) as usize;
    format!("{}{}", TENS[tens], UNITS[units])
}
