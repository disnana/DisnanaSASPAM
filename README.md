# DisnanaSASPAM

**Disnana Simple Advanced Security PAM** — SSH認証に成功した**後**、Discordで承認してからログインさせる、Ubuntu向けの軽量PAMゲートです。

> **Status: v0.1.0-alpha.3 / developer preview.** 未監査のAlpha版です。Ubuntu CI上でのSSH/PAM統合テストを実施済みですが、FIDOハードウェアとDiscord本番APIを使ったE2Eテストは未実施です。本番SSHに組み込む前に隔離VMと復旧手段を用意してください。

## 特徴

- **Post-authentication**: SSH公開鍵・FIDO・パスワードなど、一次認証成功後のPAM **account** フェーズに承認チェックを追加
- **コンソールは対象外**: `/etc/pam.d/sshd` のみ変更。`login`・`sudo`・`su` は保護対象外
- **Discordで承認と記録**: 許可したロール（複数指定可）を持ち、専用チャンネルを閲覧できるメンバーだけが「承認 / 拒否」を操作可能。拒否・期限切れはログイン不可
- **Rust中心**: PAMモジュール、Unix socket認証デーモン、管理CLI。Discord Botは非rootのPythonプロセス
- **通知を追加**: ntfy / Barkへの通知をオプションで送信可能（**初期版では通知のみ**）
- **安全に導入・解除**: 既存PAM設定を変更するのは、明示的な `saspam enable` の実行時だけ
- **おまけのSSH攻撃対策**: `saspam security` から保守的な接続・認証試行制限を確認、適用、戻す

## 動作イメージ

~~~text
SSH接続
  └─ SSH公開鍵 / FIDO / パスワード認証に成功
       └─ /etc/pam.d/sshd → pam_saspam.so（Rust、accountのみ）
            └─ /run/saspam/pam.sock → saspamd（Rust、root）
                 ├─ Discord Bot（Python、非root）へ通知 → 承認 / 拒否
                 └─ ntfy / Barkへ通知（任意、初期版は通知のみ）
                      └─ 承認あり → PAM_SUCCESS → SSHログイン
                      └─ 拒否 / タイムアウト / 障害 → PAM_PERM_DENIED
~~~

Discord Botは通常のDiscord APIへ接続するため、**TailscaleやNetBirdは必須ではありません**。ntfy / Barkの**通知のみ**ならVPNは不要です。Tailscale / NetBird内のWeb承認APIは将来の開発対象であり、**このalphaには未搭載**です。

## ワンコマンドインストール

**GitHub Releasesで公開中のAlpha版は、バージョン指定でインストールできます。**スクリプトはUbuntu amd64 / arm64向けReleaseアーカイブと`SHA256SUMS`を取得・照合して配置します。**インストールしただけではSSHを保護しません。**

~~~bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash
~~~

**プレリリース版をテスト用Ubuntuに導入する場合（Release公開後）**:

~~~bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v0.1.0-alpha.3
~~~

このコマンドもインストールしただけではPAMを有効化しません。正式な安定版のReleaseがまだない間は、バージョン省略のワンコマンドは安全のため失敗します。**Alphaは本番サーバーでの使用を推奨しません。**

特定バージョンを指定する場合:

~~~bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v0.1.0
~~~

**現時点で安定版のReleaseはありません。上記v0.1.0指定は失敗して終了します。** それが意図した安全動作です。初期開発版は [開発者向けセットアップ](docs/installation.md) を参照してください。

> 遠隔スクリプトをrootで直接実行するのが不安な場合は、先に`scripts/install.sh`を保存・確認し、その後に`sudo bash scripts/install.sh`を実行してください。SHA-256照合はファイル破損の検出であり、独立した署名検証とは異なります。

## 初期設定（必ず既存SSHセッションを残す）

Discord Developer PortalでBotを作成し、Bot Token・Guild（サーバー）ID・通知先Channel ID・承認を許す**ロールID（数値・複数ならカンマ区切り）**を用意します。専用ロールだけが通知チャンネルを閲覧できるようDiscord側で設定してください。Botにはチャンネル閲覧、メッセージ送信、Embed送信、履歴参照の権限を付与します。Message Content Intentは不要です。

