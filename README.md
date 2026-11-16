# 🔔 donoti

웹 변경·재고 등을 감시해 Web Push로 알림을 보내는 개인용 PWA. 구조는 [docs/architecture.md](docs/architecture.md), 크롤러 작성 요령은 [docs/crawler-guide.md](docs/crawler-guide.md) 참고.

## 개발

```sh
./dev.sh   # backend(Rust :4646) + frontend(:4647) 동시 기동 (Rust 툴체인 필요, backend 만은 backend/dev.sh)
           # → http://localhost:4647 접속
```

- 초기 계정: `backend/.env`의 `INITIAL_USERNAME` / `INITIAL_PASSWORD`
- 알림 파이프라인 점검: 홈의 "이 기기에서 알림 받기" → "테스트 알림 보내기"

## 로고

`assets/donoti-mark.svg`가 마크 원본(단색 벡터). 아이콘·파비콘 규칙과 재생성 명령은 dogroo [docs/icon-guide.md](../dogroo/docs/icon-guide.md) 를 따른다.

```sh
python3 ../dogroo/assets/compose-icons.py assets/donoti-mark.svg '#fed3cc' assets
for s in 180 192 512; do rsvg-convert -w $s assets/icon.svg -o frontend/public/icons/icon-$s.png; done
rsvg-convert -w 512 assets/icon-maskable.svg -o frontend/public/icons/icon-512-maskable.png
```
