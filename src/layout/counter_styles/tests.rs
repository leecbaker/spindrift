use super::*;
use crate::css::CounterStyleRangeInterval;

fn ua_counter_styles() -> HashMap<String, CounterStyleRule> {
    crate::css::html5_user_agent_stylesheet()
        .counter_styles
        .iter()
        .cloned()
        .map(|style| (style.name.clone(), style))
        .collect()
}

fn rule(name: &str, system: CounterStyleSystem) -> CounterStyleRule {
    CounterStyleRule {
        name: name.to_string(),
        system,
        symbols: Vec::new(),
        additive_symbols: Vec::new(),
        prefix: None,
        suffix: None,
        negative: None,
        pad: None,
        range: None,
        fallback: None,
        speak_as: None,
    }
}

fn fixed_rule(name: &str, first: i32, symbols: &[&str]) -> CounterStyleRule {
    let mut rule = rule(name, CounterStyleSystem::Fixed(first));
    rule.symbols = symbols.iter().map(|symbol| (*symbol).to_string()).collect();
    rule
}

#[test]
fn fallback_chains_are_unbounded_but_cycles_use_decimal() {
    let mut styles = HashMap::new();
    for index in 0..12 {
        let name = format!("style-{index}");
        let mut rule = fixed_rule(&name, 1, &["x"]);
        rule.range = Some(CounterStyleRange::Intervals(vec![
            CounterStyleRangeInterval { start: 1, end: 1 },
        ]));
        rule.fallback = Some(format!("style-{}", index + 1));
        styles.insert(name, rule);
    }
    let last = fixed_rule("style-12", 13, &["z"]);
    styles.insert(last.name.clone(), last);

    assert_eq!(
        custom_counter_text(styles.get("style-0").unwrap(), 13, &styles),
        Some("z".to_string())
    );

    let mut a = fixed_rule("a", 1, &["a"]);
    a.range = Some(CounterStyleRange::Intervals(vec![
        CounterStyleRangeInterval { start: 1, end: 1 },
    ]));
    a.fallback = Some("b".to_string());
    let mut b = fixed_rule("b", 1, &["b"]);
    b.range = Some(CounterStyleRange::Intervals(vec![
        CounterStyleRangeInterval { start: 1, end: 1 },
    ]));
    b.fallback = Some("a".to_string());
    let cycles = HashMap::from([(a.name.clone(), a.clone()), (b.name.clone(), b)]);
    assert_eq!(custom_counter_text(&a, 2, &cycles), Some("2".to_string()));
}

#[test]
fn fallback_representation_keeps_the_requested_marker_affixes() {
    let mut requested = fixed_rule("requested", 1, &["a"]);
    requested.range = Some(CounterStyleRange::Intervals(vec![
        CounterStyleRangeInterval { start: 1, end: 1 },
    ]));
    requested.fallback = Some("fallback".to_string());
    requested.prefix = Some("[".to_string());
    requested.suffix = Some("]".to_string());
    let fallback = fixed_rule("fallback", 2, &["b"]);
    let styles = HashMap::from([
        (requested.name.clone(), requested.clone()),
        (fallback.name.clone(), fallback),
    ]);

    assert_eq!(
        custom_counter_marker_text(&requested, 2, &styles),
        Some(("[b]".to_string(), false))
    );
}

#[test]
fn pad_uses_grapheme_clusters_and_includes_negative_affixes() {
    let mut combining = fixed_rule("combining", 1, &["a\u{0304}"]);
    combining.pad = Some((2, "o".to_string()));
    assert_eq!(
        custom_counter_text(&combining, 1, &HashMap::new()),
        Some("oa\u{0304}".to_string())
    );

    let mut emoji = fixed_rule("emoji", 1, &["👩‍💻"]);
    emoji.pad = Some((2, "o".to_string()));
    assert_eq!(
        custom_counter_text(&emoji, 1, &HashMap::new()),
        Some("o👩‍💻".to_string())
    );

    let mut negative = rule("negative", CounterStyleSystem::Numeric);
    negative.symbols = decimal_counter_symbols();
    negative.pad = Some((4, "0".to_string()));
    negative.negative = Some(("(".to_string(), ")".to_string()));
    assert_eq!(
        custom_counter_text(&negative, -2, &HashMap::new()),
        Some("(02)".to_string())
    );

    let fixed = fixed_rule("negative-fixed", -1, &["a"]);
    assert_eq!(
        custom_counter_text(&fixed, -1, &HashMap::new()),
        Some("a".to_string())
    );

    let cyclic = rule("negative-cyclic", CounterStyleSystem::Cyclic);
    let mut cyclic = cyclic;
    cyclic.symbols = vec!["a".into(), "b".into()];
    assert_eq!(
        custom_counter_text(&cyclic, -2, &HashMap::new()),
        Some("b".to_string())
    );
}

