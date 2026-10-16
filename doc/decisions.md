# 設計上の曖昧点の決定

spec.md は常時最新の事実だけを保持する（変更履歴は持たない。履歴は git が追う）。このファイルは決定の根拠と未決事項を記録する。

## 確定済み

- **renote / noteType**: pure renote（text・cw・files・poll がなし）は「投稿」に含める（`includeRenotes` デフォルト true、Misskey の `withRenotes` と一致）。LLM 判別用に `noteType`（`note`/`renote`/`reply`）を追加。判定優先順位は **renote > reply > note**（引用リポストは renote 側）。Misskey の `isPureRenote` は「renote があり、reply がなく、text・cw・files・poll がなし」。
- **入れ子 renote の深さ上限 10**: Misskey は `renote` を再帰 pack する（`renote.renote...`）。深さ 10 で打ち切り、`renoteDepthLimited` を true にする。
- **cw / nsfw**: 本文 truncation はしない。`cw`（警告文、UIで常時表示）と `text`（折りたまれた本文）をそのまま返し、`hasCw` フラグで「UI では cw だけが見えている」ことを LLM に伝える。`files[].isNsfw` はそのまま返す。コンテキストサイズは limit で制御。
- **`isHidden`**: Misskey の `hideNote()` は `text`/`cw` を null にするため、pure renote とポリシー由来の隠蔽が同一に見える。`isHidden` をモデルに含めて区別する。
- **limit clamp**: 要求 `limit` は **1..1000** に clamp。1リクエストの page size は 100（Misskey API の上限）。ページネーションは最大 **10 ページ**（= 1000件）。省略が発生したら `truncated` / `nextCursor` / `message`（英語）で LLM に伝える。
- **排他境界**: 日付のみ入力は `TZ` の 0 時。境界ちょうど `00:00:00.000` は排他により除外。返却前に `createdAt` を厳密検証。
- **ISO 8601 の許容範囲**: 規格準拠で常用記法から遠くない入力は許容。時刻のオフセットは**省略可**（省略時は `TZ` の壁時計時刻として解釈）。小数秒は任意桁数。拒否: 月日1桁 (`2024-2-3`)、不正日付 (`2024-02-31`)、範囲外 (`25:00:00`, `12:60:00`, leap second `:60`)、slash、相対表現、basic format (`20240203`)、週日付、space 区切り、reduced precision (`T12:34`)。
- **DST 曖昧時**: `latest()`（DST 移行後のオフセット）。`earliest()`（その日の最初の瞬間）との差は、曖昧（秋の巻き戻し）で1時間、非存在（春の送り）で2時間。`Asia/Tokyo` では差が出ない。
- **エラー文言**: LLM 向けはすべて英語。解析不能時は「具体的な日付（YYYY-MM-DD形式）で再指定してください」の英語版。MCP の `isError: true` + LLM 可読テキストで返す（JSON-RPC エラーにはしない）。
- **型厳密化（core_plus）**: `createdAt: DateTime<Utc>`（serde がデシリアライズ時に解析）、`Visibility` enum（`public|home|followers|specified`）、`NoteType` enum、`name: Option<String>`（`UserLite.name` は nullable。非 optional は実バグ）、`isHidden: bool`、`UserRef` 構造化型で username/host を保持し表示時に `@user@host` を生成。
- **`createdAt` が解析不能になる経路**: Misskey は `idService.parse(note.id).date.toISOString()` で返すため、準拠応答で壊れた文字列は届かない（不正 Date は RangeError で 500）。届くとすれば Misskey 風 API を名乗る別実装（Iceshrimp / Firefish / Mastodon 互換）またはフィールド欠落（= デシリアライズ失敗）。より現実的なリスクは**正しいが間違っている値**: `aid` 設定で remote の snowflake ID を `parseAid` すると 1970年付近のゴミ日付になる。防御分岐は削除し、README に制限として明記。
- **`undefined` と `null`**: Misskey の `renote`/`reply` は optional + nullable（未JOIN / JOIN したが存在しない / packed）の3状態。serde の `Option` では潰れる。LLM 出力には不要なので区別しない。
- **TZ**: **必須**。未設定は起動エラー（デフォルト `Asia/Tokyo` は廃止）。`chrono-tz` は env を自動参照しないため `TZ` 文字列を明示 parse。
- **ID 境界計算**: 使わない。`users/notes` + `sinceDate`/`untilDate`（epoch ms）を主経路とする。Misskey 側も `gen(ms)` → ID 比較であり、同一ミリ秒の保証がなく、remote ノートは別形式のIDで保存されるため厳密なタイムスタンプ絞り込みはできない。返却前検証が必要。
- **ソート**: API の ID 順に依存せず返却前に `createdAt` 降順でソート。
- **HTTP**: https 限定、redirect なし、タイムアウト 30s、429 は**指数 backoff**（200ms × 2^n = 200/400/800、最大3リトライ）後に LLM 向けエラー。client builder 失敗時のフォールバック（`redirect_policy::none` が効かず全リダイレクト追従）は削除しエラーとして伝播。stdout は JSON-RPC のみ、stderr はログ。token は出さない。
- **allowlist**: 実行時の allowlist 判定は持たない（呼ぶエンドポイントが決まっているため無意味）。読み取り専用の保証はコード構造とテストで検証。`meta` は不要と判断し削除。
- **mock 戦略**: `wiremock`（dev-dependency）。実 Misskey へのネットワークリクエストはしない。例外系のテストは増やさない（既存の追記・削除のみ）。
- **ローカルホスト表現**: `users/show` の**応答**では `null`（JSON の null。パラメータの省略ではない）、`notes/search` の**パラメータ**では `.`。内部で「ローカルユーザー」という概念に統一し、LLM には `@username@host`（host 省略時は API URL のホスト）を提示。
- **`@` 正規化**: 先頭 `@` を除去し、`@user@host` は username / host に分割して API に渡す。username 埋め込みホストと明示 `host` 引数の**同時指定はエラー**。
- **デフォルト引数**: `withReplies` false、`withRenotes` true、`allowPartial` false（Misskey の paramDef と一致。Misskey 側コメントは true を推奨）。

## 未決

- **username / host パラメータの形状**（人間が検討中）。現状は「同時指定不可」だけを実装。
  - 案 (a): `username` が host を内包（`alice` または `@alice@remote.example`）、`host` 引数を削除。レスポンスの `handle` 形式と揃い、LLM が応答で見た `@bob@remote.example` をそのまま再投入できる（round-trip が自然）。Misskey API 自体は `username` と `host` を分離して受けるが、username に `@` は含めないため解析は安全。
  - 案 (b): `username` は username のみ、`host` は host のみ。`@` 付き入力は拒否。Misskey API の paramDef と1対1で、解析が不要。ただし LLM が `handle` を自分で分割する必要があり、ミスの余地が残る。
  - 判断材料: LLM が実際に渡す文字列がどちらに収まるか（応答の `handle` と同一形なら (a)、API の paramDef と同一形なら (b)）。
- **テストで未カバー**: 429 backoff、`FAILED_TO_RESOLVE_REMOTE_USER`、`UNAVAILABLE` フォールバック、DST 曖昧、`host` のみの指定エラー。追加するかは未決。
