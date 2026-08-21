# 🔔 donoti

웹 변경·재고 등을 감시해 Web Push로 알림을 보내는 개인용 PWA. 구조는 [docs/architecture.md](docs/architecture.md), 크롤러 작성 요령은 [docs/crawler-guide.md](docs/crawler-guide.md) 참고.

## 개발

```sh
./dev.sh   # backend(:4646) + frontend(:4647) 동시 기동
           # → http://localhost:4647 접속
```

- 초기 계정: `backend/.env`의 `INITIAL_USERNAME` / `INITIAL_PASSWORD`
- 알림 파이프라인 점검: 홈의 "이 기기에서 알림 받기" → "테스트 알림 보내기"
