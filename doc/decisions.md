# 設計上の曖昧点の決定（後で人間が確認）

- **renote / noteType**: pure renote（text・cw が null）は「投稿」に含める（`includeRenotes` デフォルト true、Misskey の `withRenotes` と一致）。LLM 判別用に `noteType`（`note`/`renote`/`reply`）を追加。
- **cw / nsfw**: 本文 truncation はしない。`cw` と `files[].isNsfw` はそのまま返す。コンテキストサイズは limit で制御。
- **limit clamp**: `1..100`、100 超は clamp。ページネーションは最大 10 ページ（spec の「例: 20ページ」に対し暴走防止のため 10 に決定）。
- **排他境界**: 日付のみ入力は `TZ` の 0 時。境界ちょうど `00:00:00.000` は排他により除外。返却前に `createdAt` を厳密検証。
- **エラー文言**: 解析不能は「具体的な日付（YYYY-MM-DD形式）で再指定してください」。MCP の `isError: true` + LLM 可読テキストで返す（JSON-RPC エラーにはしない）。
- **mock 戦略**: `wiremock`（dev-dependency）。実 Misskey へのネットワークリクエストはしない。
- **ローカルホスト表現**: `users/show` は `null`、`notes/search` は `.`。内部で変換し、LLM には `@username@host`（host 省略時は API URL のホスト）を提示。
- **`@` 正規化**: 先頭 `@` を除去し、`@user@host` は username / host に分割して API に渡す。username 埋め込みホストが明示 `host` 引数より優先。
- **TZ**: spec は必須、design はデフォルト `Asia/Tokyo`。未設定時は `Asia/Tokyo` にフォールバック（design に合わせる）。`chrono-tz` は env を自動参照しないため `TZ` 文字列を明示 parse。
- **ID 境界計算**: 使わない。`users/notes` + `sinceDate`/`untilDate`（epoch ms）を主経路とする。
- **ソート**: API の ID 順に依存せず返却前に `createdAt` 降順でソート。
- **HTTP**: https 限定、redirect なし、タイムアウト 30s、429 は backoff リトライ（最大3回）後に LLM 向けエラー。stdout は JSON-RPC のみ、stderr はログ。token は出さない。
- **allowlist**: `meta`, `users/me`, `users/show`, `users/notes`, `notes/timeline`, `notes/local-timeline`, `notes/global-timeline`。書き込み系は呼ばない。
