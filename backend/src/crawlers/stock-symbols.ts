import type { Database } from 'better-sqlite3';

// 주식 종목 마스터 시드 - 유명 종목만. 코드는 네이버 증권 기준
// (국내 6자리, 나스닥 .O 접미사, NYSE 무접미사, 해외지수 . 접두사)

export interface StockSymbol {
  code: string;
  name: string;
  kind: 'kr' | 'us' | 'index' | 'etf';
  market: 'domestic' | 'world';
  keywords: string;
}

const S = (
  code: string,
  name: string,
  kind: StockSymbol['kind'],
  market: StockSymbol['market'],
  keywords = '',
): StockSymbol => ({ code, name, kind, market, keywords });

export const STOCK_SEED: StockSymbol[] = [
  // 한국주식
  S('005930', '삼성전자', 'kr', 'domestic', 'samsung'),
  S('000660', 'SK하이닉스', 'kr', 'domestic', 'hynix'),
  S('373220', 'LG에너지솔루션', 'kr', 'domestic', '엔솔'),
  S('207940', '삼성바이오로직스', 'kr', 'domestic', ''),
  S('005380', '현대차', 'kr', 'domestic', '현대자동차 hyundai'),
  S('000270', '기아', 'kr', 'domestic', 'kia'),
  S('035420', 'NAVER', 'kr', 'domestic', '네이버'),
  S('035720', '카카오', 'kr', 'domestic', 'kakao'),
  S('068270', '셀트리온', 'kr', 'domestic', ''),
  S('005490', 'POSCO홀딩스', 'kr', 'domestic', '포스코'),
  S('051910', 'LG화학', 'kr', 'domestic', ''),
  S('006400', '삼성SDI', 'kr', 'domestic', ''),
  S('105560', 'KB금융', 'kr', 'domestic', ''),
  S('055550', '신한지주', 'kr', 'domestic', ''),
  S('086790', '하나금융지주', 'kr', 'domestic', ''),
  S('012450', '한화에어로스페이스', 'kr', 'domestic', ''),
  S('042660', '한화오션', 'kr', 'domestic', ''),
  S('329180', 'HD현대중공업', 'kr', 'domestic', ''),
  S('028260', '삼성물산', 'kr', 'domestic', ''),
  S('015760', '한국전력', 'kr', 'domestic', '한전'),
  S('032830', '삼성생명', 'kr', 'domestic', ''),
  S('000810', '삼성화재', 'kr', 'domestic', ''),
  S('259960', '크래프톤', 'kr', 'domestic', 'krafton'),
  S('036570', '엔씨소프트', 'kr', 'domestic', 'nc'),
  S('352820', '하이브', 'kr', 'domestic', 'hybe'),
  S('090430', '아모레퍼시픽', 'kr', 'domestic', ''),
  S('097950', 'CJ제일제당', 'kr', 'domestic', ''),
  // 미국주식
  S('AAPL.O', '애플', 'us', 'world', 'apple'),
  S('MSFT.O', '마이크로소프트', 'us', 'world', 'microsoft'),
  S('NVDA.O', '엔비디아', 'us', 'world', 'nvidia'),
  S('GOOGL.O', '알파벳', 'us', 'world', 'google 구글'),
  S('AMZN.O', '아마존', 'us', 'world', 'amazon'),
  S('TSLA.O', '테슬라', 'us', 'world', 'tesla'),
  S('META.O', '메타', 'us', 'world', 'facebook 페이스북'),
  S('AVGO.O', '브로드컴', 'us', 'world', 'broadcom'),
  S('NFLX.O', '넷플릭스', 'us', 'world', 'netflix'),
  S('AMD.O', 'AMD', 'us', 'world', ''),
  S('INTC.O', '인텔', 'us', 'world', 'intel'),
  S('QCOM.O', '퀄컴', 'us', 'world', 'qualcomm'),
  S('MU.O', '마이크론', 'us', 'world', 'micron'),
  S('PLTR.O', '팔란티어', 'us', 'world', 'palantir'),
  S('COIN.O', '코인베이스', 'us', 'world', 'coinbase'),
  S('JPM', '제이피모간체이스', 'us', 'world', 'jp모건 jpmorgan'),
  S('V', '비자', 'us', 'world', 'visa'),
  S('MA', '마스터카드', 'us', 'world', 'mastercard'),
  S('LLY', '일라이릴리', 'us', 'world', 'eli lilly'),
  S('UNH', '유나이티드헬스', 'us', 'world', 'unitedhealth'),
  S('XOM', '엑슨모빌', 'us', 'world', 'exxon'),
  S('WMT.O', '월마트', 'us', 'world', 'walmart'),
  S('KO', '코카콜라', 'us', 'world', 'cocacola coca-cola'),
  S('MCD', '맥도날드', 'us', 'world', 'mcdonalds'),
  S('DIS', '디즈니', 'us', 'world', 'disney'),
  S('BA', '보잉', 'us', 'world', 'boeing'),
  S('ORCL.K', '오라클', 'us', 'world', 'oracle'),
  S('CRM', '세일즈포스', 'us', 'world', 'salesforce'),
  S('IBM', 'IBM', 'us', 'world', ''),
  S('TSM', 'TSMC', 'us', 'world', '티에스엠씨 타이완반도체'),
  // 지수
  S('KOSPI', '코스피', 'index', 'domestic', 'kospi'),
  S('KOSDAQ', '코스닥', 'index', 'domestic', 'kosdaq'),
  S('KPI200', '코스피 200', 'index', 'domestic', 'kospi200'),
  S('.DJI', '다우존스', 'index', 'world', 'dow 다우'),
  S('.IXIC', '나스닥 종합', 'index', 'world', 'nasdaq'),
  S('.INX', 'S&P 500', 'index', 'world', 'sp500 snp'),
  S('.N225', '니케이 225', 'index', 'world', 'nikkei 닛케이'),
  S('.HSI', '항셍', 'index', 'world', 'hangseng'),
  S('.SOX', '필라델피아 반도체', 'index', 'world', 'sox 필반'),
  // ETF
  S('069500', 'KODEX 200', 'etf', 'domestic', '코덱스'),
  S('122630', 'KODEX 레버리지', 'etf', 'domestic', '코덱스'),
  S('252670', 'KODEX 200선물인버스2X', 'etf', 'domestic', '곱버스 인버스'),
  S('360750', 'TIGER 미국S&P500', 'etf', 'domestic', '타이거 sp500'),
  S('133690', 'TIGER 미국나스닥100', 'etf', 'domestic', '타이거 nasdaq'),
  S('458730', 'TIGER 미국배당다우존스', 'etf', 'domestic', '타이거 슈드 schd'),
  S('SPY', 'SPDR S&P 500 ETF', 'etf', 'world', 'sp500'),
  S('QQQ.O', 'Invesco QQQ', 'etf', 'world', '나스닥100'),
  S('VOO', 'Vanguard S&P 500 ETF', 'etf', 'world', '뱅가드 sp500'),
  S('VTI', 'Vanguard Total Stock ETF', 'etf', 'world', '뱅가드'),
  S('SCHD.K', 'Schwab US Dividend ETF', 'etf', 'world', '슈드 배당 schd'),
  S('TQQQ.O', 'ProShares UltraPro QQQ', 'etf', 'world', '나스닥 3배'),
  S('SOXL.K', 'Direxion 반도체 Bull 3X', 'etf', 'world', '반도체 3배 soxl'),
];

/** 종목 마스터를 upsert 시드한다 (기동 시마다 호출해도 안전) */
export function seedStockSymbols(db: Database): void {
  const upsert = db.prepare(
    `INSERT INTO stock_symbols (code, name, kind, market, keywords) VALUES (?, ?, ?, ?, ?)
     ON CONFLICT(code) DO UPDATE SET name = excluded.name, kind = excluded.kind,
       market = excluded.market, keywords = excluded.keywords`,
  );
  const run = db.transaction((rows: StockSymbol[]) => {
    for (const row of rows) upsert.run(row.code, row.name, row.kind, row.market, row.keywords);
  });
  run(STOCK_SEED);
}
