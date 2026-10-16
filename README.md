# misskey-fetch

Misskey を読むだけの MCP サーバー。stdio で JSON-RPC を受け取り、3つのツールを MCP クライアント（LLM）に提供する。書き込みエンドポイントは呼ばない。

Rust（rmcp / reqwest / chrono-tz / schemars / serde）。

## 実行

| 環境変数 | 必須 | 内容 |
|---|---|---|
| `MISSKEY_TOKEN` | はい | 読み取り権限のトークン。空だと起動時にエラー |
| `MISSKEY_API_URL` | はい | `https://` の API ベース URL。末尾スラッシュは除去。http は拒否、リダイレクトは追わない（token の別 origin 漏洩防止） |
| `TZ` | はい | IANA タイムゾーン名（例 `Asia/Tokyo`）。未設定・不正は起動エラー。chrono-tz は env を自動参照しないので明示 parse する |

`MISSKEY_API_URL` のホスト部は `instance_host` として保持する（lowercase + punycode）。`handle` の host 省略時に使う。

```sh
cargo run --locked
```

```sh
docker build -t misskey-fetch .
docker run -i --rm -e MISSKEY_TOKEN -e MISSKEY_API_URL -e TZ misskey-fetch
```

stdout は JSON-RPC のみ。起動情報（`api_base` と `tz`）と 429 の backoff は stderr。token は stderr にも出さない。

MCP クライアントの設定例:

```json
{
  "mcpServers": {
    "misskey": {
      "command": "misskey-fetch",
      "env": {
        "MISSKEY_TOKEN": "...",
        "MISSKEY_API_URL": "https://misskey.io/api",
        "TZ": "Asia/Tokyo"
      }
    }
  }
}
```

## ツール

### `get_timeline`

| 引数 | 既定 | 内容 |
|---|---|---|
| `type` | `local` | `home` \| `local` \| `global` |
| `limit` | 20 | 1..1000 に clamp |

日付引数は受け付けない。呼ぶ endpoint: `notes/timeline` / `notes/local-timeline` / `notes/global-timeline`。

### `get_user_info`

| 引数 | 既定 | 内容 |
|---|---|---|
| `handle` | 認証ユーザー | `@username` または `@username@host` |

`users/show`（handle 省略時は `users/me`）でユーザーを解決し、`users/notes`（limit 10）をもう一度呼んで `recentNotes` を埋める。ページラッパーなしで `{user, description, pinnedNotes, recentNotes}` を返す。

### `get_user_notes`

| 引数 | 既定 | 内容 |
|---|---|---|
| `handle` | 認証ユーザー | `@username` または `@username@host` |
| `after` | なし | ISO 8601。排他境界 |
| `before` | なし | ISO 8601。排他境界 |
| `limit` | 100 | 1..1000 に clamp |
| `includeRenotes` | true | |

`after > before` はエラー。呼ぶ endpoint: `users/notes`（`sinceDate` / `untilDate` は epoch ms）。

## 応答の形

`get_timeline` / `get_user_notes` はページオブジェクト、`get_user_info` はそのままユーザーオブジェクト。すべて camelCase。

```json
{
  "notes": [
    {
      "id": "aa",
      "createdAt": "2024-02-03T00:00:00Z",
      "user": { "name": "B", "handle": "@b@misskey.io", "isBot": true },
      "text": "本文",
      "cw": "警告文",
      "visibility": "home",
      "isHidden": false,
      "poll": null,
      "files": [
        { "name": "img.png", "mime": "image/png", "size": 1234, "isSensitive": true,
          "url": "https://cdn.example/img.png", "comment": null }
      ],
      "channel": { "name": "ch", "isSensitive": false },
      "reply": { "state": "none" },
      "renote": { "state": "notFound", "id": "x1" }
    }
  ],
  "truncated": false,
  "nextCursor": null,
  "message": null
}
```

- `cw` は警告文（UI で常時表示）、`text` はその折りたまれた本文。
- `isHidden` はノート固有の property ではなく viewer 相対の relation（policy 由来の隠蔽）。
- `userId` は view に入れない。LLM に見せる識別子は `name` と `handle` だけ。
- `reply` / `renote` は 4 状態の tagged object: `none` / `notJoined{id}`（relation はあるが応答に含まれなかった）/ `notFound{id}`（relation があり detail gate は通過したが対象がない）/ `present{id, note}`。入れ子 note の renote chain は深さ 10 で打ち切り、打ち切り点は `notJoined`。
- `truncated` / `nextCursor` / `message` は省略が発生した場合だけ入る。`message` は英語の文字列で、続きの `untilId` を書く。

## 日付引数

厳格な ISO 8601 だけ受け付ける。曖昧解析はしない（正規化は LLM 側の仕事）。

