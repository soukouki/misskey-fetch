# misskey-fetch 設計図

読み取り専用のMisskey MCPサーバー。Rust（rmcp / serde / reqwest / chrono-tz / schemars）、stdioトランスポート、Docker前提。

## 確定方針

- 言語: Rust。公式SDK Tier 1のrmcp。enum＋パターンマッチでエラー・解析結果を型として表現
- 書き込みエンドポイントは呼ばない（コード上排除）
- 環境変数: `MISSKEY_TOKEN`, `MISSKEY_API_URL`, `TZ`（デフォルト `Asia/Tokyo`）。起動時に検証
- LLMには `userId` を渡さない。`name` と `handle`（`@user@host`）のみ提示

## MCPツール

1. `get_timeline(type = home|local|global, default local)` → `NoteWithUser[]` 日時降順
2. `get_user_info(username?, host?)` → `User`。`@`有無を正規化。省略時は認証ユーザー。`host` は `username` 省略時は指定不可
3. `get_user_notes(username?, host?, after?, before?, limit=100)` → `Note[]` 日時降順。`after`/`before` はAND・排他

## 日付

- 厳格なISO 8601のみ受け付け、サーバーは検証のみ（曖昧解析なし。正規化はLLM側の仕事）
- 日付のみ入力は `TZ` の0時境界。オフセット付き（Z/+09:00）はそのまま尊重
- 拒否時（非ISO 8601・不正日付）は「YYYY-MM-DD形式で再指定してください」をLLM向けエラーとして返す

## 取得戦略

- 日付範囲指定時は `/notes/search` を `query=""` で呼ぶ（paramDefのrequiredは文字列の存在なので空文字は通過し、sqlLikeは `LIKE '%%'`、Meilisearchは空クエリで全件になる）
- 日付からID境界（Crockford base32、先頭4文字＝ミリ秒タイムスタンプ）を計算し `sinceId`/`untilId` として渡す。サーバー側でフィルタリング完結
- フォールバック: `canSearchNotes` 無効（UNAVAILABLE）時、`users/notes` ページネーション＋クライアント側日付フィルタで遡及取得
- 留意点: `text` がnullのノート（メディアのみ・renoteのみ）は検索経路で漏れる。Meilisearchスコープlocalのインスタンスではremote検索が狭くなる。READMEに明記

## 構成

```
src/
  main.rs       # サーバー起動・ツール登録
  tools/        # 3ツールのハンドラ
  misskey/      # APIクライアント、Note/User/NoteWithUser型（serde）
  dates/        # ISO 8601検証、TZ境界、ID境界計算
  errors.rs     # LLM向けエラー
```

## Docker

multi-stageビルド → distroless（非root）。TOKENはイメージに焼かず実行時env注入。

## テスト

- 単体: 日付検証（許容/拒否）、ID境界計算、`@`正規化、TZ境界
- 結合: モックMisskeyに対するツールハンドラ
- 読み取り専用保証: 書き込みエンドポイントを呼ばないことの検証
