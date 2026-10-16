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
- **ID隠蔽**: 内部的な `userId` 等のIDはサーバー側で解決し、LLMにはユーザー名やハンドルなど、人間が解釈可能な識別子で提示する。
- **全文検索**: 本文検索は本仕様の対象外（必要時のみ任意機能として扱う。6.1参照）。

## 3. アーキテクチャ
本サーバーはMCP（Model Context Protocol）仕様に基づき、JSON-RPCを介してLLMにツールを提供し、内部的にMisskey REST APIを呼び出すプロキシとして動作する。

## 4. データモデル
### 4.1 Note オブジェクト
- `createdAt`: 作成日時 (ISO 8601 拡張方式)。Misskey はノートIDから導出した RFC3339 文字列で返す。
- `text`: 投稿本文
- その他、Misskey APIが提供するノート関連のメタデータ

### 4.2 NoteWithUser オブジェクト
- Noteオブジェクトのフィールドに加え、以下のフィールドを付与する。
- `name`: 表示名。Misskey の `UserLite.name` は nullable であり、表示名のないユーザーは `null` になる。
- `handle`: `@username@host.domain` 形式のハンドル
- `noteType`: `note` | `renote` | `reply`。判定優先順位は `renote` > `reply` > `note`（引用リポストは `renote`）。pure renote（本文・cw・ファイル・poll なし）は `text` が `null` であり、LLM は `noteType` で判別する。
- `hasCw`: `cw` が存在するか。`cw` は警告文（UIで常時表示）であり `text` はその隠された本文である。
- `isHidden`: Misskey のポリシーにより隠されたノート（`hideNote()` は `text`/`cw` を null にするため、`hasCw` なしでは pure renote と区別できない）。
- `visibility`: `public` | `home` | `followers` | `specified`
- **すべてのノート返却は NoteWithUser を用いる**（5.3の返却型と統一）。renote / reply が混ざると著者が queried user と異なるため、著者はノートごとに解決して提示する。
- **入れ子 renote の深さ上限は 10**。上限到達時は `renote` を `null` とし、`renoteDepthLimited` を true にする。

### 4.3 User オブジェクト
- `name`: 表示名（nullable）
- `handle`: `@username@host.domain` 形式のハンドル
- `description`: 自己紹介文
- `pinnedNotes`: ピン留め投稿（`users/show` から取得）
- `recentNotes`: 最近の投稿 (10件程度)。`users/show` には含まれないため `users/notes`（limit 10）の追加呼び出しで取得する。

## 5. インターフェース仕様 (MCPツール)

### 5.1 `get_timeline`
タイムラインの最新ノートを取得する。
- **引数**:
    - `type`: `home` | `local` | `global` (省略時は `local`)
    - `limit`: 取得件数（デフォルト 20、要求上限 1000。1リクエストの上限は100）
- **戻り値**: `NotesPage`（5.4）、`createdAt` 降順
- **対応エンドポイント**: `home` → `notes/timeline`（`read:account` 必須）、`local` → `notes/local-timeline`、`global` → `notes/global-timeline`
- **並び順**: Misskey はID順で返す。remote ノートはID生成元が異なるため `createdAt` 降順と一致しないことがある。**返却前に `createdAt` でソートすること。**
- muting / blocking / channel muting は Misskey サーバー側で適用される。適用された結果が返ることを README に明記する。

### 5.2 `get_user_info`
特定ユーザーの情報を取得する。
- **引数**:
    - `username`: ユーザー名
        - 先頭に `@` が含まれていてもいなくても受け入れること。サーバー側で `@` の有無を正規化して処理する。
        - `@username@host` 形式で渡された場合は **username と host に分割して** API に渡す（`username` フィールドに `@user@host` をそのまま渡してはいけない）。
        - 省略時は `MISSKEY_TOKEN` に紐づく認証済みユーザーを取得する（`users/me`）。
    - `host`: インスタンスホスト名
        - 省略時は自サーバーのユーザーを取得する。
        - `username` 省略時は指定不可（エラー）。
        - `username` が `@username@host` 形式で host を内包する場合、`host` と**同時に指定してはいけない**（エラー）。
- **戻り値**: User オブジェクト
- **ローカルホストの表現**: `users/show` の応答では `null`、`notes/search` のパラメータでは `.`。内部で変換する。
- **エラー**: `NO_SUCH_USER` (404) / `FAILED_TO_RESOLVE_REMOTE_USER` を LLM向けメッセージに変換する（6.2.4）。

### 5.3 `get_user_notes`
特定ユーザー（または自分）の投稿一覧を取得する。
- **引数**:
    - `username`: ユーザー名（5.2 と同じ正規化規則）
        - 省略時は `MISSKEY_TOKEN` に紐づく認証済みユーザーを取得する。
    - `host`: インスタンスホスト名（5.2 と同じ制約）
        - 省略時は自サーバーのユーザーを取得する。
        - `username` 省略時は指定不可（エラー）。
    - `after`: この日時よりも後の投稿を取得する（排他）。省略可能。
    - `before`: この日時よりも前の投稿を取得する（排他）。省略可能。
        - `after` と同時に指定したときは AND 条件となる。
        - `after > before` はエラー。
    - `limit`: 取得件数（デフォルト 100、要求上限 1000。1リクエストの上限は100）。100超はページネーションで叶える（6.1.4）。
    - `includeRenotes`: デフォルト true（Misskey API の `withRenotes` と一致）。renote を「投稿」に含めるかは README に明記。
- **戻り値**: `NotesPage`（5.4）、`createdAt` 降順

### 5.4 `NotesPage` オブジェクト
`get_timeline` と `get_user_notes` の返却型。
- `notes`: `NoteWithUser[]`、`createdAt` 降順
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

起動時にすべて検証すること。`MISSKEY_API_URL` は scheme を https に限定し、リダイレクトを追わない（token の別origin漏洩防止）。token をログ・エラーメッセージに出さない。

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
- **メディア処理**: 画像や動画のファイル解析は行わず、URL等のテキスト情報のみを返す。
- **機能制限**: リスト、アンテナ、チャンネルの管理機能は現時点での実装対象外とする。
- **トークンサイズ**: 1000ノートの JSON は LLM コンテキストに対して大きい。本文 truncation、`cw`（content warning）と nsfw の扱いを README に明記する。
