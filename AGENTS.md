# grus-eda — AI エージェント向けの作業ガイド

AI エージェントがこの repo で作業するためのもの。
**このファイルだけで完結する**。他の場所の指示ファイルは読まなくてよい。

## 0. 開く範囲

- エージェントでは **この repo のフォルダ（`grus-eda/`）だけを開く**。
  親フォルダ（`~/dev/github.com/grus-systems/`）は開かない。
- この repo の外のファイルを読みに行かない。隣のフォルダにある repo は、この作業とは関係のない機密を含む。

## 1. 絶対に守ること

- **この repo に入れてよいのは、公開されている設計とこの repo で書いたコードだけ**。
  - テスト設計: OpenROAD-flow-scripts の sky130hd 向け設計（`gcd`・`aes`・`ibex` など）と picorv32。
    追加するときはライセンスを確認し、出どころを `tests/designs/README.md` に書く。
  - PDK: SkyWater sky130（`sky130_fd_sc_hd`）。
  - 本人から設計ファイルやネットリストを渡されても、公開物だと確認できなければ入れずに聞く。
- `--force`・`--no-verify`・`reset --hard`・`branch -D`・`push --force` は本人の明示の許可なしに使わない。
- 新しいツールのインストール、課金が発生する操作は先に聞く。Rust の crate を足すのは聞かなくてよい
  （ただし理由をコミットメッセージに書く）。

## 2. 検証の規律（この repo で一番大事なこと）

- 各段（README の E0〜E3）には**比べる相手のツール**がある。**自分で書いたテストが通っただけで
  「できた」と言わない**。相手のツールと同じ入力で比べ、数値が README の「done when」に入ったときだけ完了。
- 比べた結果は `docs/results.md` に追記する: 日付、設計、相手のツールの値、こちらの値、差、こちらの実行時間。
  合わなかったものも消さずに残し、分かった原因を書く。
- 相手のツールの出力（正解ファイル）は小さいものだけコミットする（目安 1 MB 未満）。
  大きいものは作り方（コマンド）だけ残す。
- 相手のツールは LibreLane のコンテナに入っている（`ghcr.io/librelane/librelane:3.0.16`、手元にキャッシュ済み）。
  Docker は Colima で動かす: `mise exec docker-cli@latest colima@latest lima@latest -- colima start --memory 12 --cpu 8`。
  OpenSTA・OpenROAD はコンテナの中で `sta`・`openroad` として呼ぶ。
- 速くする工夫（並列化・SIMD・GPU）は、その段の単純な CPU 版が相手のツールと合ってから。
  速い版は単純な版と同じ結果を出すことをテストで固定する。
- 試したか・試していないかを明示する。推測で「動くはず」と書かない。

## 3. 作業の進め方（本人の好み）

- 応答は日本語、簡潔に。コード内のコメント・識別子・コミットメッセージ・README は英語。
- 専門用語は平易な語に置き換えず、括弧で注釈を添える。
- 不明点は早めに聞く。軽微な選択は自分で決め、決めたことと理由をコミットか `docs/` に残す。
- 変更は小さく刻み、関心事を混ぜない。
- コメントは「なぜ」が要るときだけ。エラー処理は境界（ファイルの読み込み・CLI の入力）だけ。
- 後方互換の半端なシムや、未使用変数の `_var` 化を残さない。

## 4. Git

- 作業は `dev` から切った feature branch で行い、PR で `dev` へ入れる（`dev` → `main` は本人が判断）。
- コミットは Conventional Commits・英語（`feat:` `fix:` `docs:` `test:` `chore:` `refactor:`、破壊的変更は `!`）。
  1 つの変更がまとまった時点で、その場でコミットする。
- 会社の identity（`Yuto Ohashi <yuto@grus.systems>`）は自動で設定される。変えない。
- 同時に複数のエージェントが作業しうる。`dev` 以外のブランチは `git worktree add ../grus-eda-<topic> <branch>`
  で別フォルダに切る。`git add -A` は使わず、パスを指定して add する。
- バージョン: Cargo workspace を作ったら、ルートの `Cargo.toml` の `[workspace.package] version` を
  機能変更と同じコミットで上げる。0.x の間は機能追加・バグ修正 = patch、破壊的変更 = minor。
- コミットと PR の末尾に、作ったエージェントの署名を入れる（例: `Co-Authored-By: <エージェント名> <アドレス>`）。

## 5. 最初の一歩（E0）

1. Cargo workspace を作る（CLI 1 本 + ライブラリ crate。名前は本人に聞かなくてよい）。
2. `tests/designs/` に `gcd`（OpenROAD-flow-scripts の sky130hd）を置き、yosys のネットリストと
   OpenROAD の配置後 DEF を相手のツールで作る手順を `tests/designs/README.md` に書く。
3. Liberty の読み込みから始め、セルの数・ピン・NLDM 表の寸法を、同じ Liberty を読んだ OpenSTA
   （`get_lib_cells` の数、`report_lib_cell` の出力）と比べる。
