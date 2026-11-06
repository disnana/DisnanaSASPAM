# DisnanaSASPAM

**日本語** | [English](README.en.md)

**Disnana Simple Advanced Security PAM** — SSHの公開鍵・FIDO・パスワードによる**一次認証成功後**に、Discordで追加承認を求めるUbuntu向けPAMゲート

> **v0.1.0-alpha.5 / Developer Preview** — 独立したセキュリティ監査は未完了で、**本番SSHサーバーでの運用は推奨しません**。GitHub Actionsでは実際のSSH/PAM承認フローを検証済みですが、FIDO実機・Discord API全体のE2E・PAM有効状態での更新障害復旧には追加検証が必要です。必ず既存の管理用SSHセッションと、SSHに依存しない復旧用コンソールを確保してください

## 主な機能

- **一次認証後の承認** — `sshd`のPAM **account**フェーズでのみゲート。SSHの認証方式自体は置き換えない
- **物理コンソール対象外** — 変更するのは`/etc/pam.d/sshd`のみ。`login`・`sudo`・`su`・`common-account`は変更しない
- **Discordロール認可** — 承認ロールIDはカンマ区切りで複数指定可能。対象サーバー・チャンネルで、許可ロールを1つ以上持ち閲覧権限のあるメンバーのみ操作可能
- **日本語・英語** — 初回セットアップで選択。主要CLI、Discord Embed・ボタン、Bark/ntfy通知の言語を共通で切り替え
- **シンプルなボタン操作** — Discordのボタンは無言でDeferし、元のEmbedを編集して処理済みボタンを無効化。成功時の本人向け追加メッセージは出さない
- **通知先の追加** — ntfy.sh・BarkへのHTTPS通知（**承認機能はなし**）
- **Fail Closed** — 拒否・期限切れ・Bot停止・認証デーモン停止では勝手にログインを許可しない
- **操作メニューと管理機能** — 矢印キーのメニュー、部分設定変更、通知テスト、任意のSSH試行制限、更新・ダウングレード・ロールバック

### 動作の概要

```text
SSHの公開鍵 / FIDO / パスワード一次認証が成功
  └─ sshd の PAM account フック: pam_saspam.so (Rust)
      └─ /run/saspam/pam.sock（root専用）
          └─ saspamd（Rust製承認デーモン）
              ├─ /run/saspam/bot.sock → Discord Bot（Python・非root）
              │     └─ ロールとチャンネル権限を検証して承認 / 拒否
              └─ 任意の通知 → ntfy / Bark（通知専用）
                    ├─ 承認 → PAM_SUCCESS → SSHログイン
                    └─ 拒否・期限切れ・障害 → PAM_PERM_DENIED
```

Discord Botは通常のDiscord APIへ接続するため、Tailscale・NetBirdは必須ではありません。プッシュ通知から直接承認するWeb画面は、現在のAlpha版にはありません

## インストール（まずは使い捨てUbuntu VMで）

検証対象はUbuntu 22.04・24.04のamd64・arm64、systemd、OpenSSH Serverです。その他のバージョンはすべての組み合わせを検証しているわけではありません

**公開中のAlpha版はバージョンを明示して導入してください**

```bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v0.1.0-alpha.5
```

GitHub Releaseのアーカイブと`SHA256SUMS`を取得・照合します。**インストールだけではSSHのPAM保護は有効化されません**。正式安定版のReleaseはまだないため、バージョン省略で安定版を取得する運用はできません

rootでリモートスクリプトを実行する前に、保存して中身を確認することも推奨します。SHA-256は同じ公開元のチェックサムとの照合であり、**独立した発行者署名の検証ではありません**

## 初期設定と承認テスト

Discord Developer PortalでBotを作成し、Bot Token、Guild（サーバー）ID、通知Channel ID、承認Role IDを用意してください。通知先は専用の非公開チャンネルが推奨です。Botにチャンネル閲覧・送信・Embed・履歴参照の権限を付与します。特権のMessage Content Intentは不要です

```bash
sudo saspam setup       # 初回言語・Discord・通知設定
sudo saspam doctor      # sshd、PAM、認証デーモン、Botの状態確認
sudo saspam test        # PAMを変更せず、Discordの承認ボタンをテスト
```

承認ロールは複数指定できます。以下では**どちらか1つのロール**があれば承認・拒否できる方式（OR条件）

```text
Discord server (guild) ID: 111111111111111111
Discord notification channel ID: 222222222222222222
Allowed Discord role IDs: 333333333333333333, 444444444444444444
```

実際のロール・チャンネルの閲覧権限も必要です。`@everyone`ロールを承認ロールとして指定することはできません

メンションは「**承認ロール全員**（標準）」「**@everyone**（追加確認あり）」「**なし**」から選択できます。**メンション先と承認権限は別**であり、`@everyone`へ通知しても全員がSSHログインを承認できるようにはなりません。実際のメンション通知はBot側のDiscord権限にも依存します

### SSH保護を有効にする

`sudo saspam test`で本当にDiscord承認できたことを確認し、**現在のSSHセッションを閉じずに**コンソールなどSSH以外の復旧手段を確保してください。その後に実行します

```bash
sudo saspam enable --ack-recovery --ack-tested
```

現在の設定に対する直近15分以内の承認テスト成功が必要です。有効化後は**別ターミナルから新しいSSH接続**を試し、承認・拒否・期限切れの挙動を確認してください

SASPAMによるSSH保護だけを解除する場合：

```bash
sudo saspam disable
sudo saspam status
```

## TestDisk風メニューと設定の部分変更

