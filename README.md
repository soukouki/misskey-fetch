# misskey-fetch

読み取り専用の Misskey MCP サーバー（Rust / rmcp / reqwest / chrono-tz / schemars、stdio トランスポート）。

## 実行

```sh
export MISSKEY_TOKEN=...            # read:account のみ
export MISSKEY_API_URL=https://misskey.io/api
export TZ=Asia/Tokyo                # 必須（未設定は起動エラー）
docker run -i --rm -e MISSKEY_TOKEN -e MISSKEY_API_URL -e TZ ghcr.io/.../misskey-fetch
```

stdout は JSON-RPC のみ、ログは stderr。

## ツール

- `get_timeline(type=home|local|global, limit<=1000)` → `NotesPage`（`notes` は createdAt 降順）
- `get_user_info(handle?)` → `User`（`handle` は `@username` または `@username@host`。`@` で始める必要がある（`@` なしはエラー）。host 省略はインスタンスホスト補完（= ローカルユーザー）。省略時は `users/me`）
- `get_user_notes(handle?, after?, before?, limit<=1000, includeRenotes)` → `NotesPage`（`handle` は `get_user_info` と同じ規則）

`NotesPage` は `notes` / `truncated` / `nextCursor` / `message`。省略が発生した場合のみ `message`（英語）に「表示件数」と「続きの取得方法（`untilId`）」を書く。

日付は厳格な ISO 8601 のみ（`YYYY-MM-DD`、任意で時刻）。オフセットは省略可で、省略時は `TZ` の壁時計時刻として解釈する。曖昧・相対表現（`now`, `-1d`, `2024-2-3`）、basic format（`20240203`）、週日付、space 区切り、reduced precision は拒否し、英語で「具体的な日付（YYYY-MM-DD形式）で再指定してください」を返す。

## 既知の制限

- **検索経路は定義的に不完全**（主経路は `users/notes` + `sinceDate`/`untilDate`）。`/notes/search`（任意機能）は `text == null && cw == null` のノート（メディアのみ・pure renote）を索引しない、`public`/`home` しか索引しない、Meilisearch の scope が `local`（デフォルト）なら remote ユーザーのノートが落ちる。
- `rangeStartAt`/`rangeEndAt` は `note.id > gen(ms)` の ID 比較。ID 生成方式は `aid|aidx|meid|meidg|ulid|objectid` でインスタンス設定依存（先頭4文字=ミリ秒は aid/aidx/meid/meidg のみ、ulid は10文字、objectid は4バイト）のため、remote ノートでは日付境界が機能しない。API 側の `sinceDate`/`untilDate` も同じ ID 比較であり、同一ミリ秒のノートが境界を通過する保証はない。
- **排他境界**: `after`/`before` は排他。日付のみ入力は `TZ` の 0 時境界であり、境界ちょうど（`00:00:00.000`）のノートは除外される。API で粗く絞り込み、返却前に `createdAt` を厳密検証して範囲外を落とす。DST で曖昧・非存在の壁時計時刻は `latest()`（DST 移行後のオフセット）で解決する。
- **`createdAt` の信頼性**: Misskey は `createdAt` を DB カラムではなくノート ID から導出する（`idService.parse(id).date`）。remote インスタンスが別形式の ID（Mastodon snowflake 等）を持つ場合、導出される日付が正しくないことがある（解析は通るが値がゴミ）。
- **renote semantics**: pure renote（text・cw・files・poll がなし）は「投稿」として返す（`includeRenotes` デフォルト true）。LLM が判別できるよう `noteType`（`note`/`renote`/`reply`、優先順位 renote > reply）を付与する。著者はノートごとに解決する。入れ子 renote は深さ 10 で打ち切り、`renoteDepthLimited` を true にする。
- **cw / nsfw**: 本文 truncation はしない。`cw` は**警告文**（UI で常時表示）、`text` はその**隠された本文**（UI で折りたたまれる）であり、両方をそのまま返す。`hasCw` は「UI では cw だけが見えている」ことを示す。`isHidden` はポリシー由来の隠蔽（`hideNote()` は text/cw を null にするため、cw なしでは pure renote と区別できない）。`files[].isNsfw` はそのまま返す。コンテキストサイズは limit で制御。1000 ノートの JSON は LLM コンテキストに対して大きい。
- **muting / blocking / channel muting**: Misskey サーバー側で適用され、適用された結果が返る。
- **visibility**: `users/notes` は認証ユーザーから見える範囲（自分の `followers`/`specified` を含む）を返す。タイムラインは公開範囲のみ。
- remote ユーザーの `users/notes` は ID 順で返るため、remote の ID 方式が異なると ID 順 ≠ 時系列順。遡及ページネーションは確実ではない。
- 要求 `limit` は 1000 で clamp（1リクエストの page size は 100、Misskey API の上限）。ページネーションは最大 10 ページ（暴走防止）。省略分は `truncated` / `nextCursor` / `message` で伝える。
- `canSearchNotes` は role policy（ユーザー単位）であり `meta.disableSearchNotes` とは別物。フラグ分岐せず `UNAVAILABLE` を受けてフォールバック。
- `MISSKEY_API_URL` は https 限定、リダイレクトを追わない（token の別 origin 漏洩防止）。token はログ・エラーメッセージに出さない。429 は指数 backoff（200ms × 2^n、最大3リトライ）。
