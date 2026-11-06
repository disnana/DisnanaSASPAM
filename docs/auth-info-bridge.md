# 認証方式表示の技術的制約 / Authentication-method display limitations

## v1.0.0の標準動作

OpenSSH本体は改造しない。**Discord承認Embedの認証方式欄は標準で非表示**です。情報が得られないのに「不明」を表示する欄もありません。PAM account段階の承認可否は以前と同じで、Discordボタン認可が必須です

## 実験オプション（明示的な危険性の承認が必要）

認証方式を表示したい場合のみ `sudo saspam config set auth-methods experimental --ack-ssh-risk --ack-recovery` を使用できます。SSHゲート有効中は `--ack-interruption` も必要です。`sudo saspam config set auth-methods off` で非表示に戻せます。**設定変更はOpenSSHのパッチ適用や置換を実行しません**

OpenSSHの研究用コードは[PR #14](https://github.com/disnana/DisnanaSASPAM/pull/14)に保存されています。技術的なリスクは[実験モードの説明](auth-method-display.md)を参照してください。通常のUbuntu OpenSSHでは `SSH_AUTH_INFO_0` がPAM accountで空になる実測があり、実験モードでも「不明」と表示されます。FIDO鍵の方式を推測してはなりません

2026年11月のUbuntu 22.04/24.04使い捨て環境で、研究用改造OpenSSH 10.6p2を使ったパスワード・公開鍵・模擬ECDSA-SK、複数方式、同IP並行接続を検証しました。ただし配布元のSSH更新互換性、Termius実機ハードウェアFIDO、独立した安全性評価は未確認です

## I/O・性能

標準モードでは追加の監視デーモンやログ読み取りを起動しません。研究用実装は各認証接続に固定長のメモリ領域を保持してPAMへ一度渡す設計ですが、CPUや遅延の実測は未完了です。ログ監視（`ssh_auth_events.py`）は任意の別プロセスで、認証後の監査通知にのみ利用できます

## English

Stable v1.0.0 hides authentication-method fields by default while preserving the fail-closed PAM/Discord approval gate. Experimental display requires explicit CLI risk/recovery acknowledgement and does **not** install or patch OpenSSH. Stock Ubuntu still cannot reliably expose verified methods at the PAM account boundary. The research-only patched OpenSSH 10.6p2 proof of concept is not production-supported and does not automatically receive distro security updates. See [experimental mode](auth-method-display.md).
