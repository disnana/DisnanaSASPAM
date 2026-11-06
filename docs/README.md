# DisnanaSASPAM ドキュメント / Documentation

[**日本語**](../README.md) | [**English**](../README.en.md)

**Current developer preview:** `v0.1.0-alpha.4`. This is not a production-security certification / 現在は未監査のAlpha版です

| 項目 / Topic | 日本語 | English |
|---|---|---|
| 使い方 / Overview | [README.md](../README.md) | [README.en.md](../README.en.md) |
| インストール・設定 / Installation | [日本語](installation.md) | [English](en/installation.md) |
| 構成 / Architecture | [日本語](architecture.md) | [English](en/architecture.md) |
| 復旧 / Recovery | [日本語](recovery.md) | [English](en/recovery.md) |
| セキュリティ / Security | [日本語](security.md) | [English](en/security.md) |
| 更新・復帰 / Updates & rollback | [日本語](updates.md) | [English](en/updates.md) |

物理コンソール・`sudo`・`su`はSASPAMのPAM保護対象外です。ただしサーバーの実際のコンソール復旧手段を確認してからSSH保護を有効化してください

Physical-console, `sudo` and `su` PAM stacks are outside SASPAM's SSH-only gate. Verify real out-of-band recovery before enabling SSH protection.
