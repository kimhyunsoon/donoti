import { SlackClient } from "../../core/slack.js";

// 사용 예시 앱: 인사 메시지 발송
// 실행: npm run hello -- "보낼 메시지" [#채널]
async function main(): Promise<void> {
  const [message, channel] = process.argv.slice(2);
  const slack = new SlackClient();

  const result = await slack.send(message ?? "안녕하세요! cansoon-bot 테스트 메시지입니다. 👋", {
    channel,
  });
  console.log(`발송 완료: channel=${result.channel}, ts=${result.ts}`);
}

main().catch((err: unknown) => {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
});