~~~bash
sudo saspam setup       # 対話形式で入力、設定を保存しサービスを起動
sudo saspam doctor      # sshd / PAM / IPC / Botの状態診断
sudo saspam test        # 本物のDiscord承認を試す。ここではPAMは変更しない
sudo saspam enable --ack-recovery --ack-tested
~~~

最後のコマンドは、**直近15分以内に成功した承認テスト**と、管理者の復旧経路確認を要求します。初回の有効化後も**既存SSHセッションを閉じずに**、別ターミナルからSSHでログインし、承認・拒否・期限切れをテストしてください。

### Discordロールによる承認

`sudo saspam setup` で、例えば次のように入力する

```text
Discord server (guild) ID: 111111111111111111
Discord notification channel ID: 222222222222222222
Allowed Discord role IDs: 333333333333333333, 444444444444444444
```

上記では **どちらか1つのロールを持てば承認・拒否できる（OR条件）**。ロールだけではなく、操作するDiscordサーバー・チャンネルの一致と`VIEW_CHANNEL`権限も必要。BotはDiscordのGuild Member interactionに含まれるロール情報を使用し、rootの認証デーモンは設定済みロール／チャンネル／サーバーIDを照合する。`@everyone`ロールを許可することはできない

個別のDiscordユーザーIDを登録する必要はない。ただし、専用ロールを付与できる管理者にも実質的に承認権限を付与できるため、Discordのロール管理権限は慎重に制限すること

**Bot停止・Discord接続断の場合は自動承認しない。** 既定60秒の承認期限を過ぎるとSSHログインを拒否する。認証デーモンが停止している場合はPAMが拒否する

### v0.1.0-alpha.1からの移行

旧バージョンでは承認するユーザーIDを設定していた。新方式は`allowed_role_ids`、`guild_id`、`channel_id`が必須になり、**旧設定は新デーモンでは受理されない（Fail Closed）**

以前のAlpha版が有効な場合は、管理用SSHセッションまたはクラウドコンソールを保持したまま、先に `sudo saspam disable` し、更新後 `sudo saspam setup` でロール設定を再入力すること。セットアップテスト成功後にのみ再度 `enable` する。インストーラーはPAMが有効なままの上書き更新を拒否する

### Discord通知のメンション設定と日本語表示

`sudo saspam setup` で通知メンションを次の3通りから選択できます

| 選択 | メンション対象 | 備考 |
|---|---|---|
| **1: 承認ロール全員（既定）** | `<@&ロールID>` を承認ロールの数だけ送信 | 該当ロールを持つ人へ通知（Botのメンション権限が必要な場合あり） |
| **2: @everyone** | `@everyone` を送信 | **明示的な二段階確認が必要**。チャンネルを閲覧できるメンバーへ通知 |
| **3: なし** | メンションなし | ログのみ残す |

通知の対象と承認できる権限は**別**です。`@everyone`で通知しても、実際に承認・拒否できるのは設定済みロールを持ち、対象チャンネルを閲覧できるメンバーだけ。Discordのメンション権限がない場合は通知が届かないことがあります

初代版のように、リクエスト発生時は黄、承認は緑、拒否は赤、期限切れは灰色の日本語Embedを表示します。同じメッセージを状態更新し、処理済みの承認・拒否ボタンは無効化します

```text
🔐 SSHログイン承認リクエスト
承認待ち — SSHの一次認証に成功した接続があります

ユーザー ubuntu が 100.92.24.113 からログインを試みています
🖥️ 接続先サーバー: example-server
🌐 接続元IP / ホスト: 100.92.24.113
📅 要求日時 / ⏳ 承認期限
セッションID: ...

[承認] [拒否]
```

### Botが起動しない / `saspam test`が進まない

`Testing real Discord approval without editing PAM` は、SSH設定を変更せずに**Discord上で承認が押されるのを待つ**テストでした。Botが起動していないと通知が届かず、従来は期限まで待ち続けていました

alpha.3からはBot停止時の事前チェックを行い、**すぐにエラー表示してテストを中断**します。実ログインの許可は行いません

