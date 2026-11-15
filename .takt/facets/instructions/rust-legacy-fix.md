{extends:fix}

## Report Phaseとの境界

- Workflow ContextのReport Directoryは読み取り専用です。
- 以下のレビューレポートは参照だけに使用し、直接変更しないでください。
- 修正結果はPhase 1の最終回答として返してください。レポート保存はTAKTのReport Phaseが担当します。

## Rust単一Workflow向け修正手順

### Architecture Review
{report:architect-review.md}

### Final AI Antipattern Review
{report:ai-antipattern-review-final.md}

### Rust Coding Review
{report:coding-review.md}

### Supervisor Validation
{report:supervisor-validation.md}

1. 各指摘を現在のコードで再確認し、根拠が確認できた問題だけを修正する。
2. 複数レポートの指摘が同じ原因なら、原因を一度修正して全指摘を再検証する。
3. 公開API、error型、module境界、依存方向を変更する場合は、呼び出し元とテストへの影響を追う。
4. レビュアーを満たすためだけの抽象化、単一用途trait、名前だけのNewType、
   不要なfallback、allow属性、テスト弱体化を行わない。
5. focused test、関連crate/workspace test、command quality gateの順に検証する。
6. false positiveまたは解消済みの指摘は、現在のコードとコマンド結果を根拠としてPhase 1の最終回答で報告する。
7. レビューID、修正round、内部推論をソースコメントへ残さない。
