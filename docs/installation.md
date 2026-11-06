# Install / Setup（開発版）

> 開発中の未監査alphaです。**本番SSHへ導入しないでください**。クラウドコンソールへのアクセスと別の管理セッションが必要です。

## 前提

Ubuntu 22.04 / 24.04 / 26.04をターゲットとします（対応保証はCIとVMテスト後）。systemd、OpenSSH Server（`UsePAM yes`）、amd64またはarm64、Discord Botが必要です。

## 現在の開発ブランチから（リリース前）

~~~bash
sudo apt-get install -y git build-essential pkg-config libpam0g-dev python3 python3-venv python3-pip
git clone https://github.com/disnana/DisnanaSASPAM.git
cd DisnanaSASPAM
# stable Rust / Cargo のインストールは公式 https://rustup.rs/ を参照
cargo test --workspace
cargo build --release --workspace
sudo bash scripts/install-from-build.sh
~~~

`install-from-build.sh` はソース・ビルド成果物を所定位置へ配置しますが、**PAMは変更しません**。

## 設定

1. Discord Developer PortalでBotを作成。Tokenは絶対にGitHubにコミットしない
2. Botを専用の非公開チャンネルへ招待し、閲覧・メッセージ送信・Embed・履歴参照の権限を付与
3. Discord開発者モードでGuild ID・Channel ID・**承認用Role ID（カンマ区切りで複数可）**を取得。専用チャンネルの閲覧権限をロールで制限
4. `sudo saspam setup` を実行し、Bot Token、Guild ID、Channel ID、複数Role ID、サーバー名、タイムアウト（10～180秒）を入力
5. `sudo saspam doctor` と `sudo saspam test`。DiscordのApproveボタンで成功することを確認
6. 既存SSHセッションとコンソール復旧経路を維持し `sudo saspam enable --ack-recovery --ack-tested`
7. 別SSHセッションから新しいログインでApprove / Deny / Timeoutを確認

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

`v0.1.0-alpha.1` はDiscordユーザーIDによる承認方式で、`alpha.2` はロールIDに切り替わります

1. **復旧用コンソール／既存SSHセッションを確保**
2. 更新前に`sudo saspam disable`し、PAMが無効化されていることを`sudo saspam status`で確認
3. 新バージョンのワンコマンドインストール（`--version v0.1.0-alpha.2`）を実行
4. `sudo saspam setup`で旧ユーザーID設定を新しいロールID設定へ置き換え
5. `sudo saspam doctor` → `sudo saspam test`で実承認を確認し、`enable`で再有効化
6. 別SSHセッションからログイン成功・拒否・期限切れを確認

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

`saspam test` はPAMを書き換えずにDiscordでの承認を待ちます。Bot停止時は事前チェックで中断します。復旧用コンソールを確認し、**実際にApprove（承認）できた後**でなければ`saspam enable`を実行しないでください

## v0.1.0-alpha.4 の対話設定と通知テスト

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

alpha.3には安全な更新コマンドがないため、復旧手段を確認してPAMを解除してからalpha.4のインストーラーを実行します。alpha.4以降の更新は `sudo saspam update` を利用し、PAMを無効化しない方式です。詳細は[更新ガイド](updates.md)を確認してください
