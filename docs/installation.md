# インストール・セットアップ（v1.0.0）

**日本語** | [English](en/installation.md) · [ドキュメント一覧](README.md) · [README](../README.md)

> **v1.0.0 / 正式リリース**。独立したセキュリティ監査は未完了です。**本番SSHへ導入しないでください**。まず使い捨てUbuntu VMで検証し、SSHに依存しない復旧用コンソールと別の管理セッションを確保してください。

## 前提

Ubuntu 22.04 / 24.04（amd64 / arm64）がCIの実SSH/PAMテスト対象です。Ubuntu 26.04など他バージョンでの動作保証はありません。systemd、OpenSSH Server（`UsePAM yes`）、amd64またはarm64、Discord Botが必要です。

## 公開版のインストール

テスト用Ubuntuへの新規導入は、公開中のv1.0.0を指定します。正式版という区分は本番運用の安全性を保証しません

```bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v1.0.0
```

インストールだけではSSHのPAM保護を有効化しません。`--version`を省略するとGitHubの最新の正式Releaseを取得します。版を固定するには上記の指定を使ってください。SHA-256チェックサム照合は発行者の独立した署名確認とは異なるため、rootで実行する前にスクリプトの内容を確認してください。PAM保護中の上書きは拒否されます。既存環境には[更新ガイド](updates.md)を使ってください

## ソースからのビルド（開発用）

~~~bash
sudo apt-get install -y git build-essential pkg-config libpam0g-dev python3 python3-venv python3-pip
git clone https://github.com/disnana/DisnanaSASPAM.git
cd DisnanaSASPAM
# stable Rust / Cargo のインストールは公式 https://rustup.rs/ を参照
cargo test --locked --workspace
cargo build --locked --release --workspace
sudo bash scripts/install-from-build.sh
~~~

`install-from-build.sh` はソース・ビルド成果物を所定位置へ配置しますが、**PAMは変更しません**。

## 設定

1. Discord Developer PortalでBotを作成。Tokenは絶対にGitHubにコミットしない
2. Botを専用の非公開チャンネルへ招待し、閲覧・メッセージ送信・Embed・履歴参照の権限を付与
3. Discord開発者モードでGuild ID・Channel ID・**承認用Role ID（カンマ区切りで複数可）**を取得。専用チャンネルの閲覧権限をロールで制限
4. `sudo saspam setup` を実行し、Bot Token、Guild ID、Channel ID、複数Role ID、サーバー名、タイムアウト（10～180秒）を入力
5. `sudo saspam doctor` と `sudo saspam test`。Discordの「承認」ボタンで成功することを確認
6. 既存SSHセッションとコンソール復旧経路を維持し `sudo saspam enable --ack-recovery --ack-tested`
7. 別SSHセッションから新しいログインで承認・拒否・期限切れを確認

### ntfy / Bark

`saspam setup` で任意に有効化できます。ntfy.shトピックは最低24文字のランダム名を使うこと。**公開トピックは秘匿領域ではありません**。Barkデバイスキーは秘密として保管します。両者は通知配送のみであり、承認はDiscordから行います。

## よくある問題

- `sshd -t` に失敗：SSHの設定エラーを先に解消
- `UsePAM no`：SASPAM保護は有効化できない
- `saspamd` が失敗：`sudo journalctl -u saspamd -e` で確認
- Discord Botが起動しない：`sudo journalctl -u saspam-discord -e` とToken、権限、ネットワークを確認
- Botが承認できない：`saspam test`、DiscordのGuild/Channel ID・承認用Role ID・チャンネル閲覧権限、時限切れを確認
- `saspam enable` が通らない：直近15分のテストとコンソール復旧経路を確認
- CLIが残した設定を戻す：`sudo saspam disable`、`sudo saspam security rollback`

**注意:** PAMのAccount段階での成功はSSHセッションの確立を保証するものではありません。SFTPや非対話接続でもAccountゲートが必要になる場合があります。


## 既存Alpha利用者の移行

**alpha.4以降はPAM保護を解除せず、[更新ガイド](updates.md)のバージョン指定更新を使ってください。** alpha.4では`--version`が必須で、alpha.5以降はReleaseの対話選択も利用できます

