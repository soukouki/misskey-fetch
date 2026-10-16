# 設計上の曖昧点の決定

spec.md は常時最新の事実だけを保持する（変更履歴は持たない。履歴は git が追う）。このファイルは決定の根拠と未決事項を記録する。

## 確定済み

### 決定6: model の直積構造と view layer

- **state space は直積であって直和ではない**。`noteType`（`note`/`renote`/`reply`）のような排他的分類（partition）は model に存在しない。Misskey の実際の分類は `PureRenote ⊎ Content` の 2 値のみ（`MkNote.vue` が使う `isPureRenote`）。`isQuotePacked` は `renoteId != null` の OR 条件に `replyId != null` を含むため、Quote と Reply は交差する（`Quote ∩ Reply ≠ ∅`）。優先順位は model の事実ではなく決定であり、決定として保持するならこのファイルに書く。
- **generator と derived の区別**: 独立 axis になれるのは generator だけ。`reactionCount`（`Σ reactions`）、`createdAt`（ID から導出）、`hasPoll`、`hasCw`、`isHidden`、`user`、`reply`、`renote`、`channel`、`files`、`poll` は derived。derived を独立 field として model に持たせない。
- **`hasCw` は redundant**: `cw != null` と同値。view から落とした。
- **`isHidden` は viewer 相対の relation**: `shouldHideNote(packed, meId)` は `meId` に依存する。note の property ではない。Misskey 自身も `// TODO: hiddenReason` で不足を認めている。bool の `isHidden` は当面保持し、`hiddenReason` は将来追加される（置き換えではない）。
- **`reply` / `renote` は 1 つの axis**: 「relation が存在する」ことと「relation の対象が応答に含まれる」ことは別の層。`RelationView` の 4 状態（`none` / `notJoined` / `notFound` / `present`）で表す。`notJoined` は「model に relation があるが detail gate が entity を応答から除外した」ことを意味する。
- **入れ子 note の detail gate 非対称**: `reply` の入れ子は `detail: false`（`poll` / `myReaction` / `clippedCount` が shape に入らない）、`renote` の入れ子は `detail: true`。`reply` に `poll` がないのは model の制約ではなく API 側の packing 選択（`notes/create` の paramDef の `if` 条件に `replyId` はない）。`Option<Poll>` は「応答に含まれない」を正直に表す。
- **relation の深さ limit は 10**: 打ち切られた位置は `notJoined` で表し、専用の flag field（`renoteDepthLimited`）は持たない。model に cycle はありえない（renote の対象は作成時点で存在する必要がある）。
- **invariant**: 「entity present ⇒ FK present」は `RelationView` の構造で保証できる。「FK present ⇒ entity present」は detail gate により破れるため `notFound` が必要。invariant は型ではなく documentation / 実行時 check。
- **custom presence type（`Field<T>` / `Optional<T>` / `Nullable<T>`）は使わない**。`Option<T>` のみ。理由: custom Serialize/Deserialize が必要、`schemars` が derive できない、LLM は absent と null を区別できない、「never null」は model の property ではなく実装の事実。`Relation<T>` は presence type ではなく domain type（Misskey の JOIN semantics）として保持する。
- **`RawUser` を `UserLite` と `UserDetailed` に分割**: 両者は別の shape。単一の `Option` field で混ぜると、lite shape に存在しない field が偽の `null` になる。`users/show` は常に `schema: 'UserDetailed'`。
- **view から落とした model field**: `userId`（LLM に見せない）、`emojis` / `reactionEmojis`（描画 data）、`reactionCount`（derived）、`deletedAt`（schema にあるが `pack()` が出さない）、`reactionAndUserPairCache`（opt-in flag が必要で flag を立てる endpoint がない）、`clippedCount`、`myReaction`、`avatarDecorations`。
- **closed enum は `Unknown` variant を持たない**: 再起動で回収できるシステムでは overkill。value set の drift は解析失敗で落ちる。
- **`name` の衝突**: `MiNote.name`（varchar 256, nullable）はノートのタイトルであり packed output にはない。output の `name` は `UserLite.name`（nullable）。同じ名前で意味が違うため両方は使えない。view の `name` はユーザー名として解像した値のみ。
- **canonicalization の非対称**: 入力の `@alice` は内部で `UserRef{alice, None}`（ローカル）になり、出力の `handle` は `@alice@{instance_host}` になる。同じ文字列ではない。2周目以降は round-trip で安定する。
- **host 正規化**: lookup path は lowercase + punycode（`RemoteUserResolveService.resolveUser` と同じ）。display path は stored 値を使う。`instance_host` は punycode で保存する。
- **`text` は pack 時に書き換えられる**: `note.name && (note.url ?? note.uri)` の条件で本文が組み立てられる。stored 値と output 値が異なる。
- **`uri` / `url` の矛盾**: api-doc は required + non-nullable、`pack()` は local で `?? undefined`。実装（api-doc）に従う。
- **`channel` の schema shape が不正**: api-doc の required set に従う。

### 決定1〜5（既存）

