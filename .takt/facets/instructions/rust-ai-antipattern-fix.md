{extends:ai-antipattern-fix}

## Report Phaseとの境界

- Workflow ContextのReport Directoryは読み取り専用です。
- レビューレポートは参照だけに使用し、直接変更しないでください。
- 修正結果はPhase 1の最終回答として返してください。レポート保存はTAKTのReport Phaseが担当します。

## Rust固有の修正手順

一次情報:
{report:ai-antipattern-review.md}

1. 各指摘を現在のコードで再現し、対象`file:line`と実害を特定する。
2. 確認できた問題だけを最小変更で修正する。
3. allow属性、空実装、不要なfallback/clone、テスト弱体化で指摘を黙らせない。
4. false positiveまたは解消済みなら、確認したコード根拠をPhase 1の最終回答で報告し、
   見せかけの変更を作らない。
5. focused testを実行し、最後にcommand quality gateを通す。
6. レビューのfinding IDや説明をソースコメントへ転記しない。
