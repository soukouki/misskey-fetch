# Misskey-fetch MCPサーバー 仕様書

## 1. プロジェクト概要
本プロジェクトは、Misskey APIを利用してノート（投稿）やユーザー情報を取得し、LLM（Large Language Model）がそれらの情報を参照できるようにするためのModel Context Protocol (MCP) サーバーを実装する。主に、認証済みユーザーの投稿履歴の抽出、特定ユーザーの追跡、およびタイムラインの取得を目的とする。

## 2. 機能要件
### 2.1 取得対象データ
- **タイムライン**: ホーム、ローカル、グローバルタイムラインの最新投稿。
- **ユーザー情報**: 特定のユーザーのプロフィール情報。
- **ノート一覧**:
    - 認証済みユーザー（自分）の投稿履歴。
    - 特定のユーザーの投稿履歴。
    - 特定の日付・期間における投稿の抽出。

### 2.2 実装範囲
- **提供機能**: 読み取り専用（Read-only）。投稿作成やユーザーデータの変更機能は含まない。
- **データ形式**: ノートのテキストおよびメタデータ（作成日時、ユーザーIDなど）を返す。画像データやメディアファイルのバイナリ処理は行わない。
- **ID隠蔽**: 内部的な `userId` 等のIDはサーバー側で解決し、LLMにはユーザー名やハンドルなど、人間が解釈可能な識別子で提示する。LLMが解釈できる識別子は `name` と `handle` のみであり、`userId` は view に含めない。
- **全文検索**: 本文検索は本仕様の対象外（必要時のみ任意機能として扱う。6.1参照）。

## 3. アーキテクチャ
本サーバーはMCP（Model Context Protocol）仕様に基づき、JSON-RPCを介してLLMにツールを提供し、内部的にMisskey REST APIを呼び出すプロキシとして動作する。

データは2層で扱う。

1. **内部表現**（4.1）: Misskey API の応答をそのまま受ける型。model の shape を反映する。
2. **view**（4.2）: LLM に返す型。`userId` を除き、`handle` を文字列として持つ。view layer の関数は `instance_host` を引数として取る。

## 4. データモデル

### 4.1 内部表現（API 応答の型）

**Note**
- `id`: ノートID（Misskey の `misskey:id`。LLM に渡す）
- `createdAt`: 作成日時。Misskey は `idService.parse(id).date.toISOString()` から導出する。compliant な Misskey では常に ISO 8601 UTC。解析不能なら応答全体を失敗させる（防御分岐を持たない）
- `user`: `UserLite`（required + non-nullable。pack は `user` を常に emit する）
- `text`: 本文（nullable）。pack 時に `note.name && (note.url ?? note.uri)` の条件で書き換えられることがある（stored 値と view 値が異なる）
- `cw`: 警告文（nullable）。UI で常時表示される。`text` はその折りたまれた本文
- `visibility`: `public` | `home` | `followers` | `specified`。pack 時の effective visibility（`treatVisibility` が user policy で public/home を followers に書き換えることがある）
- `isHidden`: viewer-relative の relation（`shouldHideNote(packed, meId)` は meId に依存する）。ノート固有の property ではない
- `poll`: detail gate 内
- `files`: `DriveFile[]`
- `channel`: conditional（`channel ? {...} : undefined`）
- `replyId` / `renoteId`: **常に pack される**（detail gate がない）
- `reply` / `renote`: detail gate 内（4.3）

**UserLite**
- `id`, `name`(nullable), `username`, `host`(nullable), `isBot`

**UserDetailed** = UserLite + `description`(nullable) + `pinnedNotes`
- `users/show` は常に `schema: 'UserDetailed'` で pack するため `pinnedNotes` は常に存在する（optional ではない）
- note の著者は常に `UserLite` であり `description`/`pinnedNotes` を持たない
- この2つの shape は別型として持つ。1つの struct に潰すと「detailed だが bio が空」と「lite shape では field が存在しない」を区別できず、存在しなかった field が `null` として LLM に返る

