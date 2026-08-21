import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

// 크롤링 변경 감지 등에 쓰는 앱별 상태 저장소 (.state/<이름>.json)
const STATE_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..", ".state");

/**
 * 저장된 상태를 읽는다.
 * @param name 앱 이름 (파일명으로 사용)
 * @param fallback 저장된 상태가 없을 때 반환할 기본값
 */
export function loadState<T>(name: string, fallback: T): T {
  try {
    return JSON.parse(readFileSync(resolve(STATE_DIR, `${name}.json`), "utf-8")) as T;
  } catch {
    return fallback;
  }
}

// 상태를 저장한다
export function saveState<T>(name: string, state: T): void {
  mkdirSync(STATE_DIR, { recursive: true });
  writeFileSync(resolve(STATE_DIR, `${name}.json`), JSON.stringify(state, null, 2) + "\n");
}
