# Security model & limitations

> **Pre-release, unaudited.** 実機でのPAM認証・Ubuntu複数版の統合テストが完了するまでは、本番投入しないでください。

## 守るもの

- 一次認証後の追加承認（公開鍵・FIDO・パスワードの認証自体を置き換えない）
- 要求IDの単回性、期限切れ、拒否、認証プロセス障害時のFail Closed
- 別Unix socketによるroot PAM処理と非root Discord Botの分離
- PAM更新時のバックアップと最小差分、`sshd -t` と既存セッションでの手動検証

## 守れないもの

- rootを奪取されたサーバー（rootはPAMを編集できる）
- Discordの承認者アカウント / Botトークンの侵害（攻撃者が承認できる可能性がある）
- すでに確立済みのSSH接続や多重化されたチャネル
- Discordのログ完全性（メッセージ削除・編集可能）。Ubuntuのjournalも別途保管を推奨
- SSHの認証失敗を追加承認で止めること（攻撃対策は`security`サブコマンド側）

## 注意

- 公開ntfy.shトピックは秘密保管場所ではない。推測しづらいトピックを使用し、IP等のメタデータ漏洩も認識する
- Bark device keyとDiscord Bot Tokenは機密。`/etc/saspam` はroot専用、GitHubへ公開しない
- `security apply` はOpenSSHの `MaxAuthTries 4` / `LoginGraceTime 45` / `MaxStartups 10:30:60` を提案する。既存設定との競合は自動で検知して中止
- `PasswordAuthentication` / `PermitRootLogin` は変更しない。Fail2banのインストールも自動では行わない
- IPv4/IPv6、ホスト・ユーザー別Match条件、cloud-init・既存SSH管理ツールとの競合は手動で確認
- 安全な配布は署名済みReleaseを目標とする。初期SHA-256は独立した発行者署名ではない

脆弱性はIssueで公開せず、GitHubのprivate vulnerability reporting等で非公開報告をお願いします。


## Discordロール承認

- Guild ID・Channel ID・メンバーのロールIDが設定と一致し、チャンネル閲覧権限がある場合のみ承認可能
- 複数ロールはOR判定。専用ロールのどれか1つで許可する
- `@everyone`ロールは承認ロールとして指定できない
- Discord Gatewayから受け取ったMemberロール情報を非root Botが照合し、root daemonが許可ロールと一致を再確認
- Botを侵害された場合はIPC経由でロール情報を偽装できる危険が残る。Botトークン、専用プロセス、Unix Socketの権限を保護する
- Discord Botが死んでいてもFail Closed。デーモンが動作中なら期限切れまで待って拒否。デーモンがない場合は直ちにPAMが拒否