#[test]
fn disclosure_styles_follow_writing_context_and_remain_extendable() {
    let mut extended = rule(
        "custom-disclosure",
        CounterStyleSystem::Extends("disclosure-closed".into()),
    );
    extended.prefix = Some("[".into());
    extended.suffix = Some("]".into());
    let styles = HashMap::from([(extended.name.clone(), extended.clone())]);
    let cases = [
        (ComputedStyle::initial(), "\u{25b8}", "\u{25be}"),
        (
            {
                let mut style = ComputedStyle::initial();
                style.direction = Direction::Rtl;
                style
            },
            "\u{25c2}",
            "\u{25be}",
        ),
        (
            {
                let mut style = ComputedStyle::initial();
                style.writing_mode = WritingMode::VerticalLr;
                style
            },
            "\u{25be}",
            "\u{25b8}",
        ),
        (
            {
                let mut style = ComputedStyle::initial();
                style.writing_mode = WritingMode::VerticalRl;
                style.direction = Direction::Rtl;
                style
            },
            "\u{25b4}",
            "\u{25c2}",
        ),
    ];

    for (style, closed, open) in cases {
        let context = CounterStyleRenderContext::for_style(&style);
        assert_eq!(
            counter_text_with_context(ListStyleType::DisclosureClosed, 1, &styles, context,),
            Some(closed.to_string())
        );
        assert_eq!(
            counter_text_with_context(ListStyleType::DisclosureOpen, 1, &styles, context),
            Some(open.to_string())
        );
        assert_eq!(
            custom_counter_marker_text_with_context(&extended, 1, &styles, context),
            Some((format!("[{closed}]"), false))
        );
    }
}

#[test]
fn extends_cycles_repair_only_the_cycle_members_with_decimal_bases() {
    let mut a = rule("a", CounterStyleSystem::Extends("b".into()));
    a.prefix = Some("a".into());
    let mut b = rule("b", CounterStyleSystem::Extends("c".into()));
    b.suffix = Some("b".into());
    let mut c = rule("c", CounterStyleSystem::Extends("b".into()));
    c.pad = Some((2, "c".into()));
    let styles = HashMap::from([
        (a.name.clone(), a.clone()),
        (b.name.clone(), b),
        (c.name.clone(), c),
    ]);

    let a = resolve_counter_style(&a, &styles, 0);
    assert_eq!(a.prefix, "a");
    assert_eq!(a.suffix, "b");
    assert_eq!(a.pad, None);

    let b = resolve_counter_style(styles.get("b").unwrap(), &styles, 0);
    assert_eq!(b.prefix, "");
    assert_eq!(b.suffix, "b");
    assert_eq!(b.pad, None);

    let c = resolve_counter_style(styles.get("c").unwrap(), &styles, 0);
    assert_eq!(c.prefix, "");
    assert_eq!(c.suffix, ". ");
    assert_eq!(c.pad, Some((2, "c".into())));
}

#[test]
fn complex_predefined_styles_are_extendable() {
    let custom = rule(
        "chapter",
        CounterStyleSystem::Extends("simp-chinese-informal".into()),
    );
    let styles = HashMap::from([(custom.name.clone(), custom.clone())]);
    let effective = resolve_counter_style(&custom, &styles, 0);

    assert_eq!(effective.predefined, Some("simp-chinese-informal"));
    assert_eq!(effective.suffix, "、");
    assert_eq!(
        custom_counter_text(&custom, 1_000, &styles),
        Some("一千".into())
    );
}

#[test]
fn lookup_preserves_custom_case_and_normalizes_predefined_names() {
    let custom = rule("custom", CounterStyleSystem::Numeric);
    let predefined = rule("decimal-leading-zero", CounterStyleSystem::Numeric);
    let styles = HashMap::from([
        (custom.name.clone(), custom),
        (predefined.name.clone(), predefined),
    ]);
    assert!(counter_style_rule("Custom", &styles).is_none());
    assert!(counter_style_rule("custom", &styles).is_some());
    assert!(counter_style_rule("Decimal-Leading-Zero", &styles).is_some());
}

#[test]
fn cjk_decimal_honors_its_ua_range_and_fallback() {
    let counter_styles = ua_counter_styles();
    let style = crate::css::parse_list_style_type("cjk-decimal").expect("valid style");

    assert_eq!(style, ListStyleType::Named("cjk-decimal".to_string()));
    assert_eq!(
        counter_text(style.clone(), 12_345, &counter_styles),
        Some("一二三四五".to_string())
    );
    assert_eq!(
        counter_text(style, -1, &counter_styles),
        Some("-1".to_string())
    );
}
