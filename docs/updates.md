# 更新・ダウングレード・自動復旧（v1.0.0）

**日本語** | [English](en/updates.md) · [ドキュメント一覧](README.md) · [README](../README.md)

DisnanaSASPAMの更新処理は **SSHのPAM保護を解除しない** ことを前提にしています。デーモンが停止している間に届いたSSH承認要求は、Fail Closedで拒否されます。

## 前提条件

- Ubuntu 22.04 / 24.04（amd64 / arm64）およびsystemd
- 復旧用の既存SSHセッションと、クラウドのコンソール・シリアルコンソール
- `sudo saspam doctor` と `sudo saspam test` で正常性を確認できること
- v1.0.0も独立したセキュリティ監査は未完了で、本番SSHへの導入は推奨しません。まず使い捨てUbuntu VMで更新と復旧を検証

## コマンド

alpha.4以降の、v1.0.0より古い版から更新する例です。alpha.3以前は下記の別手順を使ってください

```bash
sudo saspam update --version v1.0.0 --dry-run
sudo saspam update --version v1.0.0
```

現在のバージョンと同じ版を再配置する場合は `--allow-reinstall` が必要です。同じ版の事前検証も `sudo saspam update --version v1.0.0 --allow-reinstall --dry-run` と指定します。`--rollback`は前回確定した更新のバックアップへ戻す操作で、初回インストールの取り消しではありません。`--skip-approval-test` はSSHのPAM保護が **無効なときに限って** 利用できます

旧版へ変更する場合だけ、互換性を確認した対象タグと`--allow-downgrade`を明示します。Alpha版への変更は通常の更新手順ではありません。現在の設定や更新・復旧プロトコルに対応しない版を強制導入しないでください

## 失敗時の復旧

1. 更新前に旧バイナリ、PAMモジュール、Python環境、設定、systemdユニットをroot専用ディレクトリへバックアップ
2. 新版ReleaseのアーカイブとSHA-256を検証し、デーモンの `--check-config` とPythonコード構文を確認
3. systemdの独立したタイマーを登録。更新プロセスが異常終了しても旧版復旧を試行できるようにする
4. デーモンとBotを停止、新版へ切替・再起動。**PAMの設定行は削除しない**
5. Discordの承認テストに成功したら更新確定。失敗時は旧版を復元しサービスを再起動

復旧処理そのものが失敗した場合、勝手にPAMを無効化せず、SSHはFail Closedで拒否されます。復旧用コンソールからログを確認してください

```bash
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
sudo saspam update --rollback
sudo saspam status
```

## alpha.3以前からの移行（更新機構がない場合）

alpha.3以前には更新・復旧コマンドがありません。以下はその旧版だけの手順で、alpha.4以降の通常更新ではPAMゲートを解除しません。すべての旧版からの移行を実機検証済みという意味ではなく、まず使い捨てVMで設定の互換性と復旧を確認してください

1. 復旧用コンソールと既存の管理SSHセッションを維持し、設定を秘密情報として安全にバックアップ
2. `sudo saspam disable`後に`sudo saspam status`で解除を確認。この間はSASPAMの追加承認がないため、移行中の接続を管理できる環境で実施
3. [導入ガイド](installation.md)の`--version v1.0.0`を指定したインストーラーを実行
4. **すべての旧版移行で`sudo saspam setup`を実行**し、設定を確認・保存してデーモンとBotを再起動。インストーラーだけでは稼働中の旧プロセスを再起動しません。alpha.1の旧ユーザーID設定は互換性がないため、ロールID方式へ置き換え
5. `sudo saspam doctor`、実際のDiscord承認を伴う`sudo saspam test`を通過させてから、`sudo saspam enable --ack-recovery --ack-tested`で再有効化
6. 元の管理セッションを閉じず、別SSH接続で承認・拒否・期限切れを確認

alpha.4からは上記の`--version v1.0.0`による更新を使用します。alpha.5を中間版として導入する必要はありませんが、対象版と設定の事前検証は必要です

## SSH新規接続の一時遮断について

`--block-new-ssh` は現状 **未提供**。指定するとエラー終了します。sshポート・nftables・IPv4/IPv6・Tailscale/NetBirdの条件で誤って管理者を締め出すリスクがあるため、タイマーによる自動解除と実接続の回帰テストが完成するまで実装を有効化しません

## 既知の制限

- SHA-256はソースの真正性を独立に保証しません。署名・build provenance・サプライチェーン監査が別途必要です
- Docker実行は非rootプロセス運用との比較・脅威モデル検討中です。通常のsystemdサービスが標準です
- 更新と同時に進行中の認証要求は失敗することがありますが、認可をバイパスしません
- Fail Closedは遠隔ログイン不能につながり得るため、サーバー所有者による復旧確認が必須です

**注意:** 上記はコマンド構文例です。古い版へ戻す場合、現在の設定形式や認証プロトコルと互換性がなければ新版側の事前検証で拒否されるべきであり、無理に旧版を起動しないでください。公開チェックサムだけでは発行者の真正性を独立に保証しません

[復旧](recovery.md) · [セキュリティ](security.md) · [English](en/updates.md)

## alpha.5以降：バージョンを知らなくても候補を選べます

```bash
sudo saspam update
sudo saspam update --list
sudo saspam update --latest
```

通常は公開Releaseを新しい順に提示し、現在の版との差（新しい・同じ・古い）を表示します。対象アーキテクチャの配布アーカイブとSHA256SUMSが存在するものだけ候補にします。**`--latest`は試験版も含めます**。インストーラーの版省略（最新の正式Release）とは異なります。v1.0.0へ固定するには`--version v1.0.0`を使ってください。旧版の選択には明示的な許可が必要です
