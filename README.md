# misskey-fetch

読み取り専用の Misskey MCP サーバー（Rust / rmcp / reqwest / chrono-tz / schemars、stdio トランスポート）。

## 実行

```sh
export MISSKEY_TOKEN=...            # read:account のみ
export MISSKEY_API_URL=https://misskey.io/api
export TZ=Asia/Tokyo                # 省略時デフォルト
docker run -i --rm -e MISSKEY_TOKEN -e MISSKEY_API_URL -e TZ ghcr.io/.../misskey-fetch
```

stdout は JSON-RPC のみ、ログは stderr。

## ツール

- `get_timeline(type=home|local|global, limit<=100)` → `NoteWithUser[]`（createdAt 降順）
- `get_user_info(username?, host?)` → `User`（`@user@host` は username/host に分割、省略時は `users/me`）
- `get_user_notes(username?, host?, after?, before?, limit<=100, includeRenotes)` → `NoteWithUser[]`

日付は厳格な ISO 8601 のみ（`YYYY-MM-DD`、任意で時刻＋オフセット）。曖昧・相対表現（`now`, `-1d`, `2024-2-3`）は拒否し「具体的な日付（YYYY-MM-DD形式）で再指定してください」を返す。

## 既知の制限

- **検索経路は定義的に不完全**（主経路は `users/notes` + `sinceDate`/`untilDate`）。`/notes/search`（任意機能）は `text == null && cw == null` のノート（メディアのみ・pure renote）を索引しない、`public`/`home` しか索引しない、Meilisearch の scope が `local`（デフォルト）なら remote ユーザーのノートが落ちる。
- `rangeStartAt`/`rangeEndAt` は `note.id > gen(ms)` の ID 比較。ID 生成方式は `aid|aidx|meid|meidg|ulid|objectid` でインスタンス設定依存（先頭4文字=ミリ秒は aid/aidx/meid/meidg のみ、ulid は10文字、objectid は4バイト）のため、remote ノートでは日付境界が機能しない。
- **排他境界**: `after`/`before` は排他。日付のみ入力は `TZ` の 0 時境界であり、境界ちょうど（`00:00:00.000`）のノートは除外される。API で粗く絞り込み、返却前に `createdAt` を厳密検証して範囲外を落とす。
- **renote semantics**: pure renote（text/cw が null）は「投稿」として返す（`includeRenotes` デフォルト true）。LLM が判別できるよう `noteType`（`note`/`renote`/`reply`）を付与する。著者はノートごとに解決する。
- **cw / nsfw**: 本文 truncation はしない。`cw`・`files[].isNsfw` はそのまま返す（コンテキストサイズは limit で制御）。100 ノートの JSON は LLM コンテキストに対して大きい。
- **muting / blocking / channel muting**: Misskey サーバー側で適用され、適用された結果が返る。
- **visibility**: `users/notes` は認証ユーザーから見える範囲（自分の `followers`/`specified` を含む）を返す。タイムラインは公開範囲のみ。
- remote ユーザーの `users/notes` は ID 順で返るため、remote の ID 方式が異なると ID 順 ≠ 時系列順。遡及ページネーションは確実ではない。
- `limit` は 100 で clamp（100 超は API に渡さない）。ページネーションは最大 10 ページ（暴走防止）。
- `canSearchNotes` は role policy（ユーザー単位）であり `meta.disableSearchNotes` とは別物。フラグ分岐せず `UNAVAILABLE` を受けてフォールバック。
- `MISSKEY_API_URL` は https 限定、リダイレクトを追わない（token の別 origin 漏洩防止）。token はログ・エラーメッセージに出さない。
