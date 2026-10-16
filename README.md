# misskey-fetch

Misskey から情報を読み取るための MCP (Model Context Protocol) サーバーです。
LLM が Misskey のタイムラインやユーザー情報を取得するためのツールセットを提供します。

**本サーバーは「読み取り専用」であり、投稿などの書き込み操作は行いません。**

## クイックスタート

### 1. 起動方法

#### Docker を使用する場合 (推奨)
```sh
docker run -i --rm -e MISSKEY_TOKEN -e MISSKEY_API_URL -e TZ misskey-fetch
```
※ SSE モードで起動する場合は `--transport sse --host 0.0.0.0 --port 8080` などの引数を追加してください。

#### Cargo でビルドする場合
```sh
cargo run --locked
```

### 2. 環境設定

起動時に以下の環境変数が必要です。

| 変数名 | 必須 | 説明 |
|---|---|---|
| `MISSKEY_TOKEN` | はい | API トークン。自分自身の情報を取得するには `read:account` 権限が必要です。 |
| `MISSKEY_API_URL` | はい | API のベース URL (例: `https://misskey.io/api`)。 |
| `TZ` | はい | タイムゾーン (例: `Asia/Tokyo`)。 |

### 3. MCP クライアントの設定例

Claude Desktop などの設定ファイルに以下のように追加します。

```json
{
  "mcpServers": {
    "misskey": {
      "command": "misskey-fetch",
      "env": {
        "MISSKEY_TOKEN": "あなたのトークン",
        "MISSKEY_API_URL": "https://your-instance.sstable/api",
        "TZ": "Asia/Tokyo"
      }
    }
  }
}
```

## 利用可能なツール

### `get_timeline`
タイムラインのノートを取得します。
- **引数**: 
    - `type`: `local`, `home`, `global`
    - `limit`: 取得件数 (1〜1000)
- **用途**: インスタンスのローカルタイムライン、フォロー中のユーザー、またはグローバルな動向を確認したい時に利用します。

### `get_user_info`
特定のユーザーのプロフィールと最近の投稿を取得します。
- **引数**:
    - `handle`: `@username` または `@username@host`。省略すると認証ユーザー（自分）になります。
- **用途**: 特定のユーザーがどのような人物か、どのような投稿をしているかを把握したい時に利用します。

### `get_user_notes`
特定のユーザーの過去の投稿を期間指定などで取得します。
- **引数**:
    - `handle`: `@username` または `@username@host`。省略すると自分になります。
    - `after` / `before`: ISO 8601 形式の日付。
    - `limit`: 取得件数 (1〜1000)
- **用途**: 特定のユーザーの過去の言及や、特定の期間の投稿を詳しく分析したい時に利用します。

## 利用上の注意点と制約

### トークンの権限について
- **`read:account` (アカウントの情報を見る)**: 自分のホームタイムラインの取得や、ハンドルを省略して自分自身の情報を取得する場合に必須です。
- 権限が不足している場合、ツールは `PERMISSION_DENIED` エラーを返します。

### 日付の指定方法
日付は厳格な **ISO 8601 形式**で指定してください。
- 正しい例: `2024-02-03` または `2024-02-03T10:00:00Z`
- 相対表現（`now`, `-1d` など）や曖昧な形式は受け付けられません。

### ハンドルの指定方法
- ハンドルは必ず `@` で始めてください (例: `@user@instance.net`)。
- `@user` のようにホストを省略した場合は、サーバー設定の `MISSKEY_API_URL` のホストが自動的に適用されます。

## 開発者向け

### テストの実行
ユニットテストおよび統合テストを実行して動作を確認できます。
```sh
cargo test
```

### 内部構造
- **トランスポート**: `stdio` (標準) および `SSE` (HTTP) をサポートしています。
- **実装**: Rust (`rmcp`, `reqwest`) で構築されています。
