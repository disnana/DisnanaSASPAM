# 更新・ダウングレード・自動復旧（Alpha）

**日本語** | [English](en/updates.md) · [ドキュメント一覧](README.md) · [README](../README.md)

DisnanaSASPAMの更新処理は **SSHのPAM保護を解除しない** ことを前提にしています。デーモンが停止している間に届いたSSH承認要求は、Fail Closedで拒否されます。

## 前提条件

- Ubuntu 22.04 / 24.04（amd64 / arm64）およびsystemd
- 復旧用の既存SSHセッションと、クラウドのコンソール・シリアルコンソール
- `sudo saspam doctor` と `sudo saspam test` で正常性を確認できること
- Alpha版は開発段階。正式運用前の実機確認とセキュリティ監査が必要

## コマンド

```bash
sudo saspam update --version v0.1.0-alpha.4 --dry-run
sudo saspam update --version v0.1.0-alpha.4 --allow-reinstall
sudo saspam update --version v0.1.0-alpha.3 --allow-downgrade
sudo saspam update --rollback
```

現在のバージョンと同じ版を再配置する場合は `--allow-reinstall` が必要です。`--skip-approval-test` はSSHのPAM保護が **無効なときに限って** 利用できます

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

## 初めてAlpha.3から更新する場合

Alpha.3には安全な更新・復旧コマンドがありません。最初の移行時だけ復旧用コンソールを確保し、`saspam disable` 後にAlpha.4のインストーラーでアップグレードします。再度 `saspam setup`、`saspam doctor`、`saspam test` を通過させてから保護を有効化してください。以降のバージョン更新ではPAMゲートを解除しません

## SSH新規接続の一時遮断について

`--block-new-ssh` は現状 **未提供**。指定するとエラー終了します。sshポート・nftables・IPv4/IPv6・Tailscale/NetBirdの条件で誤って管理者を締め出すリスクがあるため、タイマーによる自動解除と実接続の回帰テストが完成するまで実装を有効化しません

## 既知の制限

- SHA-256はソースの真正性を独立に保証しません。署名・build provenance・サプライチェーン監査が別途必要です
- Docker実行は非rootプロセス運用との比較・脅威モデル検討中です。通常のsystemdサービスが標準です
- 更新と同時に進行中の認証要求は失敗することがありますが、認可をバイパスしません
- Fail Closedは遠隔ログイン不能につながり得るため、サーバー所有者による復旧確認が必須です

**注意:** 上記はコマンド構文例です。古い版へ戻す場合、現在の設定形式や認証プロトコルと互換性がなければ新版側の事前検証で拒否されるべきであり、無理に旧版を起動しないでください。公開チェックサムだけでは発行者の真正性を独立に保証しません

[復旧](recovery.md) · [セキュリティ](security.md) · [English](en/updates.md)