**DriveFile**
- `name`, `type`(mime), `size`, `isSensitive`, `url`, `comment`(nullable)

**Poll**
- `expiresAt`(nullable), `multiple`, `choices[{text, votes, isVoted}]`

**Channel**
- `name`, `isSensitive`

**型で表現しないもの（model にない、または view に不要）**
- `noteType`: product から導出される projection であり partition ではない。Misskey の actual partition は `PureRenote ⊎ Content` の2分のみ。`isQuotePacked` の disjunct に `replyId != null` があるため Quote ∩ Reply ≠ ∅。判定優先順位は decision であり model の事実ではない
- `hasCw`: `cw != null` と同値（redundant）
- `reactionCount`: `Σ reactions` から導出
- `emojis` / `reactionEmojis`: LLM に不要（drawing data）
- `deletedAt`: api-doc に宣言があるが `pack()` が emit しない
- `reactionAndUserPairCache`: opt-in flag が必要で、それを set する endpoint がない

**detail gate**: `clippedCount`, `reply`, `renote`, `poll`, `myReaction` は `detail: true` のときだけ shape に含まれる。
- `users/notes` / timeline の top-level note は `detail: true`
- 入れ子 `reply` は `detail: false`、入れ子 `renote` は `detail: true` で pack される（非対称）
- detail gate は **API 側の選択**であり model の制約ではない。`replyId` set + `poll` set + `text` null のノートは `notes/create` の paramDef で作成できる（`if` 条件に `replyId` がない）。`hasPoll` は stored column なので model は「この reply に poll がある」ことを保持し、lite packing がそれを落としているだけ

**invariant**: entity present ⇒ FK present。これは型ではなく実行時 check で検証する（drift はエラー）。

### 4.2 view（MCP 出力）

**NoteView**
- `id`, `createdAt`, `user`(UserView), `text`, `cw`, `visibility`, `isHidden`, `poll`, `files`, `channel`, `reply`(Relation), `renote`(Relation)

**UserView**
- `name`(nullable), `handle`, `isBot`

**UserViewDetailed** = UserView + `description` + `pinnedNotes` + `recentNotes`
- `pinnedNotes` は model の field（`UserDetailed`）。`recentNotes` は model の field ではなく view layer の組み立てであり、`users/notes`（limit 10）の追加呼び出しで埋める

**FileView**: `name`, `mime`, `size`, `isSensitive`, `url`, `comment`
**PollView**: `expiresAt`, `multiple`, `choices[{text, votes}]`（`isVoted` は view に含めない）
**ChannelView**: `name`, `isSensitive`

view に含めないもの: `userId`, `noteType`, `hasCw`, `renoteDepthLimited`, `reactionCount`, `emojis`, `deletedAt`, `reactionAndUserPairCache`, `clippedCount`, `myReaction`, `visibleUserIds`, `tags`, `mentions`, `uri`, `url`, `localOnly`, `reactionAcceptance`, `renoteCount`, `repliesCount`, `fileIds`, `isVoted`。

### 4.3 Relation（4状態）

`reply` / `renote` は relation として1軸で持つ（`reply` と `replyId` は独立 field ではない。invariant: entity present ⇒ FK present）。

- `none` — FK が null（relation がない）
- `notJoined` — FK はあるが entity が応答に含まれていない（detail gate、または depth 上限で打ち切り）
- `notFound` — FK はあるが entity が null（対象が削除済み）
- `present` — entity が存在する

JSON は明示的な tagged structure:

```json
{"state":"none"}
{"state":"notJoined","id":"01..."}
{"state":"notFound","id":"01..."}
{"state":"present","id":"01...","note":{...}}
```

**3状態の collapse**: serde は key absent と null を区別できない（`Option<Option<T>>` でも同一になる）。`notJoined` / `notFound` の区別は detail gate の位置から導出する — その note が `detail: false` で pack された（入れ子 reply）なら entity の不在は `notJoined`、`detail: true` なら `notFound`。depth 上限での打ち切りも `notJoined` とする（「応答に含まれなかった」の同一意味）。専用の flag field は持たない。

