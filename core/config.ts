import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

// 프로젝트 루트의 .env 파일을 읽어 process.env 에 주입 (이미 설정된 값은 유지)
function loadEnv(): void {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const envPath = resolve(root, ".env");
  let raw: string;
  try {
    raw = readFileSync(envPath, "utf-8");
  } catch {
    return;
  }
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq === -1) continue;
    const key = trimmed.slice(0, eq).trim();
    const value = trimmed.slice(eq + 1).trim();
    if (!(key in process.env)) process.env[key] = value;
  }
}

loadEnv();

// 필수 환경변수를 가져오고, 없으면 즉시 에러를 던진다
export function requireEnv(key: string): string {
  const value = process.env[key];
  if (!value) {
    throw new Error(`환경변수 ${key} 가 설정되지 않았습니다. .env 파일을 확인하세요.`);
  }
  return value;
}

// 선택 환경변수 (없으면 undefined)
export function optionalEnv(key: string): string | undefined {
  return process.env[key];
}
