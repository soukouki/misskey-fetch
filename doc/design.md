# misskey-fetch 設計図

読み取り専用のMisskey MCPサーバー。Rust（rmcp / serde / reqwest / chrono-tz / schemars）、stdioトランスポート、Docker前提。

## 確定方針

- 言語: Rust。公式SDK Tier 1のrmcp。enum＋パターンマッチでエラー・解析結果を型として表現
- 書き込みエンドポイントは呼ばない（allowlist + コード上排除）
- 環境変数: `MISSKEY_TOKEN`, `MISSKEY_API_URL`, `TZ`（デフォルト `Asia/Tokyo`）。起動時に検証（`MISSKEY_API_URL` は https 限定、リダイレクト追わない）
- LLMには `userId` を渡さない。`name` と `handle`（`@user@host`）のみ提示
- stdio: stdout は JSON-RPC のみ、ログは stderr

## MCPツール

1. `get_timeline(type = home|local|global, default local, limit default 20 / max 100)` → `NoteWithUser[]` `createdAt` 降順
2. `get_user_info(username?, host?)` → `User`。`@`有無を正規化し `@user@host` は username/host に分割。省略時は `users/me`（認証ユーザー）。`host` は `username` 省略時は指定不可。`recentNotes` は `users/notes` (limit 10) の追加呼び出しで埋める
3. `get_user_notes(username?, host?, after?, before?, limit default 100 / max 100)` → `NoteWithUser[]` `createdAt` 降順。`after`/`before` はAND・排他。`after > before` はエラー

## 日付

- 厳格なISO 8601のみ受け付け、サーバーは検証のみ（曖昧解析なし。正規化はLLM側の仕事）。ツールdescriptionにも「ISO 8601で渡す」「相対表現は渡さない」を明記
- 日付のみ入力は `TZ` の0時境界。オフセット付き（Z/+09:00）はそのまま尊重
- 拒否時（非ISO 8601・不正日付）は「YYYY-MM-DD形式で再指定してください」をLLM向けエラーとして返す
- `chrono-tz` は `TZ` 環境変数を自動参照しない。`TZ` 文字列を明示的に parse する（tzdata は chrono-tz 組み込みなので distroless で問題なし）

## 取得戦略

- **主経路: `users/notes` + `sinceDate`/`untilDate`（epoch ms）**。ID境界の自前計算はしない。ID生成方式は `aid|aidx|meid|meidg|ulid|objectid` でインスタンス設定依存（「先頭4文字＝ミリ秒」は aid/aidx/meid/meidg のみ、ulid は10文字、objectid は4バイト）
- 境界は排他。`gen(ms)` 由来の境界IDは同一ミリ秒のノートと必ずしも一致しないため、**APIで粗く絞り込み、返ってきた件数を `createdAt` で厳密検証して範囲外を落とす**
- `users/notes` を主経路にする理由: `text`/`cw` が null のノート（メディアのみ・renote）を拾える、自分の限定公開を拾える、Meilisearch scope `local` で remote が落ちない
- `/notes/search` は全文検索を明示的に要求した場合のみ（任意機能）。制約をREADMEに明記: text/cw null 漏れ、public/home のみ索引、scope local、`rangeStartAt`/`rangeEndAt` は `note.id > gen(...)` のID比較で remote ノートでは機能しない
- `canSearchNotes` は role policy（ユーザー単位）で `meta.disableSearchNotes` とは別物。フラグ分岐ではなく `UNAVAILABLE` を受けてフォールバック
- ページネーション: 1リクエスト100件上限。100超は100件ずつ遡及。停止条件 = 空ページ / 境界到達 / 最大ページ数（例20）
- remote ユーザー: `users/notes` はID順。remote のID方式が違うとID順≠時系列順。既知の制限としてREADME明記
- `limit` は100でclamp（APIに100超を渡さない）
- `createdAt` 降順で返す（APIのID順に依存しない）

## 構成

```
src/
  main.rs       # サーバー起動・ツール登録・環境変数検証
  tools/        # 3ツールのハンドラ
  misskey/      # APIクライアント（allowlist）、Note/User/NoteWithUser型（serde）
  dates/        # ISO 8601検証、TZ境界、epoch ms 境界
  errors.rs     # LLM向けエラー（NO_SUCH_USER / FAILED_TO_RESOLVE_REMOTE_USER / UNAVAILABLE / 不正日付 / 429）
```

## HTTP

- reqwest: タイムアウト設定、`redirect_policy = none`、https限定
- 429 は backoff リトライまたはLLM向けエラー
- token をログ・エラーメッセージに出さない

## Docker

multi-stageビルド → distroless（非root）。TOKENはイメージに焼かず実行時env注入。`TZ` も実行時注入（デフォルト `Asia/Tokyo`）。

## テスト

- 単体: 日付検証（許容/拒否ケース表）、TZ境界（`Asia/Tokyo` と `UTC` でずれること）、`@`正規化と username/host 分割、排他境界（0時ちょうどが除外されること）
- 結合: モックMisskeyに対するツールハンドラ（`users/notes` の `sinceDate`/`untilDate` が渡されていること、`createdAt` 再検証で範囲外が落ちること）
- 読み取り専用保証: allowlist外（書き込みエンドポイント）を呼ばないことの検証
- limit clamp（100超 → 100）、ページネーション停止条件、429 の backoff
