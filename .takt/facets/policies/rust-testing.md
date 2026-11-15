# Rust Test Coding Policy

- このstepではプロダクションコードを変更しない。
- 実装手順ではなく、観測可能な振る舞い、状態遷移、error契約、不変条件を検証する。
- テスト都合で計画にないAPIや抽象化を発明しない。
- 既存のtest module、fixture、helper、runtime、feature構成を再利用する。
- `.unwrap()`を追加しない。テスト前提には具体的な`expect()`、
  伝播可能な失敗には`Result`を使う。
- 時刻、乱数、実行順序、network、filesystemへ暗黙依存するflaky testを作らない。
- sleep時間の延長でasync testを安定化しない。
- 失敗時に破られた契約が分かるtest名とassertionを使う。
- Redは未実装要件を直接示す必要があり、fixtureやimportの不備を残さない。
- 重要な異常系では`is_err()`だけでなくerror variantまたは意味を検証する。