- `YYYY-MM-DD` だけ: `TZ` の 0 時境界
- オフセット付き（`Z` / `+09:00`）: そのまま尊重（`TZ` は影響しない）
- オフセットなしの時刻: `TZ` の壁時計時刻
- 小数秒は任意桁数（9桁超は nanosecond 分解能で切り捨て）
- DST で曖昧・非存在の壁時計時刻は `latest()` で解決

拒否: 月日1桁、不正日付、範囲外（`25:00` / `12:60` / leap second `:60`）、slash 区切り、相対表現（`now`, `-1d`）、basic format（`20240203`）、週日付、space 区切り、reduced precision。拒否時のメッセージは英語の `Invalid date. Please re-specify as a concrete date in YYYY-MM-DD format: {input}`。

境界は排他。日付のみ入力は `TZ` の 0 時境界なので、境界ちょうど（`00:00:00.000`）のノートは除外される。API で粗く絞り込み、返ってきた件数を `createdAt` で厳密検証して範囲外を落とす。

## handle

- `@` で始まっていなければならない。`@` なしはエラー。
- `@username`（host 省略）はローカルユーザー。
- `@username@host` は username / host に分割。解析した host が `instance_host` と等しい場合はローカルに正規化する（Misskey はローカルユーザーを `host: null` で保存するため、自ホストを remote host として渡すと失敗する）。
- lookup path は username lowercase、host lowercase + punycode。display path は API が返した stored 値を使い、常に完全形（`@username@instance_host`）で返す。

## 件数とページネーション

要求 limit は 1..1000 に clamp。1リクエストの page size は 100（Misskey API の上限）。ページネーションは最大 10 ページ。停止条件は件数到達、空ページ（データ尽き）、または日付境界到達。データ尽きは `truncated` にならない。

ページネーションは `untilId`（前回最後のノート）でサーバ側に行う。remote ユーザーの `users/notes` は ID 順で返るため、ID 方式が異なると ID 順 ≠ 時系列順。返却前に `createdAt` 降順に再ソートする。

## エラー

LLM 向けエラーは JSON-RPC の error ではなく `isError: true` のツール結果として返す（`Display` がそのまま文言になる英語）。

- `Invalid date. Please re-specify as a concrete date in YYYY-MM-DD format: {input}`
- `Parameter error: {message}`（handle 不正、`after > before`）
- `User not found: {handle}`（`NO_SUCH_USER`）
- `Could not resolve remote user: {handle}`（`FAILED_TO_RESOLVE_REMOTE_USER`）
- `Full-text search is unavailable (UNAVAILABLE); use the users/notes path`（全文検索ツールはない。このコードだけフォールバック用にマップしている）
- `Rate limit reached. Please retry after a while`（429 は 200ms × 2^n の指数 backoff、最大3リトライ）
- `Misskey API error ({code}): {message}`
- `HTTP error: {message}`
- `Inconsistent API response: {field} is present without its id`

HTTP timeout は 30 秒。closed enum（`Visibility` の 4 値）は `Unknown` variant を持たないので、Misskey が値を追加すると解析失敗で落ちる。

## 構成

```
src/
  main.rs        stdio エントリポイント
  config.rs      env 検証
  params.rs      ツール引数と limit 定数
  dates.rs       ISO 8601 厳格 parse
  handle.rs      handle 解析と正規化
  errors.rs      LLM 向けエラー
  misskey/       API 応答の型（model）と HTTP client
  view.rs        LLM に返す型（view）
  tools/         3ツールの実装
  service.rs     rmcp の tool router
tests/
  unit.rs        日付・handle のケース表
  integration.rs wiremock（ページネーション、view の JSON shape、relation 状態）
```

```sh
cargo test
```

## 実装上の挙動

- `createdAt` は Misskey がノート ID から導出する値（DB カラムではない）。解析に失敗したノートは個別 skip せず応答全体を失敗させる。非準拠実装（Mastodon snowflake 等）はスコープ外。
- `text` は pack 時に書き換えられることがある（`note.name && (note.url ?? note.uri)` の条件）。stored 値と view 値が異なる。
- 入れ子 note の detail gate は非対称: `reply` の入れ子は `detail: false`（`poll` が shape に入らない）、`renote` の入れ子は `detail: true`。
- muting / blocking / channel muting はサーバー側で適用され、適用された結果が返る。
- `users/notes` は認証ユーザーから見える範囲（自分の `followers` / `specified` を含む）を返す。タイムラインは公開範囲のみ。
- view は model の mirror ではない。`reactionCount`（derived）、`emojis`、`deletedAt`、`myReaction`、`clippedCount` 等は落としている。
