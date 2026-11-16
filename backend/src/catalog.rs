// 알림 종류 2뎁스 카탈로그: 카테고리 > 제공자 (frontend/src/catalog.ts와 동기 유지)
pub const CATALOG: &[(&str, &[&str])] = &[
    ("inventory", &["emart", "nintendo", "playstation"]),
    ("movie", &["cgv", "lotte", "megabox"]),
    ("price", &["fx", "stock"]),
    ("community", &["dcinside"]),
];

/// 카테고리·제공자 조합이 카탈로그에 있는지.
/// CATALOG[category]?.includes(provider) - category 가 Object.prototype 속성 이름이면 TS 는 TypeError (Err)
pub fn is_valid_target(category_key: &str, provider: Option<&str>) -> Result<bool, String> {
    if let Some((_, list)) = CATALOG.iter().find(|(k, _)| *k == category_key) {
        return Ok(provider.is_some_and(|p| list.contains(&p)));
    }
    if crate::jsx::is_proto_key(category_key) {
        return Err("CATALOG[category]?.includes is not a function".into());
    }
    Ok(false)
}

// 알림 제목 앞에 붙는 provider별 이모지 (영화 3사는 동일, 쇼핑은 마트 아이콘)
fn provider_emoji(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "emart" => "🛒",
        "nintendo" => "🎮",
        "playstation" => "🎮",
        "cgv" => "🎬",
        "lotte" => "🎬",
        "megabox" => "🎬",
        "fx" => "💱",
        "stock" => "📈",
        "dcinside" => "💬",
        _ => return None,
    })
}

// 영화·쇼핑은 어느 브랜드인지 제목에 표기 (시세는 이름만으로 충분)
fn provider_label(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "emart" => "이마트",
        "nintendo" => "닌텐도",
        "playstation" => "PS Store",
        "cgv" => "CGV",
        "lotte" => "롯데시네마",
        "megabox" => "메가박스",
        "dcinside" => "디시인사이드",
        _ => return None,
    })
}

// 합쳐진 브라우저 알림 제목용 (개별 제목에 브랜드를 안 쓰는 fx·stock도 이름이 필요)
fn group_label(source: &str) -> Option<&'static str> {
    match source {
        "fx" => Some("환율"),
        "stock" => Some("주식"),
        s => provider_label(s),
    }
}

/// 알림 제목: '🎬 CGV · 오디세이'처럼 이모지·브랜드 프리픽스를 붙인다
pub fn notification_title(provider: &str, name: &str) -> String {
    let text = match provider_label(provider) {
        Some(label) => format!("{label} · {name}"),
        None => name.to_string(),
    };
    match provider_emoji(provider) {
        Some(emoji) => format!("{emoji} {text}"),
        None => text,
    }
}

/// 같은 종류를 합친 브라우저 알림 제목: '🎬 CGV 알림 3건'
pub fn group_notification_title(source: &str, count: usize) -> String {
    let emoji = provider_emoji(source).unwrap_or("🔔");
    let label = group_label(source).unwrap_or("두노티");
    format!("{emoji} {label} 알림 {count}건")
}
