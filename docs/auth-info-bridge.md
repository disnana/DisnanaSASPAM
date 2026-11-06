# SSH認証方式の取得・安全性 / Verified auth method bridge

## 方針変更：OpenSSH本体は改造しない / No OpenSSH patching

**Ubuntu標準のOpenSSHは変更・置換しません**。ディストリビューションのセキュリティ更新と復旧手段を維持します。beta.1で別添配布した実験用パッチは研究用で、本番環境への適用は推奨しません。beta.2以降のReleaseアセットから除外します。

PAM account承認前の認証方式は、標準OpenSSHで確実に取得する方法が確認できていません。「不明（取得不可）」を表示し、設定や公開鍵の提示だけから推測しません。承認後のPAM session情報を要求IDで結び付けて表示更新する案は未実装で、承認前表示と同等ではありません。

**English:** SASPAM will not patch or replace distribution-managed sshd. Verified pre-approval method metadata is not reliably available at the stock Ubuntu PAM account stage; show Unknown. The research patch from beta.1 is not shipped with beta.2. A verified post-approval session update may be explored, but is not equivalent to verified pre-approval information.

## v1.0.0-beta.1 の研究経緯

本リリースは**検証用ベータ版**です。実験用の修正OpenSSHが `SASPAM_VERIFIED_AUTH_INFO_0` をPAMに渡す場合のみ、Discordの承認待ち画面で認証方式を確認できます。

**Ubuntu標準のOpenSSHはこの環境変数を提供しません。標準環境では引き続き「不明（取得不可）」と表示します。** 既存のSSHログイン承認機能は変わりません。

## 仕組みとI/O

- OpenSSH特権モニターが認証成功した方式だけを、接続ごとの512バイト固定長バッファに記録します
- `publickey,password`など、認証された方式の順番と重複を保持します
- 鍵内容・鍵指紋・パスワードはDiscordやSASPAM IPCへ転送しません
- 認証方式取得に追加のログ監視、ディスクI/O、ポーリング、外部ネットワーク通信は発生しません
- 取得できないときや容量超過時には「不明」とし、表示情報だけでログイン許可・拒否を行いません

## 実験用ファイル

- `experimental/openssh/V_10_6_P2-saspam-authinfo.patch` — OpenSSH 10.6p2ソース向け、独立監査前の研究用パッチ
- `.github/workflows/verified-auth-bridge-lab.yml` — 使い捨てUbuntu runnerでlocalhostの別ポートから確認するワークフロー
- `tests/integration/verified_auth_roundtrip.sh` — パスワード・公開鍵・模擬ECDSA-SK、複数方式、同一ユーザー/IP同時接続、失敗・拒否・タイムアウトの確認

**このパッチを運用中の `/usr/sbin/sshd` に適用しないでください。** 配布元のセキュリティパッチやOpenSSHの自動更新が失われる危険があります。本リリースはSSHサーバーの自動置換や本番移行機能を提供しません。

## 正式v1.0.0の残条件

1. Ubuntuディストリビューションのセキュリティパッチと更新・ロールバックを維持できるサポート手順
2. 物理FIDO（TermiusのECDSA-SK）の承認前表示と同時接続の実機検証
3. SSHモニター・PAM・Bot間の独立したセキュリティレビュー
4. CPU、メモリ、システムコール、追加I/Oとp95/p99ログイン遅延の比較計測

この条件が完了するまで安定版v1.0.0は公開しません。

## English

**Beta preview, not a supported patched OpenSSH distribution.** Verified pre-approval authentication-method display is supported only when a separately patched, isolated test SSH daemon provides `SASPAM_VERIFIED_AUTH_INFO_0` to PAM. Stock Ubuntu remains Unknown.

The experimental privileged monitor keeps a fixed 512-byte, connection-local list in RAM and supplies the list once before the existing PAM account check. There are no new log watchers, disk writes, periodic polls or external requests for method detection. Missing data does not bypass or deny SSH access on its own.

The separate research patch is pinned to OpenSSH 10.6p2 and is not installed by SASPAM. Do not replace a production Ubuntu sshd. Distro security update compatibility, supported rollback, physical FIDO/Termius testing, performance measurements and independent audit remain stable-release blockers.
