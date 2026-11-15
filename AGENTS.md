# Kotonoha Voice: AI エージェント向けの決まりごと（Claude Code / Codex 共通）

## 守ること

- 音声も文字も、利用者の LAN の外（インターネットやクラウド）に出さない。クラウドの API、テレメトリ、自動更新は入れない。
  例外は LAN 連携だけ: 利用者がオンにして組にした、同じ LAN 内のコンピューターにだけ、暗号化して送る
  （`kotonoha_core::lan`、`kotonoha_platform::lan`）。接続先は `lan::address::is_lan` が LAN と判断するアドレスに限り、
  相手の鍵を確かめてから送る。オンにするまでソケットを開かず、mDNS も使わない。
  更新は通知だけ。起動の数秒後（設定で止められる）と設定画面のボタンで GitHub Releases の最新版番号を問い合わせ、
  新版があればメニューと設定画面で知らせる（`kotonoha_core::update`）。
  インストールは設定画面の「今すぐ更新」を利用者が確認したときだけ、同梱の `install.sh` / `install.ps1` を実行する。黙って更新しない
- 話した内容（文字起こし結果）をログに書かない。ログに残すのは文字数・時間・状態だけ
- UI の文字列に環境依存文字を使わない（絵文字、Mac のキー記号、丸数字など）。キー名は
  `kotonoha_core::shortcut` の整形結果（"Option + Space" / "Ctrl + Alt + Space"）を使う。
  `cd app && npm run check` が検査する
- マイク、アクセシビリティ、ローカルネットワーク、ファイアウォールの許可は、利用者が自分で出すもの。コードやスクリプトで許可を変えようとせず、設定の場所を案内する。
  例外は macOS の更新だけ: `install.sh` が更新時に限り、このアプリの許可を `tccutil reset` で消す（再署名で古い許可が効かなくなるため）。許可を出す操作は常に利用者がする

## 構成

- `crates/kotonoha-core`: 規則とセッション制御。OS に触れない。外部とのやり取りは `ports.rs` のトレイト経由
- `crates/kotonoha-platform`: トレイトの実装（録音、whisper.cpp / llama.cpp、貼り付け、保存）
- `crates/kotonoha-cli`: 動作確認用のコマンド
- `app/`: Tauri アプリ（`src-tauri` が組み立て役、`src` が画面）

新しい規則は core に、OS ごとの処理は platform に置く。core にテストのない変更を入れない。

## 確認

- `cargo test --workspace --exclude kotonoha-app`
- `cargo clippy --workspace --all-targets`
- `cd app && npm run check`
- Windows 向けのコンパイル確認: `cargo check -p kotonoha-platform --target x86_64-pc-windows-msvc`
- 実音声の通し確認（macOS）: `say -v Kyoko -o /tmp/t.wav --data-format=LEF32@16000 "..."` →
  `cargo run -p kotonoha-cli -- transcribe /tmp/t.wav`

## macOS の署名

署名なし（ad-hoc）でビルドすると、ビルドのたびにマイクとアクセシビリティの許可が外れる。
コード署名用の証明書を作り、`app/src-tauri/tauri.conf.json` の `bundle.macOS.signingIdentity`
（または環境変数 `APPLE_SIGNING_IDENTITY`）に名前を設定すると、再ビルドしても許可が保たれる。