alpha.3以前には更新機構がないため、別途[旧版の移行手順](updates.md)が必要です。`v0.1.0-alpha.1`はDiscordユーザーID方式で、alpha.2以降のロールID方式と設定に互換性がありません。すべての旧版移行で`sudo saspam setup`を実行し、設定を確認・保存してデーモンとBotを再起動します。`doctor`と実承認テストを通過させてから保護を有効化してください。旧版の全組み合わせについて移行が検証済みという意味ではありません

Botが停止すると未承認要求は期限切れで拒否されます。Botロールの変更はDiscord側で行えますが、既存の承認メッセージや進行中のセッションは更新・再接続の影響を受けることがあります

## Botの権限と通知メンション（alpha.3）

`saspam setup` は承認用DiscordロールIDをカンマ区切りで受け付け、通知方式は `1: ロール全員` / `2: @everyone` / `3: メンションなし` から選びます。初期値はロール全員で、`@everyone`には明示的な追加確認が必要です。Botにメンションの権限を与える場合は、必要なチャンネルに限定してください

ロールをメンションしても、承認可能なのは設定したロールのいずれかを保持し、Discordサーバー・チャンネル・閲覧権限の検証に通るメンバーだけです。初代版同様の日本語Embed（承認待ち／承認／拒否／期限切れ）と日本語ボタンを使います

## Botが`203/EXEC`で起動しない場合

インストーラーがrootでvenvを作る際、`umask 077`でディレクトリがroot専用になることがありました。alpha.3では専用グループ権限の補正と、非root実行テストをインストール中に行います

```bash
sudo systemctl status saspam-discord --no-pager -l
sudo journalctl -u saspam-discord -n 80 --no-pager -o cat
sudo namei -l /opt/saspam/venv/bin/python
sudo runuser -u saspam-bot -- /opt/saspam/venv/bin/python -c 'import discord; print(discord.__version__)'
```

`saspam test` はPAMを書き換えずにDiscordでの承認を待ちます。Bot停止時は事前チェックで中断します。復旧用コンソールを確認し、**実際に「承認」できた後**でなければ`saspam enable`を実行しないでください

## 対話設定と通知テスト

初回セットアップで日本語またはEnglishを選びます。Discord、Bark、ntfyのメッセージは同じ言語が既定になります。変更は `sudo saspam config` で個別に設定できます

```bash
sudo saspam config set language en
sudo saspam config set language ja
sudo saspam config set mentions roles
sudo saspam config set ntfy topic --generate
sudo saspam config set ntfy on
sudo saspam notify test ntfy
sudo saspam config set bark key
sudo saspam config set bark on
sudo saspam notify test bark
```

鍵・BotトークンはCLI引数に渡さないでください。Barkキーは非表示入力を使います

### 初期バージョンからの更新

alpha.3以前には更新コマンドがありません。alpha.4以降ではPAMを無効化せず `sudo saspam update --version v1.0.0` を利用できます。設定互換性と復旧経路を確認し、[更新ガイド](updates.md)の事前検証・旧版移行手順に従ってください

## 安全に設定を変更するには

`sudo saspam config`で言語、Bark、ntfy、メンション、ロール、承認期限などを個別に変更できます。SSH保護が有効な場合は再起動に伴って承認中のログインが拒否される可能性があり、明示確認が必要です。`sudo saspam notify test ntfy`や`sudo saspam notify test bark`はPAMを変更せず送信を試します

[更新・ダウングレード](updates.md) · [復旧手順](recovery.md) · [English version](en/installation.md)

## v1.0.0：認証方式欄は標準で非表示

**標準設定では認証方式欄を表示しません**。「不明」の欄も出しません。新規導入・更新とも設定が未指定ならOFFです。PAMとDiscordの承認判定は変わりません

改造OpenSSHを別途検証・管理している利用者だけが、[実験的な表示](auth-method-display.md)を明示的なリスク・復旧確認付きで有効にできます。この設定はOpenSSHのパッチ適用・ビルド・置換を行いません。標準Ubuntuでは実験モードでも「不明」と表示されます。改造OpenSSHは本番サポート対象外で、FIDO実機検証と独立監査も未完了です。鍵本体や指紋、パスワードは転送しません
