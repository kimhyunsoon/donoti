import { readFileSync, writeFileSync, mkdirSync, readdirSync, renameSync, unlinkSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import { SlackClient, type SlackBlock } from "./slack.js";

// 파일 기반 발송 큐
// queue/pending/  대기 중인 잡 (파일명 = 생성시각 접두라 FIFO)
// queue/failed/   재시도 소진 후 실패한 잡 (수동 확인용)
// queue/tmp/      작성 중인 파일 (완성 후 pending 으로 rename → 워커가 미완성 파일을 읽는 일 방지)

const QUEUE_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..", "queue");
const PENDING_DIR = resolve(QUEUE_DIR, "pending");
const FAILED_DIR = resolve(QUEUE_DIR, "failed");
const TMP_DIR = resolve(QUEUE_DIR, "tmp");

const MAX_ATTEMPTS = 5;
// 재시도 백오프: 10초 → 1분 → 5분 → 15분
const BACKOFF_MS = [10_000, 60_000, 300_000, 900_000];

export interface QueueMessage {
  text: string;
  channel?: string;
  blocks?: SlackBlock[];
  threadTs?: string;
}

interface QueueJob {
  id: string;
  // 어느 앱이 넣었는지
  source: string;
  message: QueueMessage;
  createdAt: string;
  attempts: number;
  // 이 시각 이후에만 발송 시도 (ISO)
  nextAttemptAt: string;
  lastError?: string;
}

function ensureDirs(): void {
  for (const dir of [PENDING_DIR, FAILED_DIR, TMP_DIR]) {
    mkdirSync(dir, { recursive: true });
  }
}

/**
 * 발송 큐에 메시지를 넣는다.
 * @param source 넣는 주체 (앱 이름)
 * @param message 발송할 메시지
 * @returns 잡 id
 */
export function enqueue(source: string, message: QueueMessage): string {
  ensureDirs();
  const now = new Date();
  const job: QueueJob = {
    id: randomUUID(),
    source,
    message,
    createdAt: now.toISOString(),
    attempts: 0,
    nextAttemptAt: now.toISOString(),
  };
  const filename = `${now.getTime()}-${job.id.slice(0, 8)}.json`;
  const tmpPath = resolve(TMP_DIR, filename);
  writeFileSync(tmpPath, JSON.stringify(job, null, 2) + "\n");
  renameSync(tmpPath, resolve(PENDING_DIR, filename));
  return job.id;
}

// 잡 하나를 발송 시도하고 파일을 정리한다
async function processJob(slack: SlackClient, filename: string): Promise<void> {
  const path = resolve(PENDING_DIR, filename);
  let job: QueueJob;
  try {
    job = JSON.parse(readFileSync(path, "utf-8")) as QueueJob;
  } catch {
    // 파싱 불가 파일은 failed 로 격리
    renameSync(path, resolve(FAILED_DIR, filename));
    console.error(`[queue] 잘못된 잡 파일 격리: ${filename}`);
    return;
  }

  if (new Date(job.nextAttemptAt).getTime() > Date.now()) return;

  try {
    await slack.send(job.message.text, {
      channel: job.message.channel,
      blocks: job.message.blocks,
      threadTs: job.message.threadTs,
    });
    unlinkSync(path);
    console.log(`[queue] 발송 완료: ${job.source} (${filename})`);
  } catch (err) {
    job.attempts += 1;
    job.lastError = err instanceof Error ? err.message : String(err);
    if (job.attempts >= MAX_ATTEMPTS) {
      writeFileSync(path, JSON.stringify(job, null, 2) + "\n");
      renameSync(path, resolve(FAILED_DIR, filename));
      console.error(`[queue] 최종 실패 → failed/ 이동: ${job.source} — ${job.lastError}`);
    } else {
      const backoff = BACKOFF_MS[Math.min(job.attempts - 1, BACKOFF_MS.length - 1)];
      job.nextAttemptAt = new Date(Date.now() + backoff).toISOString();
      writeFileSync(path, JSON.stringify(job, null, 2) + "\n");
      console.warn(`[queue] 발송 실패 (${job.attempts}/${MAX_ATTEMPTS}), ${backoff / 1000}초 후 재시도: ${job.lastError}`);
    }
  }
}

/**
 * 큐 워커를 시작한다. pending 을 주기적으로 FIFO 소비해 슬랙으로 발송.
 * @param slack 발송에 사용할 클라이언트
 * @param intervalMs 폴링 주기 (기본 5초)
 * @returns 워커 중지 함수
 */
export function startQueueWorker(slack: SlackClient, intervalMs = 5_000): () => void {
  ensureDirs();
  let busy = false;

  const tick = async (): Promise<void> => {
    if (busy) return;
    busy = true;
    try {
      const files = readdirSync(PENDING_DIR).filter((f) => f.endsWith(".json")).sort();
      for (const file of files) {
        await processJob(slack, file);
      }
    } finally {
      busy = false;
    }
  };

  const timer = setInterval(() => void tick(), intervalMs);
  void tick();
  console.log(`[queue] 워커 시작 (폴링 ${intervalMs / 1000}초)`);
  return (): void => clearInterval(timer);
}
