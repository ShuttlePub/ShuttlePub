{extends:plan}

## Report Phaseとの境界

- Workflow ContextのReport DirectoryはPhase 1では読み取り専用です。
- `plan.md`などのoutput contract対象ファイルを直接作成、変更、移動、削除しないでください。
- 親Instructionにある計画の「作成」「出力」は、Phase 1の最終回答として計画内容を返す意味です。
- レポートファイルの保存は、Workflowの`output_contracts`とTAKTのReport Phaseが担当します。

## Rust固有の計画手順

1. 対象workspaceとcrateを確定する。
   - rootおよび対象crateの`Cargo.toml`
   - 存在する場合は`rust-toolchain.toml`、`flake.nix`、`.cargo/config.toml`
   - edition、feature、target、async runtime、主要依存
2. 同種の既存実装とテストを検索し、採用するpatternを`file:line`付きで示す。
3. 要件ごとに、変更file、公開APIへの影響、ownership/lifetime、`Send`/`Sync`、
   async境界、error型と伝播経路を決める。
4. 各受け入れ条件を、作成するテストまたは実行する検証へ対応付ける。
5. NewType、trait、module分割を提案する場合は、防ぐ具体的な誤用または
   分離する責務を説明する。
6. `unsafe`が必要なら、safe Rustでは要件を満たせない理由を示す。
7. タスク外の改善を実装項目へ混ぜない。

Coderが追加設計を行わず実装できる粒度にしますが、コードを先取りして書かないでください。
