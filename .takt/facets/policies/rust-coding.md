# Rust Production Coding Policy

## 適用範囲

- 実装・修正stepが作成または変更するプロダクションRustコードへ適用する。
- タスク、計画、既存アーキテクチャ、公開契約を優先し、
  要求されていない全体リファクタリングを行わない。
- repository全体の決定的な検査はcommand quality gateを正とする。
- 無関係な既存違反をscope拡大の理由にしない。ただしgate失敗は無視しない。

## エラー処理

- `.unwrap()`を導入しない。回復可能な失敗は`Result`と`?`で伝播する。
- 不変条件に依存する場合だけ、成立理由が分かる`expect()`を使う。
- プロダクションAPIへ`Box<dyn Error>`相当のerror消去を導入しない。
- 既存のdomain errorを優先し、必要な場合は明示的なerror型を定義する。
- errorを文字列化して型情報や原因連鎖を失わせない。

## unsafe

- safe Rustでは要件を満たせない場合だけ新しい`unsafe`を使う。
- unsafe block直前の`// SAFETY:`で、不変条件と成立根拠を説明する。
- `unsafe fn`、`unsafe trait`、`unsafe impl`は利用側・実装側の義務を文書化する。

## moduleと型

- 新しい`mod.rs`を作らない。既存`mod.rs`を無関係に移動しない。
- 1ファイルに`pub struct`を3個以上置かない。
- 2個の場合はbuilder/productなど同居理由がコードから明確であること。
- 行数だけで分割せず、責務・変更理由・公開境界で分ける。
- 小さなfileやre-exportをPolicyのためだけに大量生成しない。

## 公開API

- 公開境界で同期・配送・保存などの実装詳細を直接漏らさない。
- `Arc<Mutex<_>>`、`Arc<RwLock<_>>`、`Sender<_>`、`Receiver<_>`、
  `Proxy<_>`、用途固定の複合collectionは、domain上の意味、不変条件、
  許可操作を持つ場合にNewTypeで包む。
- private fieldに汎用型が存在するだけではNewTypeを強制しない。
- 名前だけのNewTypeを作らず、誤用を防ぐAPIまたは不変条件を提供する。
- 型中心のconstructor、変換、操作はinherent `impl`を優先する。
- 既存の意味ある契約がある場合だけtraitを使い、
  module function回避のための単一用途traitを作らない。

## 命名、async、generic

- `as_*`は借用、`to_*`は借用から新値生成、`into_*`は`self`消費、
  `*_mut`は`&mut self`、`is_*`は観察的判定とする。
- 通常関数でasync blockを返すだけのmanual Futureは`async fn`にする。
- object safety、`Send`境界、公開API互換、外部trait契約がある場合は例外を許容し、
  理由を近傍に記録する。
- trait boundが1個ならinline、2個以上なら`where`句を使う。
- 将来用途だけのgenericや抽象化を追加しない。

## コメントと完了

- コメントは理由、不変条件、外部契約、非自明な制約を説明する。
- コードの言い換え、banner、区切り線、AIレビューID、修正roundを残さない。
- リポジトリ内に定義サイトを持たない外部ドキュメントのID・セクション番号等を書き込まない。
  コメントやドキュメントは自己完結させる。
- 契約ID等のラベルを使う場合は、見出し・定義・対応する実行可能テスト名を明記した
  定義サイトをリポジトリ内の1モジュールに置く。
- IDの参照は定義モジュール内と、定義が実行可能な記録として指名したテストファイル内に限る。
  モジュール外でID記法を使わない。分散参照は定義変更時の追随漏れでドキュメントを腐敗させる。
- モジュール外で定義内容が必要な場合は自己完結の文章で書く。記述が大幅に膨らむ場合は
  intra-doc linkやSee alsoで定義サイトを参照し、その内容が定義サイト由来であることを明示する。
- テストを通すために期待値や検証を弱めない。
- command quality gate未通過のまま完了を宣言しない。
