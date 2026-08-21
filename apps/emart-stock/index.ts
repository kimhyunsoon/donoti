import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import * as cheerio from "cheerio";
import { enqueue } from "../../core/queue.js";
import { loadState, saveState } from "../../core/state.js";

// 이마트 스위치2 프로컨트롤러 재고 확인 앱 (다지점)
// 페이지가 Vue SPA 라 headless chromium 으로 렌더링 후 #ern-bottom 의 2번째 자식 버튼 텍스트를 확인.
// '다 팔렸어요!' 가 아닌 지점이 있으면 매번 슬랙 알림.
// 중지 조건 (이 앱만 중지, 데몬은 유지): ① 판매가 10만 원 이상 ② 2026-08-22 00:00 (KST) 도달
const APP_NAME = "emart-stock";
const SHARE_URL = "https://emart.kr/1gUk7";
const SOLD_OUT_TEXT = "다 팔렸어요!";
const PRICE_STOP_THRESHOLD = 100_000;
// 2026-08-22 00:00 KST
const DEADLINE = new Date("2026-08-21T15:00:00Z");
const USER_AGENT =
  "Mozilla/5.0 (Linux; Android 13; SM-G991B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0 Mobile Safari/537.36";

interface Store {
  name: string;
  url: string;
}

const STORES: Store[] = [
  {
    name: "구로점",
    url: "https://eapp.emart.com/webapp/digital/view/4902370552850?storeCode=3400&trcknCode=snsshare",
  },
  {
    name: "성남점",
    url: "https://eapp.emart.com/webapp/digital/view/4902370552850?storeCode=1560&trcknCode=snsshare&storeType=E",
  },
  {
    name: "일렉트로마트 스타필드 하남점",
    url: "https://eapp.emart.com/webapp/digital/view/4902370552850?storeCode=3031&trcknCode=snsshare&storeType=E",
  },
  {
    name: "이마트 하남점",
    url: "https://eapp.emart.com/webapp/digital/view/4902370552850?storeCode=1360&trcknCode=snsshare&storeType=E",
  },
  {
    name: "이마트 명일점",
    url: "https://eapp.emart.com/webapp/digital/view/4902370552850?storeCode=7300&trcknCode=snsshare&storeType=E",
  },
  {
    name: "이마트 천호점",
    url: "https://eapp.emart.com/webapp/digital/view/4902370552850?storeCode=3700&trcknCode=snsshare&storeType=E",
  },
  {
    name: "이마트 분당점",
    url: "https://eapp.emart.com/webapp/digital/view/4902370552850?storeCode=2500&trcknCode=snsshare&storeType=E",
  },
];

const execFileAsync = promisify(execFile);

interface State {
  // 가격 조건으로 중지되었으면 true (이후 동작 안 함)
  stopped: boolean;
  // 지점별 파싱 실패 경고 발송 여부 (중복 경고 방지)
  warned: Record<string, boolean>;
}

// headless chromium 으로 렌더링된 DOM 을 가져온다
async function fetchRenderedHtml(url: string): Promise<string> {
  const { stdout } = await execFileAsync(
    "chromium",
    [
      "--headless=new",
      "--disable-gpu",
      "--no-sandbox",
      `--user-data-dir=${tmpdir()}/emart-stock-chrome`,
      "--virtual-time-budget=15000",
      `--user-agent=${USER_AGENT}`,
      "--dump-dom",
      url,
    ],
    { timeout: 60_000, maxBuffer: 20 * 1024 * 1024 },
  );
  return stdout;
}

interface StoreResult {
  store: Store;
  buttonText: string;
  priceText: string;
  price: number;
}

// 한 지점의 버튼 텍스트와 판매가를 확인한다
async function checkStore(store: Store): Promise<StoreResult> {
  const html = await fetchRenderedHtml(store.url);
  const $ = cheerio.load(html);
  const priceText = $(".final-price > .price > .value").first().text().trim();
  const buttonText = $("#ern-bottom").children().eq(1).text().trim();
  return { store, buttonText, priceText, price: Number(priceText.replace(/,/g, "")) };
}

export async function run(): Promise<void> {
  const state = loadState<State>(APP_NAME, { stopped: false, warned: {} });
  if (state.stopped) {
    console.log(`[${APP_NAME}] 가격 조건으로 중지된 상태`);
    return;
  }
  if (Date.now() >= DEADLINE.getTime()) {
    console.log(`[${APP_NAME}] 마감(2026-08-22 00:00 KST) 경과, 중지 상태`);
    return;
  }

  // chromium 프로필 공유 때문에 지점별 순차 확인
  const available: StoreResult[] = [];
  for (const store of STORES) {
    const result = await checkStore(store);
    console.log(`[${APP_NAME}] ${store.name}: 버튼 "${result.buttonText}", 판매가 ${result.priceText || "?"}원`);

    // 중지 조건 ①: 판매가 10만 원 이상
    if (result.priceText && Number.isFinite(result.price) && result.price >= PRICE_STOP_THRESHOLD) {
      saveState<State>(APP_NAME, { ...state, stopped: true });
      enqueue(APP_NAME, {
        text: `🛑 재고 감시 중지: ${store.name} 판매가 ${result.priceText}원 (10만 원 이상)\n${SHARE_URL}`,
      });
      console.log(`[${APP_NAME}] ${store.name} 판매가 ≥ 10만 원, 앱 중지`);
      return;
    }

    if (!result.buttonText) {
      console.error(`[${APP_NAME}] ${store.name}: 버튼 텍스트를 찾지 못함 (페이지 구조 변경 가능성)`);
      if (!state.warned[store.name]) {
        enqueue(APP_NAME, {
          text: `⚠️ 재고 확인 실패 (${store.name}): #ern-bottom 버튼을 찾지 못했습니다. 페이지 구조를 확인하세요.\n${store.url}`,
        });
        state.warned[store.name] = true;
        saveState<State>(APP_NAME, state);
      }
      continue;
    }

    if (result.buttonText !== SOLD_OUT_TEXT) available.push(result);
  }

  if (!available.length) return;

  const lines = available.map(
    (r) => `• ${r.store.name}: "${r.buttonText}" / 판매가 ${r.priceText || "?"}원\n${r.store.url}`,
  );
  enqueue(APP_NAME, {
    text: `🎮 *스위치2 프로컨트롤러 재고 확인!*\n${lines.join("\n")}\n${SHARE_URL}`,
  });
  console.log(`[${APP_NAME}] 재고 알림 발송 (${available.map((r) => r.store.name).join(", ")})`);
}

// 단독 실행 (테스트): npm run emart-stock
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  run().catch((err: unknown) => {
    console.error(err instanceof Error ? err.message : err);
    process.exit(1);
  });
}
