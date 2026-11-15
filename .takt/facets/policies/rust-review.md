# Rust Semantic Review Policy

- activeなreview instruction、output contract、routing conditionを優先する。
  独自のAPPROVE/REJECT形式を追加しない。
- command quality gateがfmt、clippy、dylint、testと決定的な構造検査を担当する。
  review stepでは意味的な妥当性に集中する。
- 確認した違反をすべて報告し、各findingを
  `[rule_name] file:line — 問題 — 最小修正`で記述する。
- 例外はcode、test、documentation、明示理由の証拠がある場合だけ認める。

## 確認対象

- 動的error消去が公開・伝播error APIを曖昧にしていないか
- `SAFETY:`説明が実際のunsafe操作と不変条件に対応しているか
- 同一fileの複数`pub struct`が本当に密接か
- 公開境界のwrapper型にdomain NewTypeが必要か
- 型中心の公開module functionがinherent methodであるべきか
- method名とownership semanticsが一致するか
- generic boundの配置がPolicyに合うか
- コメントが理由ではなくcodeの言い換えになっていないか
- コメントやrustdocに、リポジトリ内に定義サイトを持たない外部ドキュメントの
  ID・セクション番号等を書き込んでいないか
- 契約IDが定義モジュールと指名された記録テストの外で参照されていないか。
  モジュール外は自己完結の文章、または定義サイトへのリンクと由来明示になっているか
- AI tracking markerがsourceへ残っていないか
- manual Futureの例外理由が具体的か

Policyを満たすためだけのNewType、trait、file分割を修正案として要求しない。
