import cron from "node-cron";
import { SlackClient } from "../core/slack.js";
import { startQueueWorker } from "../core/queue.js";
import { run as emartStock } from "../apps/emart-stock/index.js";

// 상주 데몬: 크론 잡 실행 + 파일 큐 소비(슬랙 발송)를 담당
// 실행: npm run scheduler (상시 실행은 systemd/slack-bot-scheduler.service 사용)

interface Job {
  name: string;
  // 크론 표현식 (분 시 일 월 요일)
  schedule: string;
  task: () => Promise<void>;
}

const jobs: Job[] = [
  // 이마트 재고 확인: 3분마다 (판매가 10만 원 이상 또는 2026-08-22 00:00 KST 이후 자체 중지)
  { name: "emart-stock", schedule: "*/3 * * * *", task: emartStock },
  // 새 앱 추가 시: apps/<앱>/index.ts 에서 run 을 export 하고 여기에 한 줄 등록
];

// 실행 중인 잡 (겹침 실행 방지)
const running = new Set<string>();

async function execute(job: Job): Promise<void> {
  if (running.has(job.name)) {
    console.warn(`[${job.name}] 이전 실행이 아직 진행 중, 건너뜀`);
    return;
  }
  running.add(job.name);
  const start = Date.now();
  try {
    await job.task();
    console.log(`[${job.name}] 완료 (${Date.now() - start}ms)`);
  } catch (err) {
    console.error(`[${job.name}] 실패:`, err instanceof Error ? err.message : err);
  } finally {
    running.delete(job.name);
  }
}

for (const job of jobs) {
  if (!cron.validate(job.schedule)) {
    throw new Error(`[${job.name}] 잘못된 크론 표현식: ${job.schedule}`);
  }
  cron.schedule(job.schedule, () => void execute(job), { timezone: "Asia/Seoul" });
  console.log(`잡 등록: ${job.name} (${job.schedule})`);
}

// 큐 워커 시작: 앱들이 enqueue 한 메시지를 실제로 발송하는 유일한 지점
startQueueWorker(new SlackClient());

console.log(`스케줄러 시작 — ${jobs.length}개 잡 대기 중`);
