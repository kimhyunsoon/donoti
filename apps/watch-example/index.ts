import { pathToFileURL } from "node:url";
import * as cheerio from "cheerio";
import { enqueue } from "../../core/queue.js";
import { fetchText } from "../../core/http.js";
import { loadState, saveState } from "../../core/state.js";

// 크롤링 예시 앱: 페이지 제목이 바뀌면 발송 큐에 알림 등록
// 스케줄러 등록용으로 run 을 export 하며, 단독 실행도 가능 (npm run watch-example)
const APP_NAME = "watch-example";
const TARGET_URL = "https://example.com";

interface State {
  lastTitle: string;
}

export async function run(): Promise<void> {
  const html = await fetchText(TARGET_URL);
  const $ = cheerio.load(html);
  const title = $("title").text().trim();

  const state = loadState<State>(APP_NAME, { lastTitle: "" });
  if (title === state.lastTitle) {
    console.log("변경 없음, 알림 생략");
    return;
  }

  enqueue(APP_NAME, { text: `🔔 페이지 변경 감지\n*${title}*\n${TARGET_URL}` });
  saveState<State>(APP_NAME, { lastTitle: title });
  console.log(`알림 큐 등록 완료: ${title}`);
}

// 파일을 직접 실행했을 때만 동작 (스케줄러에서 import 할 때는 실행되지 않음)
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  run().catch((err: unknown) => {
    console.error(err instanceof Error ? err.message : err);
    process.exit(1);
  });
}
