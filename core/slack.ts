import { requireEnv, optionalEnv } from "./config.js";

// Slack Block Kit 블록 (필요한 형태만 느슨하게 정의)
export interface SlackBlock {
  type: string;
  [key: string]: unknown;
}

export interface SendMessageOptions {
  // 채널명(#general) 또는 채널 ID. 생략 시 SLACK_DEFAULT_CHANNEL 사용
  channel?: string;
  // Block Kit 블록 (있으면 text 는 알림용 fallback 으로 사용됨)
  blocks?: SlackBlock[];
  // 스레드에 답글로 달 때 부모 메시지의 ts
  threadTs?: string;
}

interface SlackApiResponse {
  ok: boolean;
  error?: string;
  ts?: string;
  channel?: string;
}

export interface SendResult {
  // 발송된 메시지의 timestamp (스레드 답글 등에 사용)
  ts: string;
  // 실제 발송된 채널 ID
  channel: string;
}

// 범용 슬랙 메시지 발송 클라이언트
export class SlackClient {
  private readonly token: string;
  private readonly defaultChannel?: string;

  constructor(token?: string, defaultChannel?: string) {
    this.token = token ?? requireEnv("SLACK_BOT_TOKEN");
    this.defaultChannel = defaultChannel ?? optionalEnv("SLACK_DEFAULT_CHANNEL");
  }

  /**
   * 메시지를 발송한다.
   * @param text 메시지 본문 (mrkdwn 지원: *굵게*, _기울임_, `코드` 등)
   * @param options 채널·블록·스레드 옵션
   */
  async send(text: string, options: SendMessageOptions = {}): Promise<SendResult> {
    const channel = options.channel ?? this.defaultChannel;
    if (!channel) {
      throw new Error("채널이 지정되지 않았습니다. options.channel 또는 SLACK_DEFAULT_CHANNEL 을 설정하세요.");
    }

    const res = await fetch("https://slack.com/api/chat.postMessage", {
      method: "POST",
      headers: {
        "Content-Type": "application/json; charset=utf-8",
        Authorization: `Bearer ${this.token}`,
      },
      body: JSON.stringify({
        channel,
        text,
        ...(options.blocks ? { blocks: options.blocks } : {}),
        ...(options.threadTs ? { thread_ts: options.threadTs } : {}),
      }),
    });

    const data = (await res.json()) as SlackApiResponse;
    if (!data.ok) {
      throw new Error(`슬랙 발송 실패: ${data.error ?? "unknown_error"} (channel=${channel})`);
    }
    return { ts: data.ts ?? "", channel: data.channel ?? "" };
  }
}
