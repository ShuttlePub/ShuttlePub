## Report Phaseとの境界

- Workflow ContextのReport DirectoryはPhase 1では読み取り専用です。
- `docs-review.md`を直接作成・変更しないでください。
- レビュー結果はPhase 1の最終回答として返してください。保存はTAKTのReport Phaseが担当します。

## 計画
{report:plan.md}

## レビュー目的

このレビューはdocs-onlyタスクを**収束させるための限定レビュー**です。
判定は `approved` / `needs_fix` の二値です。

指摘してよいのは次の4観点だけです。各findingには必ず観点ID（D1〜D4）と、
対応するタスクチェックリスト項目を付けてください。

### D1 — Checklist completeness
タスク指示書のチェックリスト項目が、計画された記載先に存在しない、または必要内容が欠落している。

### D2 — Source-of-truth consistency
記載内容が正本となるテスト・実装・ADR・公開契約と矛盾している。
正本を実ファイルで確認せず、推測でD2を作らないでください。

### D3 — Mechanical documentation validity
doc build、rustdoc warning、intra-doc link切れ、またはquality gate相当の機械的問題がある。

### D4 — Docs-only scope violation
今回の差分に、docs以外のシグネチャ・実装・テスト・設定等の挙動変更が混入している。

## 指摘禁止

次をfindingまたは`needs_fix`の根拠にしてはいけません。

- 文体、語調、言い回し、表現の好み
- 一般論としての章立て・構成改善
- タスクチェックリスト外の追記提案
- スコープ外のリファクタリング・設計改善・追加説明
- 「より良くできる」「より親切にできる」だけの提案

これらしか指摘がない場合は `approved` にしてください。
禁止観点を`needs_fix`の根拠にしたレビューは不成立です。

## 判定手順

1. タスク指示書のチェックリストを全件列挙する。
2. `plan.md`の対応表と現在のdiffを照合する。
3. 各項目についてD1〜D4だけを確認する。
4. findingがある場合は、必ず以下を示す。
   - 観点ID
   - チェックリスト項目
   - `file:line`等の事実根拠
   - 正本（D2の場合）
   - 最小修正
5. D1〜D4の未解決findingが1件でもあれば `needs_fix`。
6. 全項目がD1〜D4を通過したら `approved`。
   「もっと改善できる」はapprovedを妨げません。
