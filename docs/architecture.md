# Architecture

- `pam_saspam.so`（Rust、PAM account）：`PAM_SERVICE` が `sshd` のときだけ動作し、root専用の `/run/saspam/pam.sock` に接続。サービス停止・タイムアウト・不正応答なら許可しない
- `saspamd`（Rust、root）：要求ID、期限、承認状態、許可されたDiscord User IDを一元管理。承認は1回のみ、競合・期限切れは拒否
- `saspam`（Rust、root管理CLI）：設定、診断、テスト、`/etc/pam.d/sshd` の管理行追加・除去、OpenSSH攻撃対策
- `saspam-discord`（Python、専用非rootユーザー）：ローカル `bot.sock` の状態をポーリングし、DiscordのEmbed / ボタンを生成。Discordのインタラクション発信者IDをデーモン側のallowlistと照合
- `ntfy.sh` / Bark（HTTPS）：**通知配送のみ**。APIをインターネットへ公開したり、通知リンク自体を認可トークンとしたりしない

### Trust boundaries

- PAM→daemon：ローカル Unix socket / root-only / `SO_PEERCRED` でUID確認
- Bot→daemon：`saspam-bot` グループ限定の別Socket。BotはPAM設定・rootファイルを編集できない
- Discord interaction：許可したDiscord User IDの照合と単回・期限内の決定
- 公開通知サービス：公開され得るメタデータのみ。認証用トークン、FIDO情報、秘密鍵、承認リンクの秘密値は送らない

### Scope

`pam_sm_acct_mgmt` は認証済みアカウントのアクセス可否を判定するLinux-PAMインターフェース。SSHで `UsePAM yes` のときにpublickey/FIDO/password等の一次認証後のAccount段階へ組み込む。Console・sudo・suは対象外。認証方式自体の検査・変更は行わない。
