# Rust Planning Constraints

- 既存workspace、crate境界、feature、公開API、error型、async runtime、
  テスト構成を根拠に計画する。
- 要求されていないcrate分割、依存追加、公開API変更、一般化、将来拡張を混ぜない。
- 新しい型、trait、NewType、moduleは、具体的な責務、不変条件、公開境界、
  または既存patternで必要性を説明できる場合だけ提案する。
- constructorや型中心の操作はinherent `impl`を基本とし、
  モジュール関数を避けるためだけのtraitを作らない。
- `Box<dyn Error>`、新しい`unwrap()`、根拠のない`unsafe`、
  不要なmanual Futureを前提にしない。
- タスク外の既存違反一掃を計画しない。quality gateを阻害する既存問題は分離して示す。
- 設計判断には既存実装の`file:line`、明示要件、外部契約のいずれかを根拠として示す。
