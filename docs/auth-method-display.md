# SSH認証方式表示（実験的オプション） / Experimental SSH method display

## 標準動作

v1.0.0では**認証方式欄をデフォルトで非表示**にしています。新規・既存インストールとも設定が未指定ならOFFです。Discord承認のロール認可、期限切れ、拒否、PAMのFail Closedの挙動は変わりません。標準のUbuntu OpenSSH、sshd_config、ファイアウォールにパッチは不要です

## ONにする場合

**独立したコンソールなどの復旧手段を必ず確認してください。** 改造OpenSSHの使用は脆弱性修正の欠落、Ubuntuのパッケージ更新との競合、SSH切断・ログイン不能、同時ログイン時の識別不具合につながり得ます。自己責任の研究用であり、本番SSHサーバーへの適用は推奨しません

```bash
sudo saspam config show
sudo saspam config set auth-methods experimental --ack-ssh-risk --ack-recovery
sudo saspam config set auth-methods off
```

PAMゲートが有効なホストでは上記変更に加えて `--ack-interruption` が必要です。設定画面でも二段階の確認を求めます。設定変更時、承認待ちリクエストがあれば変更を拒否し、設定適用失敗時は以前の設定へロールバックします

**ここで有効になるのは表示だけ**です。標準のsshdが `SASPAM_VERIFIED_AUTH_INFO_0` を出すようになるわけではありません。OpenSSHのビルド・インストール、systemdサービス差し替え、パッチ適用は一切行いません。標準Ubuntuでは依然として「不明（取得不可）」です

## 研究用OpenSSHブリッジ

[PR #14](https://github.com/disnana/DisnanaSASPAM/pull/14)に、10.6p2を対象にした実験パッチとlocalhost限定の統合テストがあります。**v1.0.0のGitHub ReleaseにはOpenSSHパッチや独自sshdバイナリを同梱せず、自動適用もしません**。改造SSHはディストリビューションからの安全な更新を保証できず、物理FIDO・Termiusでの検証や独立監査も未完了です

## OFFへ戻す

```bash
sudo saspam config set auth-methods off
sudo saspam config show
```

これはSASPAMの表示を元に戻すだけで、手動導入した改造OpenSSHを復元しません。もしSSHサーバーを別途改造していた場合は、既存のSSH管理セッションか独立コンソールを維持し、ディストリビューションの正規パッケージへ戻すための事前に確認した復旧手順に従ってください

## English

Default: authentication-method field hidden on all pending/completed Discord cards. `saspam config set auth-methods experimental --ack-ssh-risk --ack-recovery` opts into **display only**, with `--ack-interruption` required when the PAM gate is active. This does not build, patch, or replace Ubuntu's sshd. Unpatched Ubuntu may still show Unknown in experimental mode. Experimental custom SSH entails security-update and lockout risks and is **not** production-supported. Disable the field with `saspam config set auth-methods off`. The research patch is retained separately in [PR #14](https://github.com/disnana/DisnanaSASPAM/pull/14) but not published in the stable Release.
