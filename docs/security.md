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

## Discord通知のメンション権限

- メンション対象：なし／設定済みの全承認ロール／`@everyone`。初期設定時の既定はロールメンション、`@everyone`には明示的な追加確認が必要
- ロールメンションはDiscordで実際に通知可能なBot権限（ロールのメンション許可や`MENTION_EVERYONE`）に依存
- `@everyone`は**通知だけ**。承認権限・PAM認可の判定には一切利用しない
- `AllowedMentions`で意図した対象のメンションだけ許可し、ユーザーが入力したSSH接続元データがメンションを発生させないようエスケープ
- 完了状態のメッセージ編集で追加メンションは送信しない。処理済みボタンは無効化
- `saspam-bot`以外のアカウントにvenvの読み書き権限を広げない。`/etc/saspam/discord.env`はroot専用に維持

## alpha.4 の構成変更と更新時の境界

- CLIの変更可能項目は言語・通知・メンション・承認ロール・承認期限・サーバー名に限定
- 更新中もSSHのPAM accountゲートは残し、Bot・デーモンを停止した期間はFail Closed
- 同時更新の排除、旧版バックアップ、独立したsystemd復旧タイマー、設定形式の検証を実施
- SHA-256の照合だけでは独立した署名検証にはならないため、正式版前に署名・provenanceと依存関係の監査が必要
- `--block-new-ssh` はまだ未検証のため拒否。誤って全SSH接続を遮断するファイアウォール設定は適用しない
- 物理・シリアルコンソールの通常のloginサービス、sudo、suのPAMは変更しない

更新・自動復旧の詳細は[updates.md](updates.md)
