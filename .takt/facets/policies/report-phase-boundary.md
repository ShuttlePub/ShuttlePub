# Report Phase境界Policy

Phase 1とTAKTのReport Phaseの責務を混同しない。

## Phase 1

- Instructionで指定された本来の作業、調査、実装、修正、レビューを行う。
- Workflow ContextのReport Directoryは読み取り専用として扱う。
- Report Directory内のファイルをWrite、Edit、Bash、その他のツールで直接作成、変更、移動、削除しない。
- output contractに必要な情報はPhase 1の最終回答へ含める。

## Report Phase

- レポートファイル名と形式はWorkflowの`output_contracts`が定義する。
- レポート本文の形式はoutput contract facetが定義する。
- 実際のレポートファイル保存はTAKTのReport Phaseへ委ねる。

## 親Instructionの解釈

継承した親Instructionに「レポートを作成する」「Scopeへ記録する」
「結果を出力する」などの表現がある場合、明示的な別成果物への書込み指示でない限り、
output contract対象ファイルをPhase 1で直接操作する意味には解釈しない。
Phase 1の最終回答へ必要情報を含める意味として扱う。

## 禁止

- output contract対象ファイルの先行作成
- 既存レポートの上書き
- タイムスタンプ付き退避ファイルの手動作成
- Report Directory内でのrename、copy、remove
- レポート保存を目的としたshell redirection