```bash
sudo saspam                         # ↑↓ / Enter / Space / Esc
sudo saspam menu                    # メニューを明示起動
sudo saspam config                  # 必要な設定だけを選んで変更
sudo saspam config show             # 秘密鍵などを隠して設定確認
sudo saspam config set language en  # CLI・Bot・通知を英語に変更
sudo saspam config set language ja
sudo saspam config set mentions roles
sudo saspam config set mentions none
sudo saspam config set timeout 60
sudo saspam config set roles 333333333333333333,444444444444444444
sudo saspam config set ntfy topic --generate
sudo saspam config set ntfy on
sudo saspam config set bark key     # デバイスキーは非表示で対話入力
sudo saspam config set bark on
sudo saspam notify test ntfy
sudo saspam notify test bark
sudo saspam notify test all
```

Bark・ntfyは**通知専用**です。公開プッシュサービスへは接続元IP・ホスト名などの情報が送られる場合があるため、プライバシーに注意してください

SSH保護中の設定変更はサービスの再起動で一時的に承認要求が拒否されることがあります。CLIは変更前の確認を求め、検証・再起動に失敗した場合は旧設定への復旧を試みます。無停止変更の保証ではありません

### 認証方式の表示（alpha.5）

Discordの承認Embedには、接続元・ユーザー名に加えて **SSHの一次認証方式** を表示します。OpenSSHのPAM環境 `SSH_AUTH_INFO_0` を読み、公開鍵、パスワード、FIDO鍵、keyboard-interactive、複数方式を区別します。取得できなければ「不明（取得不可）」と表示し、推測はしません

SSH公開鍵の内容・指紋・パスワードは通知に含めません。パーサーの単体テストを追加済み。ただしUbuntu 22.04/24.04の実SSH/PAMテストではOpenSSHからこの情報が空で渡されるため、現状は「不明」と表示されます。確実な認証方式取得とFIDO実機検証は未完了です

### バージョン不要の更新（alpha.5）

```bash
sudo saspam update            # 最近のRelease一覧から選択
sudo saspam update --list     # 利用可能な版と現在との差を表示
sudo saspam update --latest   # 最新の配布版を自動選択
```

対話メニューの「更新・ダウングレード」からも選択できます。古いバージョンを選んだ場合は確認が必要です。現在の版を再配置するには `--allow-reinstall` を使います。各Releaseのチェックサムとアーキテクチャ互換性を確認したうえで、従来のFail Closed更新機構に処理を渡します

## 更新・ダウングレード・自動ロールバック

```bash
sudo saspam update --version v0.1.0-alpha.5 --dry-run
sudo saspam update --version v0.1.0-alpha.5 --allow-reinstall
sudo saspam update --version v0.1.0-alpha.3 --allow-downgrade
sudo saspam update --rollback
```

更新処理は、新版の事前検証、旧版と設定のバックアップ、サービス切替、承認テスト、確定の順。独立したsystemdタイマーが未完了更新を検知した場合は旧版への復元を試みます。**更新中もSSHのPAMフックは解除せず、承認できない接続は拒否**する構成です

**alpha.3からalpha.4へ最初に移行する場合**は、alpha.3に更新機構がないため、復旧経路を確保したうえで`sudo saspam disable`してからバージョン指定インストーラーを実行してください。以降の更新は`saspam update`を使用できます

`--block-new-ssh`は未提供で、指定するとエラーになります。ファイアウォールを使った新規SSH接続の一時遮断は安全性検証後に対応する方針です。[詳細な更新・復旧手順](docs/updates.md)

## 任意のSSH攻撃対策

```bash
sudo saspam security audit
sudo saspam security apply --ack-recovery
sudo saspam security rollback
```

現在の任意設定は`MaxAuthTries 4`・`LoginGraceTime 45`・`MaxStartups 10:30:60`です。IPごとの時間制BANではありません。SSHポート・パスワード認証・rootログインを勝手に変更しません

**SASPAMの標準承認待ち60秒と`LoginGraceTime 45`の整合性は、環境で別途確認が必要**です。実機での挙動を確認するまで`security apply`を安易に実行しないでください

## 障害・復旧・制限事項

SSHへ新しくログインできない場合、既存の管理用セッションまたは実際のコンソールから：

```bash
sudo saspam disable
sudo /usr/sbin/sshd -t
sudo saspam status
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
```

Botが停止しているときは勝手に承認されず、未承認要求は期限切れになります。認証デーモン停止時はPAMが拒否します。既存のSSHセッションや多重化済みのチャネルには再承認を要求しません

実Discord APIのすべての経路、FIDOハードウェア、PAM有効状態の障害復旧、独立したセキュリティ監査、署名・provenance検証は安定版までの課題です。DockerによるBot分離やVPN経由のBark/ntfy承認画面はalpha.4では提供していません

## ドキュメント / Documentation

| 内容 | 日本語 | English |
|---|---|---|
| README | このページ | [English README](README.en.md) |
| インストール・設定 | [日本語](docs/installation.md) | [English](docs/en/installation.md) |
| 構成・権限境界 | [日本語](docs/architecture.md) | [English](docs/en/architecture.md) |
| ロックアウト時の復旧 | [日本語](docs/recovery.md) | [English](docs/en/recovery.md) |
| セキュリティと制限 | [日本語](docs/security.md) | [English](docs/en/security.md) |
| 更新・ダウングレード | [日本語](docs/updates.md) | [English](docs/en/updates.md) |

[日英ドキュメント一覧](docs/README.md)

## ライセンス・脆弱性報告

MIT License。脆弱性の悪用可能な詳細は公開Issueへ書かず、GitHubの非公開脆弱性報告など適切な非公開窓口を利用してください