cycle は model で不可能（renote の対象は renote 作成時点で存在しなければならない）。depth 上限（10）は hostile な instance に対する size policy であり model の性質ではない。

### 4.4 ハンドルの正規化

- 入力は `@username` または `@username@host`。`@` で始まらない入力はエラー。
- host 省略（`@username`）はインスタンスホスト補完（= ローカルユーザー）。
- 解析した host がインスタンスホストと一致する場合はローカルに正規化する（`users/show` には `null`）。Misskey はローカルユーザーを `host: null` で保存するため、自ホストを remote host として渡すと失敗する。
- **正規化は lookup path だけ**: username は lowercase、host は lowercase + punycode（`RemoteUserResolveService.resolveUser` は `username.toLowerCase()` と `utilityService.toPuny(host)` を使う。`host === toPuny(config.host)` で local と判定する）。
- **display path は API が返した stored 値を使う**: remote user の username は case を保存する。input の case を display に反映させない。
- 内部表現（`UserRef { username, host }`）と出力表現（canonical `handle` 文字列）は別物である。`UserRef` には handle 文字列も `id` もない。
- round-trip: `parse_handle("@user")` → `UserRef{user, None}` → `handle_of` → `@user@{instance_host}`。1回目は input と output が異なる文字列になる。2回目以降は不動点。この非対称は仕様である。

## 5. インターフェース仕様 (MCPツール)

### 5.1 `get_timeline`
タイムラインの最新ノートを取得する。
- **引数**:
    - `type`: `home` | `local` | `global` (省略時は `local`)
    - `limit`: 取得件数（デフォルト 20、要求上限 1000。1リクエストの上限は100）
- **戻り値**: `PageView`（5.4）、`createdAt` 降順
- **対応エンドポイント**: `home` → `notes/timeline`（`read:account` 必須）、`local` → `notes/local-timeline`、`global` → `notes/global-timeline`
- **並び順**: Misskey はID順で返す。remote ノートはID生成元が異なるため `createdAt` 降順と一致しないことがある。**返却前に `createdAt` でソートすること。**
- muting / blocking / channel muting は Misskey サーバー側で適用される。適用された結果が返ることを README に明記する。

### 5.2 `get_user_info`
特定ユーザーの情報を取得する。
- **引数**:
    - `handle`: `@username` または `@username@host`（4.4 の規則）
        - 値は `@` で始まっていなければならない。`@` がない入力はエラー。
        - `@username@host` 形式は **username と host に分割して** API に渡す（`username` フィールドに `@user@host` をそのまま渡してはいけない）。
        - host 省略（`@username`）はインスタンスホスト補完（= ローカルユーザー）。
        - 解析した host がインスタンスホストと一致する場合はローカルに正規化する（`users/show` には `null`）。
        - 省略時は `MISSKEY_TOKEN` に紐づく認証済みユーザーを取得する（`users/me`）。
- **戻り値**: `UserViewDetailed`（4.2）。ページラッパーは付けない。
- **エラー**: `NO_SUCH_USER` (404) / `FAILED_TO_RESOLVE_REMOTE_USER` を LLM向けメッセージに変換する（6.2.4）。

### 5.3 `get_user_notes`
特定ユーザー（または自分）の投稿一覧を取得する。
- **引数**:
    - `handle`: ユーザー指定（5.2 と同じ規則）
        - 省略時は `MISSKEY_TOKEN` に紐づく認証済みユーザーを取得する。
    - `after`: この日時よりも後の投稿を取得する（排他）。省略可能。
    - `before`: この日時よりも前の投稿を取得する（排他）。省略可能。
        - `after` と同時に指定したときは AND 条件となる。
        - `after > before` はエラー。
    - `limit`: 取得件数（デフォルト 100、要求上限 1000。1リクエストの上限は100）。100超はページネーションで叶える（6.1.4）。
    - `includeRenotes`: デフォルト true（Misskey API の `withRenotes` と一致）。renote を「投稿」に含めるかは README に明記。
