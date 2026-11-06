# SSH auth.log / journald audit (research only)

[日本語](#日本語) | [English](#english)

## 日本語

これは以前の SSH 認証通知 Python スクリプトをもとに作成した、**SSHログの読み取り専用監査機能**です。**SASPAMの認証可否やDiscordの承認処理には使いません**。Alpha.6では独立したReleaseアセットとして配布しますが、自動インストール、サービス登録、SSH承認システムへの組み込みは行いません。

### 認証ログの注意点

[Ubuntu 22.04 / 24.04 の実SSH検証](https://github.com/disnana/DisnanaSASPAM/actions/runs/42212716858)では、Discord承認待ちの間には Accepted publickey が観測できず、承認後にはじめて出力されました。承認前に見えた authenticated 0 は「鍵の提示」であり、認証成功ではありません。

OpenSSHソースで auth_log の呼び出しが PAM account より前でも、privileged monitor とログ転送の都合で、実際に監視プロセスから観測できる時刻は異なります。**Acceptedログを未承認カードの認証方式として採用したり、ユーザー名・IPだけで他の接続と関連付けたりしないでください**。Acceptedログも「セッション確立済み」そのものの証明ではありません。

### 手動で試す

ソースチェックアウトに加え、Alpha.6以降はReleaseから単体ファイルを取得できます。配布物と同じReleaseのチェックサムを照合してください（独立した発行者署名ではありません）。

```bash
TAG=v1.0.0-beta.2
BASE="https://github.com/disnana/DisnanaSASPAM/releases/download/$TAG"
curl --proto '=https' --tlsv1.2 -fsSLO "$BASE/SHA256SUMS"
curl --proto '=https' --tlsv1.2 -fsSLO "$BASE/ssh_auth_events-$TAG.py"
grep -F "  ssh_auth_events-$TAG.py" SHA256SUMS | sha256sum -c -
sudo python3 "ssh_auth_events-$TAG.py" --journal-follow
```

以下はリポジトリのルートで実行する例です。デフォルトでは外部送信しません。

    sudo python3 tools/ssh_auth_events.py --file /var/log/auth.log
    sudo python3 tools/ssh_auth_events.py --journal-follow

Discordへの**別の監査通知**を有効にする場合だけ、非公開の環境変数 SASPAM_AUDIT_DISCORD_WEBHOOK に Discord Webhook URL を設定して、明示的に --discord を付けます。

    sudo --preserve-env=SASPAM_AUDIT_DISCORD_WEBHOOK python3 tools/ssh_auth_events.py --journal-follow --discord

Webhook URLをコマンド引数、ソースコード、GitHubに置かないこと。旧コードに含まれたWebhookがまだ有効なら再生成してください。Discordの allowed_mentions は空、通知は毎分最大30件、90秒以内の同一イベントは重複除外します。

journal JSONを処理する際は、呼び出し元が自由に設定できる SYSLOG_IDENTIFIER だけでなく、systemdが付与する _EXE が既知のOpenSSH実行ファイルであることも確認します。これでも監査ログはログイン許可の根拠にはできません。

取得できるのはユーザー名、接続元IP、接続元ポート、認証方式、あればjournalのPIDだけ。秘密鍵、公開鍵指紋、パスワード、元ログは転送しません。鍵アルゴリズムがログに無ければ公開鍵認証という分類に留めます。ECDSA-SK、ED25519-SKを区別します。OpenSSH/PAM、SSH設定、Firewallの変更はありません。

### 未完了

- 本番環境のログ形式との照合（特にUbuntu 26.04、物理FIDO鍵、Termius）
- 接続単位の厳密な照合と、承認後の既存Embed更新の設計
- 最小権限のjournal閲覧サービスと安全なインストール・ロールバック
- Discordへの実際の配信テスト、同時接続、ログ欠落、更新後の回帰テスト

## English

This is an **optional, read-only** log audit based on the operator's 2025 Python SSH notification scripts. It is not an SASPAM authorization method and cannot approve or deny any SSH connection. As of alpha.6 the script is published as a separate, checksum-listed optional release asset. It is not installed or started by default.

The [live Ubuntu 22.04/24.04 SSH experiment](https://github.com/disnana/DisnanaSASPAM/actions/runs/42212716858) did not expose Accepted publickey to the log observer until **after** the PAM account approval was released. An earlier authenticated 0 line was an unverified key offer. Do not assume log arrival before approval merely because OpenSSH source invokes auth_log earlier. Do not correlate sessions using only username/IP.

Run manually:

    sudo python3 tools/ssh_auth_events.py --file /var/log/auth.log
    sudo python3 tools/ssh_auth_events.py --journal-follow

For explicitly opted-in **separate audit notifications** set a private SASPAM_AUDIT_DISCORD_WEBHOOK environment variable and add --discord. The Discord webhook is HTTPS-only; user mentions are disabled; duplicate events and excess notifications are suppressed. The output is a sanitized JSON observation with method, user, source IP/port and optional journal PID; never a credential or full log. In the absence of an algorithm, publickey remains generic.

Journal JSON is accepted only when the systemd-provided _EXE field identifies a known OpenSSH executable; the user-settable SYSLOG_IDENTIFIER alone is not trusted.

An Accepted line is not a reliable input for a pending approval card and does not itself prove a fully established SSH session. This code does not modify the PAM gate, sshd, firewall, or systemd units. Real deployments, exact connection binding, privilege isolation, concurrent-session tests, real FIDO hardware, and upgrade/rollback support are still pending.

### Offline tests

    python3 -m unittest discover -s tests -p 'test_ssh_auth_events.py' -v
    python3 -m py_compile tools/ssh_auth_events.py
