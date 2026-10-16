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

- `get_timeline(type=home|local|global, limit<=1000)` → `Page`（`notes` は createdAt 降順）
- `get_user_info(handle?)` → `UserViewDetailed`（ページラッパーなし）
- `get_user_notes(handle?, after?, before?, limit<=1000, includeRenotes)` → `Page`

`handle` は `@username` または `@username@host`。`@` で始める必要がある（`@` なしはエラー）。host 省略は `MISSKEY_API_URL` のホストで補完（= ローカルユーザー）。省略時は `users/me`。

`Page` は `notes` / `truncated` / `nextCursor` / `message`。省略が発生した場合のみ `message`（英語）に「表示件数」と「続きの取得方法（`untilId`）」を書く。

日付は ISO 8601（`YYYY-MM-DD`、任意で時刻）。オフセットは省略可で、省略時は `TZ` の壁時計時刻として解釈する。曖昧・相対表現（`now`, `-1d`, `2024-2-3`）、basic format（`20240203`）、週日付、space 区切り、reduced precision は拒否し、英語で「具体的な日付（YYYY-MM-DD形式）で再指定してください」を返す。

## 出力 shape（view layer）

- `NoteView`: `id`, `createdAt`, `user`, `text`, `cw`, `visibility`, `isHidden`, `poll`, `files`, `channel`, `reply`, `renote`
- `UserView`: `name`, `handle`, `isBot`（`userId` は LLM に見せない）
- `FileView`: `name`, `mime`, `size`, `isSensitive`, `url`, `comment`
- `PollView`: `expiresAt`, `multiple`, `choices[{text, votes}]`
- `ChannelView`: `name`, `isSensitive`
- `RelationView`（`reply` / `renote`）: `none` / `notJoined{id}` / `notFound{id}` / `present{id, note}`

`cw` は**警告文**（UI で常時表示）、`text` はその**隠された本文**（UI で折りたたまれる）。`isHidden` は viewer 相対の relation（`shouldHideNote(packed, meId)`）であり、note の property ではない。

## 既知の制限

- **検索経路は定義的に不完全**（主経路は `users/notes` + `sinceDate`/`untilDate`）。`/notes/search`（任意機能）は `text == null && cw == null` のノート（メディアのみ・pure renote）を索引しない、`public`/`home` しか索引しない、Meilisearch の scope が `local`（デフォルト）なら remote ユーザーのノートが落ちる。
- `sinceDate`/`untilDate` は Misskey 側で `note.id > gen(ms)` の ID 比較。ID 生成方式は `aid|aidx|meid|meidg|ulid|objectid` でインスタンス設定依存のため、remote ノートでは日付境界が機能しないことがある。同一ミリ秒のノートが境界を通過する保証はない。
- **排他境界**: `after`/`before` は排他。日付のみ入力は `TZ` の 0 時境界であり、境界ちょうど（`00:00:00.000`）のノートは除外される。API で粗く絞り込み、返却前に `createdAt` を厳密検証して範囲外を落とす。DST で曖昧・非存在の壁時計時刻は `latest()`（DST 移行後のオフセット）で解決する。
- **`createdAt` の信頼性**: Misskey は `createdAt` を DB カラムではなくノート ID から導出する（`idService.parse(id).date`）。非準拠実装（Mastodon snowflake 等）はスコープ外。解析に失敗した場合は応答全体を失敗させる（個々のノートを黙って skip しない）。
- **view は model の mirror ではない**。model の直積構造を保持し、`noteType` のような排他的分類（partition）は持たない。Misskey の実際の分類は `PureRenote ⊎ Content` の 2 値のみ。
- **closed enum の drift**: `Visibility`（4値）と `RelationView`（4値）は model の value set を写した closed enum であり、`Unknown` variant は持たない。Misskey が値を追加した場合（例: `isHidden` → `hiddenReason`）は解析失敗で落ちる。再起動で回収できるシステムではこれが許容範囲という判断。
- **view から落とした model field**: `userId`（LLM に見せない）、`emojis` / `reactionEmojis`（描画 data）、`reactionCount`（`Σ reactions` から derived）、`deletedAt`（schema にあるが `pack()` が出さない）、`reactionAndUserPairCache`（opt-in flag が必要で flag を立てる endpoint がない）、`clippedCount`、`myReaction`、`avatarDecorations`、`isBot`/`onlineStatus` 以外の user field。
- **`name` の衝突**: `MiNote.name`（varchar 256, nullable）はノートのタイトルであり packed output にはない。output の `name` は `UserLite.name`（nullable）であり、同じ名前で意味が違う。両方は使えないため view の `name` はユーザー名として解像した値のみ。
- **canonicalization の非対称**: 入力の `@alice` は内部で `UserRef{alice, None}`（ローカル）になり、出力の `handle` は `@alice@{instance_host}` になる。同じ文字列ではない。2周目以降は round-trip で安定する。
- **host 正規化**: lookup path は lowercase + punycode（`RemoteUserResolveService.resolveUser` と同じ）。display path は stored 値を使う。`instance_host` は punycode で保存する。
- **`text` は pack 時に書き換えられる**: `note.name && (note.url ?? note.uri)` の条件で本文が組み立てられる。stored 値と output 値が異なる。
- **入れ子 note の detail gate 非対称**: `reply` の入れ子は `detail: false`（`poll` / `myReaction` / `clippedCount` が shape に入らない）、`renote` の入れ子は `detail: true`。`reply` に `poll` がないのは model の制約ではなく API 側の packing 選択（`replyId + poll` のノートは作れる）。
- **relation の深さ limit は 10**。打ち切られた位置は `notJoined` で表し、専用の flag field は持たない。model に cycle はありえない（renote の対象は作成時点で存在する必要がある）。
- **invariant は型ではなく実行時 check**: 「entity present ⇒ FK present」は `RelationView` の構造で保証できるが、「FK present ⇒ entity present」は detail gate により破れるため `notFound` が必要。
- **muting / blocking / channel muting**: Misskey サーバー側で適用され、適用された結果が返る。
- **visibility**: `users/notes` は認証ユーザーから見える範囲（自分の `followers`/`specified` を含む）を返す。タイムラインは公開範囲のみ。`treatVisibility` は `public`/`home` を policy で `followers` に書き換える（`makeNotesFollowersOnlyBefore`）。
- remote ユーザーの `users/notes` は ID 順で返るため、remote の ID 方式が異なると ID 順 ≠ 時系列順。返却前に `createdAt` 降順に再ソートする。
- 要求 `limit` は 1000 で clamp（1リクエストの page size は 100、Misskey API の上限）。ページネーションは最大 10 ページ（暴走防止）。省略分は `truncated` / `nextCursor` / `message` で伝える。
- `canSearchNotes` は role policy（ユーザー単位）であり `meta.disableSearchNotes` とは別物。フラグ分岐せず `UNAVAILABLE` を受けてフォールバック。
- `MISSKEY_API_URL` は https 限定、リダイレクトを追わない（token の別 origin 漏洩防止）。token はログ・エラーメッセージに出さない。429 は指数 backoff（200ms × 2^n、最大3リトライ）。