- **戻り値**: `PageView`（5.4）、`createdAt` 降順

### 5.4 `PageView` オブジェクト
`get_timeline` と `get_user_notes` の返却型。
- `notes`: `NoteView[]`、`createdAt` 降順
- `truncated`: 要求件数に対して省略した部分があるかどうか
- `nextCursor`: 続きを取得する場合の `untilId`（最後の返却ノートのID）。省略時は `null`
- `message`: `truncated` が true のときのみ存在し、LLM に省略を伝える英語の指示文（表示件数と `nextCursor` への続きの指示を含む）

## 6. 実装上の注意点

### 6.1 データ取得戦略
1. **主経路は `users/notes` + `sinceDate`/`untilDate`（epoch ms）**。日付範囲指定がある場合、ID境界を自前で計算して `sinceId`/`untilId` に変換する方式は**使わない**。Misskey のID生成方式は `aid | aidx | meid | meidg | ulid | objectid` でインスタンス設定により異なり、「先頭4文字＝ミリ秒タイムスタンプ」は aid/aidx/meid/meidg にしか成立しない（ulid は先頭10文字、objectid は先頭4バイト）。
2. **`users/notes` を主経路とする理由**（検索経路は定義的に不完全）:
    - `text` が null のノート（メディアのみ・renote）を拾える。Meilisearch 経路は `text == null && cw == null` を索引しない。
    - 自分自身の限定公開（`followers` / `specified`）を拾える。Meilisearch 経路は `home`/`public` しか索引しない。
    - Meilisearch の scope が `local`（デフォルト）の場合、remote ユーザーのノートは索引されず検索結果から落ちる。
3. **`/notes/search` は全文検索を明示的に要求した場合のみ**（任意機能）。その場合の制約を README に明記する: text/cw が null のノートが漏れる、public/home しか索引されない、scope local で remote が狭くなる、`rangeStartAt`/`rangeEndAt` は `note.id > gen(...)` というID比較であり remote ノート（Misskey形式でないID）で日付境界が機能しない。`canSearchNotes` は role policy（ユーザー単位）であり `meta` の `disableSearchNotes` とは別物。フラグで分岐せず、`UNAVAILABLE` を受けてからフォールバックする。
4. **ページネーション**: 1リクエストの上限は100件。要求 `limit` は 1..1000 に clamp し、100件ずつ `untilId` で遡及する（最大10ページ = 1000件）。停止条件:
    - 空ページが返った
    - 境界（`after`）に到達した
    - 要求件数に到達した
    - 最大ページ数（10ページ）に到達した（暴走防止）
    省略が発生した場合は 5.4 の `truncated` / `nextCursor` / `message` で LLM に伝える。
5. **境界の含/不含**: `sinceId`/`untilId`/`sinceDate`/`untilDate` は排他だが、`gen(ms)` から作った境界IDは同一ミリ秒のノートと必ずしも一致しない（末尾がランダム）。したがって **ID境界で粗く絞り込み、返ってきた100件を `createdAt` で厳密に検証して範囲外を落とす**。これで 6.2 の排他 semantics が厳密に成立する。
6. **remote ユーザーの留意点**: `users/notes` はID順で返す。remote インスタンスが別のID方式を使うとID順は時系列順ではない。この場合の遡及ページネーションは確実ではない（既知の制限として README に明記）。

### 6.2 日付形式の解釈
1. **厳格な形式**: 入力はLLMによって正規化されたISO 8601（`YYYY-MM-DD`、任意で時刻付き）であることを前提とする。サーバーは曖昧な形式の解析を行わず、検証のみを行う。
2. **拒絶基準**: 厳密なISO 8601でない、または妥当な日付として成立しない場合（例：2月31日）はエラーを返す。相対的な時間表現は解析しない。
   - 許容: `2024-02-03`, `2024-02-03T12:34:56Z`, `2024-02-03T12:34:56+09:00`, `2024-02-03T12:34:56`（オフセットなしは `TZ` の壁時計時刻）, 小数秒は任意桁数
   - 拒否: `2024-2-3`, `2024-02-31`, `2024-02-03T25:00:00`, `2024-02-03T12:34:60`（leap second）, `2024-02-03 12:34:56Z`（space区切り）, `20240203`（basic format）, `2024-W05-3`（週日付）, `2024-02-03T12:34`（reduced precision）, `2024/02/03`, `now`, `-1d`
