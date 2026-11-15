{extends:write-tests-first}

## Report Phaseとの境界

- Workflow ContextのReport DirectoryはPhase 1では読み取り専用です。
- `test-report.md`をWrite、Edit、Bash、その他のツールで直接作成、変更、移動、削除しないでください。
- 親Instructionにあるテストレポートの「作成」「出力」は、Phase 1の最終回答へテスト結果と実装stepへの申し送りを含める意味です。
- `test-report.md`の保存は、Workflowの`output_contracts`とTAKTのReport Phaseが担当します。

## Rust固有のテスト作成手順

1. 計画レポート、対象crateのmanifest、既存テストを確認する。
2. 受け入れ条件ごとに最も狭い層へテストを置く。
   - 純粋な変換・不変条件: unit test
   - 公開crate APIや複数moduleの配線: integration test
   - 公開利用契約: doc test
   - featureや型境界: 必要な場合だけcompile test
3. 正常系、error、境界値、状態遷移、重複、順序の影響を検証する。
4. async testでは固定sleepの延長を使わず、既存の決定的な同期方法を使う。
5. focused testを実行し、Redの原因が未実装要件であることを確認する。
   import、fixture、feature指定、テストlogicの誤りはこのstepで解消する。
6. 未実装APIによりコンパイル不能なら、計画上のsignatureと解消条件をPhase 1の最終回答で報告する。

プロダクションコードを変更してGreenにしてはいけません。
