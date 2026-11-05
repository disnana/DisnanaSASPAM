# DisnanaSASPAM

**Disnana Simple Advanced Security PAM** — SSH認証に成功した**後**、Discordで承認してからログインさせる、Ubuntu向けの軽量PAMゲートです。

> **Status: v0.1.0-alpha.1 / developer preview.** 現在は未監査の開発版です。SSHへ組み込む前に必ず隔離VMで検証してください。正式Releaseの配布・実機での認証テストはまだ完了していません。

## 特徴

- **Post-authentication**: SSH公開鍵・FIDO・パスワードなど、一次認証成功後のPAM **account** フェーズに承認チェックを追加
- **コンソールは対象外**: `/etc/pam.d/sshd` のみ変更。`login`・`sudo`・`su` は保護対象外
- **Discordで承認と記録**: 許可されたユーザーのみ「Approve / Deny」を操作可能。拒否・期限切れはログイン不可
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

**正式なGitHub Releaseの公開後**に利用可能となる予定です。スクリプトはUbuntu amd64 / arm64向けReleaseアーカイブと`SHA256SUMS`を取得・照合して配置します。**インストールしただけではSSHを保護しません。**

~~~bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash
~~~

**プレリリース版をテスト用Ubuntuに導入する場合（Release公開後）**:

~~~bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v0.1.0-alpha.1
~~~

このコマンドもインストールしただけではPAMを有効化しません。正式な安定版のReleaseがまだない間は、バージョン省略のワンコマンドは安全のため失敗します。**Alphaは本番サーバーでの使用を推奨しません。**

特定バージョンを指定する場合:

~~~bash
curl -fsSL https://raw.githubusercontent.com/disnana/DisnanaSASPAM/main/scripts/install.sh | sudo bash -s -- --version v0.1.0
~~~

**現在は安定Releaseがないため、上記コマンドは失敗して終了します。** それが意図した安全動作です。初期開発版は [開発者向けセットアップ](docs/installation.md) を参照してください。

> 遠隔スクリプトをrootで直接実行するのが不安な場合は、先に`scripts/install.sh`を保存・確認し、その後に`sudo bash scripts/install.sh`を実行してください。SHA-256照合はファイル破損の検出であり、独立した署名検証とは異なります。

## 初期設定（必ず既存SSHセッションを残す）

Discord Developer PortalでBotを作成し、Bot Token・通知先Channel ID・承認を許す**ユーザーID（数値）**を用意します。Botにはチャンネル閲覧、メッセージ送信、Embed送信、履歴参照の権限を付与します。Message Content Intentは不要です。

~~~bash
sudo saspam setup       # 対話形式で入力、設定を保存しサービスを起動
sudo saspam doctor      # sshd / PAM / IPC / Botの状態診断
sudo saspam test        # 本物のDiscord承認を試す。ここではPAMは変更しない
sudo saspam enable --ack-recovery --ack-tested
~~~

最後のコマンドは、**直近15分以内に成功した承認テスト**と、管理者の復旧経路確認を要求します。初回の有効化後も**既存SSHセッションを閉じずに**、別ターミナルからSSHでログインし、承認・拒否・期限切れをテストしてください。

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

| 項目 | v0.1.0-alpha.1 |
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
