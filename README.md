# Kotonoha（ことのは）

話した言葉を、その場で入力欄に文字として入れる音声入力アプリです。macOS と Windows に対応しています。
音声も文字も、すべて自分のコンピューターの中で処理されます。外部のサーバーには何も送りません。

- ショートカットを押して話し、もう一度押すと、カーソルのある入力欄に文章が入ります（押し続けて話し、離しても同じです）
- 「えーと」などのつなぎ言葉や「北口、いや南口」のような言い直しを整えます
- 文の終わりで改行します。長い文章は話題のまとまりごとに改行します（設定でオフにできます）
- 人名や製品名は辞書に登録しておくと、正しい表記で入ります
- 使うモデルは空きメモリに合わせて自動で選びます
- 文字起こしには [whisper.cpp](https://github.com/ggml-org/whisper.cpp)、句読点の整形には [llama.cpp](https://github.com/ggml-org/llama.cpp) を使います

## 必要なもの

| | macOS | Windows |
|---|---|---|
| OS | macOS 13 以降 | Windows 10 / 11（64 ビット） |
| メモリ | 8 GB 以上 | 8 GB 以上 |
| 空き容量 | 約 2 GB | 約 2 GB |

## インストール

### Mac

1. 「ターミナル」を開きます（Finder の「アプリケーション」>「ユーティリティ」の中にあります）
2. 次の 1 行を貼り付けて、Return キーを押します

```sh
curl -fsSL https://raw.githubusercontent.com/kk-stack-dev/kotonoha-app/main/install.sh | sh
```

### Windows ※不具合発生中v1.0.0

1. スタートメニューで「PowerShell」と入力して開きます
2. 次の 1 行を貼り付けて、Enter キーを押します

```powershell
irm https://raw.githubusercontent.com/kk-stack-dev/kotonoha-app/main/install.ps1 | iex
```

インストールが終わると Kotonoha が起動し、セットアップ画面が開きます。新しい版が出たときも、同じ 1 行で更新できます。


## 使い方

1. 起動するとメニューバー（Windows は通知領域）に葉のアイコンが出て、初回はセットアップ画面が開きます
2. 「ダウンロードして始める」を押します。whisper.cpp、llama.cpp、モデルをまとめて取得します
   （空きメモリに合わせて約 1 GB〜1.7 GB）
3. macOS では、マイクとアクセシビリティを許可します（画面に手順が出ます）
4. セットアップ画面で試しに話してみます。あとは入力欄にカーソルを置き、ショートカットを押して話し、もう一度押します

既定のショートカットは macOS が Option + Space、Windows が Ctrl + Alt + Space です。設定画面で変えられます。

Mac では、更新するとマイクとアクセシビリティの許可が外れることがあります。そのときはもう一度許可してください。

## アンインストール

- Mac：「アプリケーション」から Kotonoha.app をゴミ箱に入れます。設定とモデルも消すときは、
  `~/Library/Application Support/dev.Kotonoha.Kotonoha` と `~/Library/Logs/Kotonoha` も消します
- Windows：「設定」>「アプリ」から Kotonoha をアンインストールします。設定とモデルも消すときは、
  `%LOCALAPPDATA%\Kotonoha` と `%APPDATA%\Kotonoha` も消します

## ソースからビルドする

```sh
# Rust（rustup）と Node.js 20 以降が必要
cargo test --workspace --exclude kotonoha-app   # 中核と OS 連携のテスト
cd app && npm install && npm run check            # 画面の型検査と、環境依存文字の検査
npm run tauri dev                                 # 開発モードで起動
npm run tauri build                               # 配布用にビルド
```

コマンドラインツールで、アプリを使わずに状態の確認や文字起こしができます。

```sh
cargo run -p kotonoha-cli -- doctor                  # ツール・モデル・権限の状態
cargo run -p kotonoha-cli -- models download         # 標準モデルを取得（チェックサム検証つき）
cargo run -p kotonoha-cli -- transcribe voice.wav    # WAV を最後まで処理して結果を表示
cargo run -p kotonoha-cli -- record --seconds 5      # マイクで録音して結果を表示
```

開発の決まりごとは [AGENTS.md](AGENTS.md) にあります。

## リリース

| リリース | 内容 | 作り方 |
|---|---|---|
| `v*`（最新） | アプリ本体。`Kotonoha-mac.zip`（Apple シリコンと Intel 共通）と `Kotonoha-windows-x64-setup.exe` | Actions の Release を、`publish` をオンにして実行 |
| `tools-v*` | セットアップで取得する `whisper-cli` と `llama-completion`（ソースは無変更） | Actions の Tools を、`release` に `tools-v<番号>` を入れて実行 |

アプリは `crates/kotonoha-core/src/tools.rs` の `TOOL_CATALOG` に書いた SHA-256 と一致したツールだけを使います。
ツールを新しくしたら、リリースに付く `catalog.rs` の中身を `TOOL_CATALOG` に貼ります。公開済みのリリースのファイルは
差し替えないでください。

## ライセンス

MIT ライセンスです（[LICENSE](LICENSE)）。

セットアップでダウンロードするものは、それぞれの配布元のライセンスに従います。

- whisper.cpp、llama.cpp：MIT ライセンス。`tools-v*` の各リリースに `LICENSE-whisper.cpp.txt` と `LICENSE-llama.cpp.txt` を付けています
- モデル：[whisper.cpp のモデル](https://huggingface.co/ggerganov/whisper.cpp)、
  [Qwen2.5-1.5B-Instruct-GGUF](https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF)、
  [Qwen3.5-0.8B-GGUF](https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF) の各ページに記載のライセンス