- **renote / noteType**: pure renote（text・cw・files・poll がなし）は「投稿」に含める（`includeRenotes` デフォルト true、Misskey の `withRenotes` と一致）。LLM 判別用に `noteType`（`note`/`renote`/`reply`）を追加。判定優先順位は **renote > reply > note**（引用リポストは renote 側）。Misskey の `isPureRenote` は「renote があり、reply がなく、text・cw・files・poll がなし」。
- **cw / nsfw**: 本文 truncation はしない。`cw`（警告文、UIで常時表示）と `text`（折りたまれた本文）をそのまま返す。`files[].isSensitive` はそのまま返す。コンテキストサイズは limit で制御。
- **limit clamp**: 要求 `limit` は **1..1000** に clamp。1リクエストの page size は 100（Misskey API の上限）。ページネーションは最大 **10 ページ**（= 1000件）。省略が発生したら `truncated` / `nextCursor` / `message`（英語）で LLM に伝える。
- **排他境界**: 日付のみ入力は `TZ` の 0 時。境界ちょうど `00:00:00.000` は排他により除外。返却前に `createdAt` を厳密検証。
- **ISO 8601 の許容範囲**: 規格準拠で常用記法から遠くない入力は許容。時刻のオフセットは**省略可**（省略時は `TZ` の壁時計時刻として解釈）。小数秒は任意桁数。拒否: 月日1桁 (`2024-2-3`)、不正日付 (`2024-02-31`)、範囲外 (`25:00:00`, `12:60:00`, leap second `:60`)、slash、相対表現、basic format (`20240203`)、週日付、space 区切り、reduced precision (`T12:34`)。
- **DST 曖昧時**: `latest()`（DST 移行後のオフセット）。`Asia/Tokyo` では差が出ない。
- **エラー文言**: LLM 向けはすべて英語。解析不能時は「具体的な日付（YYYY-MM-DD形式）で再指定してください」の英語版。MCP の `isError: true` + LLM 可読テキストで返す（JSON-RPC エラーにはしない）。
- **`createdAt` が解析不能になる経路**: Misskey は `idService.parse(note.id).date.toISOString()` で返すため、準拠応答で壊れた文字列は届かない。非準拠実装（Iceshrimp / Firefish / Mastodon 互換）はスコープ外。解析失敗は応答全体を失敗させる（個々のノートを黙って skip しない）。防御分岐は削除。
- **TZ**: **必須**。未設定は起動エラー（デフォルト `Asia/Tokyo` は廃止）。`chrono-tz` は env を自動参照しないため `TZ` 文字列を明示 parse。
- **ID 境界計算**: 使わない。`users/notes` + `sinceDate`/`untilDate`（epoch ms）を主経路とする。Misskey 側も `gen(ms)` → ID 比較であり、同一ミリ秒の保証がなく、remote ノートは別形式のIDで保存されるため厳密なタイムスタンプ絞り込みはできない。返却前検証が必要。
- **ソート**: API の ID 順に依存せず返却前に `createdAt` 降順でソート。
- **HTTP**: https 限定、redirect なし、タイムアウト 30s、429 は**指数 backoff**（200ms × 2^n = 200/400/800、最大3リトライ）後に LLM 向けエラー。stdout は JSON-RPC のみ、stderr はログ。token は出さない。
- **allowlist**: 実行時の allowlist 判定は持たない（呼ぶエンドポイントが決まっているため無意味）。読み取り専用の保証はコード構造とテストで検証。`meta` は不要と判断し削除。
- **mock 戦略**: `wiremock`（dev-dependency）。実 Misskey へのネットワークリクエストはしない。
- **ローカルホスト表現**: `users/show` の**応答**では `null`（JSON の null。パラメータの省略ではない）、`notes/search` の**パラメータ**では `.`。内部で「ローカルユーザー」という概念に統一し、LLM には `@username@host`（host 省略時は API URL のホスト）を提示。
- **`@user@host` 正規化（決定5）**: 入出力とも `@user@host` 形式に統一し、ユーザー指定の引数は `handle` のみ（`username` / `host` 引数は撤去）。値は `@` で始まっていなければならない（`@` なしはエラー）。`@username@host` は username / host に分割して API に渡す（`username` フィールドに `@user@host` をそのまま渡してはいけない）。host 省略（`@username`）はインスタンスホスト補完（= ローカルユーザー）。解析した host がインスタンスホストと一致する場合はローカルに正規化する（`users/show` には `null`）。内部表現は構造化 `UserRef { username, host }`（`None` = ローカル）で、表示用文字列から生成しない。表示は常に `@username@{API_URL のホスト}` の完全形。
- **デフォルト引数**: `withReplies` false、`withRenotes` true、`allowPartial` false（Misskey の paramDef と一致。Misskey 側コメントは true を推奨）。

## 未決

- **テストで未カバー**: 429 backoff、`FAILED_TO_RESOLVE_REMOTE_USER`、`UNAVAILABLE` フォールバック、DST 曖昧。追加するかは未決。
- **`pinnedNotes` と `recentNotes`**: `pinnedNotes` は model の field（`UserDetailed`）だが `recentNotes` は model の field ではなく view layer の組み立て（`users/notes` の追加呼び出し、limit 10）。この非対称を受け入れるか未決。
