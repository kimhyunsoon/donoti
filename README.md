# cansoon-slack-bot

개인 슬랙 워크스페이스로 메시지를 보내는 프로그램 모음. 파일 큐를 중심으로 동작하며, Arch 노트북에서 상주 데몬이 크론 잡 실행과 큐 소비(발송)를 담당한다.

## 동작 방식

```
생산자                              큐 (파일)                 소비자 (데몬)
─────────────────────────          ─────────────────         ──────────────────
크론 잡 (apps/*, run 함수)  ──┐
CLI (npm run enqueue)       ──┼──► queue/pending/*.json ──► 큐 워커 → 슬랙 발송
외부 스크립트 (파일 직접 생성)──┘         │
                                        ├── 성공 → 파일 삭제
                                        └── 실패 → 백오프 재시도 (최대 5회)
                                                    → 소진 시 queue/failed/ 이동
```

- 발송은 데몬의 큐 워커가 유일하게 담당. 앱들은 `enqueue()` 만 호출한다.
- 네트워크 장애·데몬 재시작에도 메시지는 파일로 남아 유실되지 않는다.
- 재시도 백오프: 10초 → 1분 → 5분 → 15분. 최종 실패는 `queue/failed/` 에서 확인.

## 구조

```
cansoon-slack-bot/
├── core/                  # 범용 모듈
│   ├── config.ts          #   .env 로딩 및 환경변수 유틸
│   ├── slack.ts           #   SlackClient (chat.postMessage 래퍼)
│   ├── queue.ts           #   파일 큐: enqueue / startQueueWorker
│   ├── http.ts            #   크롤링용 fetchText / fetchJson
│   └── state.ts           #   변경 감지용 상태 저장 (.state/<앱>.json)
├── apps/                  # 용도별 앱
│   ├── hello/             #   예시: 즉시 발송 (큐 우회, 토큰 테스트용)
│   ├── enqueue/           #   CLI 로 큐에 메시지 넣기
│   ├── watch-example/     #   예시: 페이지 변경 감지 → 큐 등록
│   └── emart-stock/       #   이마트 재고 감시 (headless chromium 크롤링)
├── scheduler/index.ts     # 상주 데몬: 크론 잡 + 큐 워커
├── queue/                 # 파일 큐 (pending / failed / tmp) — git 제외
└── systemd/               # 데몬 상시 실행용 유저 서비스
```

## 설정

```bash
cp .env.example .env   # SLACK_BOT_TOKEN, SLACK_DEFAULT_CHANNEL 입력
npm install
```

## 실행

```bash
# 데몬 포그라운드 실행 (크론 잡 + 큐 워커)
npm run scheduler

# 큐에 메시지 넣기 (데몬이 발송)
npm run enqueue -- "보낼 메시지" ["#채널"]

# 즉시 발송 (큐 우회, 토큰 테스트용)
npm run hello -- "보낼 메시지" ["#채널"]

# 크론 잡 단독 실행 (테스트)
npm run watch-example
```

외부 스크립트에서 발송하려면 아래 형식의 JSON 을 `queue/pending/` 에 넣으면 된다 (파일명은 `<epoch ms>-<아무 문자열>.json`):

```json
{
  "id": "임의 고유값",
  "source": "my-script",
  "message": { "text": "보낼 메시지", "channel": "#alerts" },
  "createdAt": "2026-08-20T12:00:00.000Z",
  "attempts": 0,
  "nextAttemptAt": "2026-08-20T12:00:00.000Z"
}
```

## 상시 실행 (systemd 유저 서비스)

```bash
mkdir -p ~/.config/systemd/user
cp systemd/slack-bot-scheduler.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now slack-bot-scheduler

# 상태·로그 확인
systemctl --user status slack-bot-scheduler
journalctl --user -u slack-bot-scheduler -f
```

로그아웃 상태에서도 유지하려면: `loginctl enable-linger $USER`

## 새 앱 추가하는 법

1. `apps/<용도>/index.ts` 를 만들고 `run(): Promise<void>` 를 export. 발송은 `enqueue()` 사용.
2. 주기 실행이 필요하면 `scheduler/index.ts` 의 `jobs` 배열에 한 줄 추가:

```ts
{ name: "my-app", schedule: "0 9 * * *", task: myApp },  // 매일 9시
```

3. `systemctl --user restart slack-bot-scheduler` 로 반영

## 작업 가이드 (에이전트용)

이 프로젝트를 수정·확장하는 에이전트를 위한 작업 절차와 과거 작업에서 배운 것들.

### 핵심 규칙

