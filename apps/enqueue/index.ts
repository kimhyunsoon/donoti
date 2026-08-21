import { enqueue } from "../../core/queue.js";

// CLI 로 큐에 메시지를 넣는 앱 (발송은 데몬의 큐 워커가 담당)
// 실행: npm run enqueue -- "보낼 메시지" ["#채널"]
function main(): void {
  const [text, channel] = process.argv.slice(2);
  if (!text) {
    console.error('사용법: npm run enqueue -- "보낼 메시지" ["#채널"]');
    process.exit(1);
  }
  const id = enqueue("enqueue-cli", { text, channel });
  console.log(`큐에 등록됨: ${id} (데몬이 발송 처리)`);
}

main();
