# アーキテクチャと信頼境界

**日本語** | [English](en/architecture.md) · [ドキュメント一覧](README.md) · [README](../README.md)

- `pam_saspam.so`（Rust、PAM account）：`PAM_SERVICE` が `sshd` のときだけ動作し、root専用の `/run/saspam/pam.sock` に接続。サービス停止・タイムアウト・不正応答なら許可しない
- `saspamd`（Rust、root）：要求ID、期限、承認状態、許可されたDiscord Guild/Channel/Role IDを一元管理。承認は1回のみ、競合・期限切れは拒否
- `saspam`（Rust、root管理CLI）：設定、診断、テスト、`/etc/pam.d/sshd` の管理行追加・除去、OpenSSH攻撃対策
- `saspam-discord`（Python、専用非rootユーザー）：ローカル `bot.sock` の状態をポーリングし、DiscordのEmbed / ボタンを生成。DiscordのインタラクションのGuild/Channel/Memberロールとチャンネル閲覧権限をBot側で検証し、daemon側でも許可Role IDとの一致を確認
- `ntfy.sh` / Bark（HTTPS）：**通知配送のみ**。APIをインターネットへ公開したり、通知リンク自体を認可トークンとしたりしない

### 信頼境界

- PAM→daemon：ローカル Unix socket / root-only / `SO_PEERCRED` でUID確認
- Bot→daemon：`saspam-bot` グループ限定の別Socket。BotはPAM設定・rootファイルを編集できない
- Discord interaction：Guild/Channel・承認ロール照合、単回・期限内の決定
- 公開通知サービス：公開され得るメタデータのみ。認証用トークン、FIDO情報、秘密鍵、承認リンクの秘密値は送らない

### 保護範囲

`pam_sm_acct_mgmt` は認証済みアカウントのアクセス可否を判定するLinux-PAMインターフェース。SSHで `UsePAM yes` のときにpublickey/FIDO/password等の一次認証後のAccount段階へ組み込む。Console・sudo・suは対象外。認証方式自体の検査・変更は行わない。

### リソース消費と運用上の制限

PAMモジュールはDiscordやHTTPS通信を直接起動せず、root専用Unix Socketで結果を待ちます。待機時間が長くてもCPUを占有し続けるわけではありません。Rustデーモンは単一の非同期リアクターを使用し、未処理の要求数に上限を設けています。Botの状態照会は別の非rootプロセスで行います

**安全性の限界:** Botを侵害された場合はBot IPC経由のロール情報を偽装し得るため、daemon側のロールID照合だけでBot侵害を防げるわけではありません。署名検証、FIDO実機、実Discord API、PAM有効状態での異常復旧などは独立した確認が必要です

[セキュリティ](security.md) · [復旧](recovery.md) · [English](en/architecture.md)