3. **エラーハンドリング**: 解析不能な場合は、LLMに対し英語で「具体的な日付（YYYY-MM-DD形式）で再指定してください」という指示を返すこと。LLM向けエラー文言はすべて英語とする。
4. **タイムゾーン境界**: 日付のみの入力（時刻なし）は `TZ` の0時境界として扱う。オフセット付き（`Z`、`+09:00`等）はそのまま尊重する。境界ちょうど（`00:00:00.000`）のノートは**排他により除外される**。この挙動を README に明記する。DSTで曖昧または存在しない壁時計時刻は `latest()`（DST移行後のオフセット）で解決する。
5. **LLM側への指示**: ツールの description に「日付は ISO 8601（`YYYY-MM-DD`）で渡すこと、相対表現は渡さないこと」を明記する。

### 6.3 環境変数
以下の環境変数を必須とする。
- `MISSKEY_TOKEN`: APIアクセストークン
- `MISSKEY_API_URL`: APIベースURL (例: `https://misskey.io/api`)
- `TZ`: タイムゾーン（**デフォルトなし。未設定は起動エラー**。`chrono-tz` は環境変数を自動で読まないため、明示的に文字列として解析する）

起動時にすべて検証すること。`MISSKEY_API_URL` は scheme を https に限定し、リダイレクトを追わない（token の別origin漏洩防止）。token をログ・エラーメッセージに出さない。`instance_host` は punycode で保存する。

### 6.4 HTTP 層
- 1リクエストの `limit` は Misskey API の上限（100）で clamp する。要求件数（5.4 の `limit`）は 1000 に clamp する。
- タイムアウトを設定する（reqwest デフォルトは無制限）。
- 429（rate limit）は指数 backoff（200ms × 2^n、最大3リトライ）でリトライし、超過 then LLM向けエラー。
- stdio トランスポート: stdout は JSON-RPC のみ、ログは stderr。

## 7. 制約事項とセキュリティ
- **書き込み不可**: 本サーバーはデータの取得のみを行い、投稿、リポスト、ユーザー情報の変更などの書き込み操作は行わない。読み取り専用の保証はコード構造（呼び出されるエンドポイントが読み取り系のみ）とテストで検証する。実行時の allowlist 判定は持たない。
- **呼び出し可能エンドポイント**: `users/me`, `users/show`, `users/notes`, `notes/timeline`, `notes/local-timeline`, `notes/global-timeline`（全文検索を任意機能として実装する場合のみ `notes/search`）。テストで書き込み系エンドポイントが呼ばれないことを検証する。
- **権限の最小化**: `MISSKEY_TOKEN` は `read:account` 権限のみを持ち、`write` 系の権限を含まないこと。
- **必要権限（Scope）**:
    - `read:account`: 認証ユーザーのアイデンティティ確認および「ホーム」タイムライン、および限定公開ノートへのアクセスに必要。
    - ※公開ノートおよび公開プロフィール情報の取得は、認証済みユーザーとしてアクセスすることで可能であり、別途のスコープは不要。
- **メディア処理**: 画像や動画のファイル解析は行わず、URL等のテキスト情報のみを返す。LLM は画像をダウンロードして開けるため、file の `url` は view に含める。
- **機能制限**: リスト、アンテナ、チャンネルの管理機能は現時点での実装対象外とする。
- **トークンサイズ**: 1000ノートの JSON は LLM コンテキストに対して大きい。本文 truncation、`cw`（content warning）と nsfw の扱いを README に明記する。
