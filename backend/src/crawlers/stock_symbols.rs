// 주식 종목 마스터 시드 - 유명 종목만. 코드는 네이버 증권 기준
// (국내 6자리, 나스닥 .O 접미사, NYSE 무접미사, 해외지수 . 접두사)
use rusqlite::Connection;

use crate::core::db::{self, P};

/// (code, name, kind, market, keywords)
type S = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

pub const STOCK_SEED: &[S] = &[
    // 한국주식
    ("005930", "삼성전자", "kr", "domestic", "samsung"),
    ("000660", "SK하이닉스", "kr", "domestic", "hynix"),
    ("373220", "LG에너지솔루션", "kr", "domestic", "엔솔"),
    ("207940", "삼성바이오로직스", "kr", "domestic", ""),
    ("005380", "현대차", "kr", "domestic", "현대자동차 hyundai"),
    ("000270", "기아", "kr", "domestic", "kia"),
    ("035420", "NAVER", "kr", "domestic", "네이버"),
    ("035720", "카카오", "kr", "domestic", "kakao"),
    ("068270", "셀트리온", "kr", "domestic", ""),
    ("005490", "POSCO홀딩스", "kr", "domestic", "포스코"),
    ("051910", "LG화학", "kr", "domestic", ""),
    ("006400", "삼성SDI", "kr", "domestic", ""),
    ("105560", "KB금융", "kr", "domestic", ""),
    ("055550", "신한지주", "kr", "domestic", ""),
    ("086790", "하나금융지주", "kr", "domestic", ""),
    ("012450", "한화에어로스페이스", "kr", "domestic", ""),
    ("042660", "한화오션", "kr", "domestic", ""),
    ("329180", "HD현대중공업", "kr", "domestic", ""),
    ("028260", "삼성물산", "kr", "domestic", ""),
    ("015760", "한국전력", "kr", "domestic", "한전"),
    ("032830", "삼성생명", "kr", "domestic", ""),
    ("000810", "삼성화재", "kr", "domestic", ""),
    ("259960", "크래프톤", "kr", "domestic", "krafton"),
    ("036570", "엔씨소프트", "kr", "domestic", "nc"),
    ("352820", "하이브", "kr", "domestic", "hybe"),
    ("090430", "아모레퍼시픽", "kr", "domestic", ""),
    ("097950", "CJ제일제당", "kr", "domestic", ""),
    // 미국주식
    ("AAPL.O", "애플", "us", "world", "apple"),
    ("MSFT.O", "마이크로소프트", "us", "world", "microsoft"),
    ("NVDA.O", "엔비디아", "us", "world", "nvidia"),
    ("GOOGL.O", "알파벳", "us", "world", "google 구글"),
    ("AMZN.O", "아마존", "us", "world", "amazon"),
    ("TSLA.O", "테슬라", "us", "world", "tesla"),
    ("META.O", "메타", "us", "world", "facebook 페이스북"),
    ("AVGO.O", "브로드컴", "us", "world", "broadcom"),
    ("NFLX.O", "넷플릭스", "us", "world", "netflix"),
    ("AMD.O", "AMD", "us", "world", ""),
    ("INTC.O", "인텔", "us", "world", "intel"),
    ("QCOM.O", "퀄컴", "us", "world", "qualcomm"),
    ("MU.O", "마이크론", "us", "world", "micron"),
    ("PLTR.O", "팔란티어", "us", "world", "palantir"),
    ("COIN.O", "코인베이스", "us", "world", "coinbase"),
    ("JPM", "제이피모간체이스", "us", "world", "jp모건 jpmorgan"),
    ("V", "비자", "us", "world", "visa"),
    ("MA", "마스터카드", "us", "world", "mastercard"),
    ("LLY", "일라이릴리", "us", "world", "eli lilly"),
    ("UNH", "유나이티드헬스", "us", "world", "unitedhealth"),
    ("XOM", "엑슨모빌", "us", "world", "exxon"),
    ("WMT.O", "월마트", "us", "world", "walmart"),
    ("KO", "코카콜라", "us", "world", "cocacola coca-cola"),
    ("MCD", "맥도날드", "us", "world", "mcdonalds"),
    ("DIS", "디즈니", "us", "world", "disney"),
    ("BA", "보잉", "us", "world", "boeing"),
    ("ORCL.K", "오라클", "us", "world", "oracle"),
    ("CRM", "세일즈포스", "us", "world", "salesforce"),
    ("IBM", "IBM", "us", "world", ""),
    ("TSM", "TSMC", "us", "world", "티에스엠씨 타이완반도체"),
    // 지수
    ("KOSPI", "코스피", "index", "domestic", "kospi"),
    ("KOSDAQ", "코스닥", "index", "domestic", "kosdaq"),
    ("KPI200", "코스피 200", "index", "domestic", "kospi200"),
    (".DJI", "다우존스", "index", "world", "dow 다우"),
    (".IXIC", "나스닥 종합", "index", "world", "nasdaq"),
    (".INX", "S&P 500", "index", "world", "sp500 snp"),
    (".N225", "니케이 225", "index", "world", "nikkei 닛케이"),
    (".HSI", "항셍", "index", "world", "hangseng"),
    (".SOX", "필라델피아 반도체", "index", "world", "sox 필반"),
    // ETF
    ("069500", "KODEX 200", "etf", "domestic", "코덱스"),
    ("122630", "KODEX 레버리지", "etf", "domestic", "코덱스"),
    (
        "252670",
        "KODEX 200선물인버스2X",
        "etf",
        "domestic",
        "곱버스 인버스",
    ),
    (
        "360750",
        "TIGER 미국S&P500",
        "etf",
        "domestic",
        "타이거 sp500",
    ),
    (
        "133690",
        "TIGER 미국나스닥100",
        "etf",
        "domestic",
        "타이거 nasdaq",
    ),
    (
        "458730",
        "TIGER 미국배당다우존스",
        "etf",
        "domestic",
        "타이거 슈드 schd",
    ),
    ("SPY", "SPDR S&P 500 ETF", "etf", "world", "sp500"),
    ("QQQ.O", "Invesco QQQ", "etf", "world", "나스닥100"),
    (
        "VOO",
        "Vanguard S&P 500 ETF",
        "etf",
        "world",
        "뱅가드 sp500",
    ),
    ("VTI", "Vanguard Total Stock ETF", "etf", "world", "뱅가드"),
    (
        "SCHD.K",
        "Schwab US Dividend ETF",
        "etf",
        "world",
        "슈드 배당 schd",
    ),
    (
        "TQQQ.O",
        "ProShares UltraPro QQQ",
        "etf",
        "world",
        "나스닥 3배",
    ),
    (
        "SOXL.K",
        "Direxion 반도체 Bull 3X",
        "etf",
        "world",
        "반도체 3배 soxl",
    ),
];

/// 종목 마스터를 upsert 시드한다 (기동 시마다 호출해도 안전)
pub fn seed_stock_symbols(c: &Connection) -> db::Result<()> {
    db::transaction(c, |c| {
        for (code, name, kind, market, keywords) in STOCK_SEED {
            db::run(
                c,
                "INSERT INTO stock_symbols (code, name, kind, market, keywords) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT(code) DO UPDATE SET name = excluded.name, kind = excluded.kind,
       market = excluded.market, keywords = excluded.keywords",
                &[P::from(*code), P::from(*name), P::from(*kind), P::from(*market), P::from(*keywords)],
            )?;
        }
        Ok::<(), db::DbError>(())
    })
}
