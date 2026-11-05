# 復旧手順（最重要）

PAM設定の誤りはSSHでのログイン不能につながります。SASPAMは通信障害・Discord障害・タイムアウト時に**許可しません**。そのため以下の復旧経路は導入前に確保してください。

## 導入前

- クラウドのコンソール / serial console / VMハイパーバイザーの管理画面を実際に試す
- 現在のroot権限を持つSSHセッションは維持する
- 別セッションで `saspam test` 後にゲートを有効化する
- 既存 `/etc/pam.d/sshd` のバックアップを別途保存する
- `/etc/pam.d/common-account`・`/etc/pam.d/login`はSASPAMが変更しないことを確認する

## SSHログインできなくなった場合

既存の管理セッションまたはクラウドコンソールから:

~~~bash
sudo saspam disable
sudo /usr/sbin/sshd -t
sudo saspam status
sudo journalctl -u saspamd -u saspam-discord -n 100 --no-pager
~~~

`saspam` が実行不能な場合、コンソールから `/etc/pam.d/sshd` 中の正確なSASPAM管理行のみ取り除きます。

~~~text
account required /usr/local/lib/security/pam_saspam.so # DisnanaSASPAM-managed
~~~

**他のPAM行を消さないこと。** バックアップは `/var/lib/saspam/sshd.pam.backup` にあります。ただしそのまま全面復元すると、後から他の管理者が加えた正当な設定変更を失う可能性があるため、差分を確認してください。

## 再起動時の注意

PAMが有効なままで `saspamd` やDiscord Botが起動しないと、次のSSHログインは拒否されます。サービスの原因を直すか、コンソールからSASPAMだけを解除します。**障害時に自動で認証ゲートをオフにするFail Openは実装しません。**

## アンインストール

~~~bash
sudo saspam disable
sudo saspam security rollback
sudo saspam uninstall --yes
~~~

アンインストーラーは管理対象のバイナリとsystemdユニットを除去します。設定ファイルとjournal監査ログは意図せず消さないよう保留します。資格情報の削除は内容を確認してから行ってください。

## v0.1.0-alpha.1からの互換性

新デーモンではGuild ID・Channel ID・Role IDの設定が必須です。旧User ID設定のまま更新するとデーモンは起動せず、PAMはFail Closedでログインを拒否します。**既存の管理セッション／コンソールを保持したまま保護を解除してから**、インストールと`saspam setup`を実行してください。