```bash
sudo systemctl status saspam-discord --no-pager -l
sudo journalctl -u saspam-discord -n 80 --no-pager -o cat
sudo namei -l /opt/saspam/venv/bin/python
sudo runuser -u saspam-bot -- /opt/saspam/venv/bin/python -c 'import discord; print(discord.__version__)'
```

旧インストーラーは`umask 077`でvenvを作り、非rootのBotから`203/EXEC Permission denied`が起こることがありました。新インストーラーでは**Python仮想環境のみをroot:saspam-botグループから実行可能**にし、インストール時にBotユーザーで実行テストします。`/etc/saspam`内の秘密情報はroot専用のままです

**既に保護を有効化しているホストでは、復旧用コンソールとSSHセッションを残し、必ず `sudo saspam disable` の後に更新してください。** 新しいAlpha版を導入しても自動でPAMを有効化しません

### よく使うコマンド

| コマンド | 用途 |
|---|---|
| `saspam help` | 全コマンド表示 |
| `sudo saspam setup` | Discord / ntfy / Bark設定を対話形式で作成 |
| `sudo saspam doctor` | 安全性・サービス・設定診断 |
| `sudo saspam test` | PAMを有効化せず承認動作確認 |
| `sudo saspam status` | PAM保護とサービスの稼働確認 |
| `sudo saspam enable --ack-recovery --ack-tested` | SSHだけに承認ゲートを追加 |
| `sudo saspam disable` | SASPAMが管理するPAM行だけ除去 |
| `sudo saspam security audit` | SSH実効設定とFail2ban稼働状況の診断 |
| `sudo saspam security apply --ack-recovery` | SSHの保守的な試行制限を適用 |
| `sudo saspam security rollback` | 上記のSSH試行制限設定を削除 |
| `sudo saspam uninstall --yes` | PAM解除後にサービス・バイナリを除去 |

`security apply` はSSHパスワード認証やrootログインを勝手に無効化しません。既存のSSH制御設定との衝突をチェックし、検証に失敗した場合は適用を中止します。

### 設定とログ

- `/etc/saspam/config.toml`: 承認時間、サーバー名、通知先（root専用）
- `/etc/saspam/discord.env`: Bot Token・Channel ID・許可ユーザーID（root専用）
- `/run/saspam/pam.sock`: root専用のPAM IPC
- `/run/saspam/bot.sock`: 専用Botグループ限定のIPC
- `journalctl -u saspamd -u saspam-discord -f`: サービス・承認イベントログ
- `/var/lib/saspam/sshd.pam.backup`: 初回有効化時のPAMバックアップ

通知先の追加・変更は `sudo saspam setup` から。**SSH保護を無効にしてから**実施してください。Discordへの通知は便利な閲覧用記録ですが、削除・編集が可能なので、監査はローカルjournalも併用します。

## 困ったとき

~~~bash
# 既存SSHセッションかクラウドコンソールから、まず保護だけ解除
sudo saspam disable
sudo saspam status
sudo /usr/sbin/sshd -t

# ログ確認
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
~~~

インストール時にPAMは書き換えません。初期版は通信断でも**Fail Closed**（ログイン拒否）なので、必ず**クラウドのシリアルコンソール等の復旧経路**を確認してください。詳しくは [復旧手順](docs/recovery.md)。

## 対応範囲と制限

| 項目 | v0.1.0-alpha.2 |
|---|---|
| Ubuntu / systemd / amd64・arm64 | Ubuntu 22.04/24.04のSSH統合テストをCIで実施（実機・FIDOは未検証） |
| SSH一次認証後にDiscord承認 | 実装対象 |
| 拒否 / 期限切れ / Bot停止時の拒否 | 実装対象 |
| ntfy / Bark | **通知のみ** |
| ntfy / BarkのWeb承認（Tailscale / NetBird） | **未実装** |
| 全ホスト向けの公開API | なし |
| コンソール・sudo・suへの保護 | なし |
| 自動マージ / 本番運用保証 | なし |

## ドキュメント

[インストールと開発版](docs/installation.md) · [構成](docs/architecture.md) · [復旧](docs/recovery.md) · [セキュリティ](docs/security.md)

## License

MIT License. 脆弱性の報告は公開Issueではなく、GitHubのprivate vulnerability reportingなど非公開の連絡手段でお願いします。