- **데몬은 코드를 시작 시점에 로드한다.** 파일을 수정해도 `systemctl --user restart slack-bot-scheduler` 전까지는 옛 코드로 돈다. 이를 역이용하면: 앱을 수정하고 `npm run <앱>` 으로 단독 실행하면 새 코드를 데몬과 무관하게 테스트할 수 있다 (enqueue 된 메시지는 돌고 있는 데몬 워커가 발송해주므로 전체 파이프라인 검증이 된다).
- **슬랙 발송 지점은 데몬의 큐 워커 하나뿐.** 앱은 `enqueue()` 만 호출한다. `apps/hello` 만 예외적으로 즉시 발송 (토큰 테스트용).
- 앱별 영속 상태는 `.state/<앱이름>.json` (`core/state.ts`). 앱 상태를 초기화하려면 해당 파일을 지우면 된다.
- 재고 감시류 앱의 "중지" 는 데몬 중지가 아니라 앱 스스로 조기 return 하는 것 (state 플래그 또는 마감 시각 체크). 데몬(부모 프로세스)은 계속 돈다.

### 배포·확인 절차

```bash
# 1. 코드 수정 후 단독 실행으로 검증
npm run <앱이름>

# 2. 데몬에 반영
systemctl --user restart slack-bot-scheduler

# 3. 로그로 크론 실행·큐 발송 확인 ("[queue] 발송 완료" 가 뜨면 슬랙 도착)
journalctl --user -u slack-bot-scheduler -f

# 발송 실패 잡 확인
ls queue/failed/
```

systemd 서비스 파일(`systemd/slack-bot-scheduler.service`)을 수정했다면 `~/.config/systemd/user/` 에 복사 후 `systemctl --user daemon-reload` 필요.

### 크롤링 앱 작성 시 디버깅 순서

emart-stock 작업에서 확립한 절차. 셀렉터를 코드에 넣기 전에 반드시 실제 응답에서 확인할 것.

1. **정적 fetch 먼저**: `curl -sL -A "<모바일 UA>" <URL>` 로 받아서 목표 셀렉터가 초기 HTML 에 있는지 `grep`. 있으면 `core/http.ts` 의 `fetchText` + cheerio 로 충분.
2. **단축링크·브릿지 페이지 주의**: 예컨대 `emart.kr/*` 단축링크는 앱 유도용 브릿지 페이지라 본문이 없다. HTML 안의 리다이렉트 스크립트에서 실제 URL 을 찾아 그 URL 을 크롤링 대상으로 쓴다.
3. **SPA 면 headless chromium**: 셀렉터가 초기 HTML 에 없으면 (Vue/React 마운트 지점 `<div id="webapp">` 등만 있으면) JS 렌더링이 필요하다:
   ```bash
   chromium --headless=new --disable-gpu --no-sandbox \
     --user-data-dir=/tmp/앱이름-chrome --virtual-time-budget=15000 \
     --user-agent="<모바일 UA>" --dump-dom "<URL>" > rendered.html
   ```
   렌더링된 DOM 에서 셀렉터와 실제 텍스트를 확인한 뒤 코드를 작성한다. 코드에서는 `apps/emart-stock/index.ts` 의 `fetchRenderedHtml()` 패턴(execFile + cheerio)을 재사용. 실측 1~2초/회라 1분 주기도 문제없다.
4. **파싱 실패 대비**: 페이지 구조가 바뀌어 셀렉터를 못 찾으면 조용히 실패하지 말고 슬랙으로 1회 경고를 보낸다 (state 의 `warned` 플래그로 중복 방지). emart-stock 참고.

### 테스트 발송 요령

실제 알림 조건을 인위적으로 만들기 어려울 때 (예: 품절 상품의 재고 알림), 조건을 임시로 반전시켜 메시지가 큐를 거쳐 슬랙까지 도착하는지 확인한 뒤 원복한다. 반전 코드가 데몬에 반영된 동안은 매 주기 메시지가 오는 것이 정상이므로, 확인 후 바로 원복 + 데몬 재시작할 것.

### 자주 만나는 에러

| 증상 | 원인 |
|---|---|
| `invalid_auth` | `.env` 의 `SLACK_BOT_TOKEN` 오타·만료 |
| `not_in_channel` | 채널에 봇 미초대 (`/invite @봇이름`) |
| `channel_not_found` | 채널명 오타 또는 비공개 채널 |
| 큐에 쌓이는데 발송 안 됨 | 데몬 미가동 — `systemctl --user status slack-bot-scheduler` |

## core 사용법

```ts
import { enqueue } from "../../core/queue.js";
import { fetchText, fetchJson } from "../../core/http.js";
import { loadState, saveState } from "../../core/state.js";
import * as cheerio from "cheerio";

// 발송 큐에 등록 (채널 생략 시 기본 채널로 발송됨)
enqueue("my-app", { text: "메시지", channel: "#alerts" });

// Block Kit
enqueue("my-app", {
  text: "fallback 텍스트",
  blocks: [{ type: "section", text: { type: "mrkdwn", text: "*굵은* 메시지" } }],
});

// 크롤링: HTML 파싱
const $ = cheerio.load(await fetchText("https://example.com"));
const title = $("title").text();

// 크롤링: JSON API
const data = await fetchJson<{ items: string[] }>("https://api.example.com/items");

// 변경 감지 (마지막 값을 .state/ 에 저장)
const prev = loadState<string>("my-app", "");
if (title !== prev) {
  enqueue("my-app", { text: `변경됨: ${title}` });
  saveState("my-app", title);
}
```
