# Agent QA Harness — Rust / Cargo

Codex、Claude Code、Antigravity、Cursorで利用する探索的QAハーネスです。v0.2.0では実行コードをRustへ移行しました。利用中のフロンティアモデルが判断し、18個のMCPツールと4つのSkillsが探索・証拠保存・独立した再現確認・JSON/Markdownレポートを支えます。別のモデルAPIキーは不要です。

## 導入

```sh
cargo install --git https://github.com/msandroid/agent-qa-harness --tag v0.2.0 --locked
```

ソースから実行する場合:

```sh
cargo build --release --locked
cargo install --path . --locked
agent-qa config --client codex --workspace "C:\Projects\MyApp"
```

`--client`は`codex`、`claude`、`cursor`、`antigravity`を指定できます。出力された設定を既存のMCP設定へ追加してください。設定生成はファイルを書き換えません。Python/Nodeは実行時に不要です。ブラウザ操作にはChrome/Chromiumが必要で、自動検出できない場合は環境変数`QA_CHROME`へ実行ファイルの絶対パスを指定します。

## 省メモリと並列実行

```sh
agent-qa serve --workspace "C:\Projects\MyApp" --max-concurrency 4 --max-browsers 2
```

Tokioの非同期ワーカーは2スレッドです。ツール操作は既定で最大4件、ブラウザコンテキストは最大2件とし、上限を超える要求はbusyエラーで返します。異なるジョブは並列に進み、同じジョブの操作は直列化します。ブラウザは共有Chromiumプロセス内でジョブごとに分離します。

証拠のコピーとSHA-256検査は64 KiBずつ処理し、証拠1件は25 MiBまでです。レポートはSQLiteから順次出力します。ジョブの一覧・詳細・ログにも件数上限を設け、切り捨てを明示します。Chromiumとページ自体のメモリは別途必要です。これらはOSによるメモリ上限ではありません。

## 実行の流れ

1. 対象ビルド、合否基準、範囲、許可オリジン、予算を固定する。
2. 対象をリセットして探索を開始し、操作と証拠を保存する。
3. 不具合候補を記録し、新しい実行・証拠で再現確認する。
4. 基準ごとのpass/fail/inconclusive/not_testedを記録してレポートを出力する。

データは既定で対象ワークスペースの`.qa-harness`へ保存します。ゲーム・デスクトップ・モバイルではホストの既存ツールを使用し、証拠を取り込みます。ハーネスの処理成功と、検査対象の品質合格は別です。リセットや外部観測は操作者の申告として扱います。

## 検証

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo test --locked --test browser -- --ignored --nocapture
cargo run --locked -- package
```

ブラウザ試験では2ジョブの並列動作と分離、保存後の再読込で値が戻る不具合の発見、新しいコンテキストでの再現、レポート出力を確認します。実モデルの課金呼び出しは行いません。

以前のPython版で行ったCodex・Claude・Cursorの実モデル検証と、今回のRust版の検証は区別しています。Antigravityの実モデル検証は未完了です。Rust版の全4クライアントでの実モデル検証済みとはしていません。

詳細: [English README](README.md)、[クライアント設定](docs/clients.md)、[検証・移行](docs/validation.md)、[境界と設計](docs/architecture.md)。
