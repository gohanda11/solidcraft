# Turning CAM fork

開発先は [gohanda11/solidcraft](https://github.com/gohanda11/solidcraft) です。

| 対象 | 用途 |
| --- | --- |
| `origin` | 自分のフォーク。CAM のブランチと PR の公開先 |
| `upstream` | 本家 SolidCraft。更新の取得元 |
| `origin/main` | 本家の変更だけを取り込む比較基準 |
| `cam/turning-foundation` | 旋削 CAM の実装。実行・開発はこちら |

CAM の差分はフォーク内の PR、または `git diff origin/main...cam/turning-foundation`
で確認できます。CAM を使うときは `main` ではなく CAM ブランチをチェックアウトします。
この運用では CAM の PR を比較用に開いたままにし、所有者の指示があるまで
`main` にはマージしません。バージョン番号は本家に合わせ、リリースは別途行います。

## 本家の更新を取り込む

まず作業ツリーを確認し、未コミットの変更と未追跡のソースを保全します。
保存前の設計はアプリで保存してから、更新・ビルドを行います。

```text
git fetch upstream main
git fetch origin main
git merge-base --is-ancestor origin/main upstream/main
git push origin upstream/main:main
```

祖先確認が失敗した場合は中断して履歴を確認します。`main` を強制更新しません。
CAM の変更を push する際は、作業を保全したうえで最新の `origin/main` に rebase します。
基準版への追従だけを目的とする push は行いません。履歴の更新が必要なときは
CAM ブランチだけを `--force-with-lease` で更新し、`main` は強制更新しません。
競合の解消後は CAM、エンジン、UI のテスト・アプリ表示・全体の品質チェックを行い、
PR を更新します。

## 検証とデータ

`cargo xtask ci` が共通の品質チェックです。CAM の仕様と解析的な作例は
[turning-cam.md](turning-cam.md) と `examples/turning-*.json` にあります。
フォーク用 GitHub Actions は標準ランナーで CAM の検証を実行します。
本家の CI は本家のランナー構成に依存するため、フォークのドラフト PR では使用しません。

Windowsで全体検査を行う場合は、本家の[ローカルCI手順](book/src/dev/testing.md)に従い、
`cargo-nextest` 導入後に `SOLIDCRAFT_TEST_RUNNER=nextest` を設定します。
既存の `local-ci` 設定がGPUの初期化を直列化します。通常のlibtest実行はこの設定を使わず、
複数のGPUテストが同時にシェーダーをコンパイルするとメモリ使用量が増大するため注意します。

WindowsのGNUツールチェーンで日本語パスによるDLL生成エラーが出る場合は、
`CARGO_TARGET_DIR` を一時的にASCII文字だけのローカル作業ディレクトリへ設定します。
この場合もビルド結果はコミットせず、検証に使ったプロファイルとログを記録します。

ダウンロードした工具 STEP、利用者の設計、スクリーンショット、制御トークン、
バックアップ、ビルド結果はコミットしません。作業用データは無視対象の `target/` などに
置きます。リポジトリには工具の登録機能と出典付き寸法情報だけを含めます。

NC 出力は検討用です。実機の制御・工具補正・取付寸法の確認を完了するまでは、
現在の実行抑止付き出力を維持します。

CAM の追加ラベルは日本語・英語に対応しています。中国語カタログの CAM 追加分は
当面英語表記とし、本家の既存翻訳は維持します。
