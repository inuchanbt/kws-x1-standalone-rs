# ファームウェアのブランチ

[English（正本）](BRANCHES.md)

| ブランチ | 用途 | ビルド |
| --- | --- | --- |
| `main` | 画面キャプチャ版の基準と既存Release | 通常ビルド |
| `charging-logger` | 電圧・電流、パッシブPD記録、CSV出力とAVS検証 | `--features charging-logger` |

今後の独立した派生版も、用途の分かるブランチへ分けます。新しい派生版を作った際に、
ビルド方法・保存領域・検証結果・Release状況を追記します。共通修正をmainへ戻す場合も、
個別に確認します。実験版全体のmainへの統合は別の判断です。

mainの既存履歴（選択式のlogger v0.2を含む）や公開済みキャプチャ版Releaseは保持します。
loggerブランチでも通常ビルドはキャプチャ版のため、loggerにはfeature指定が必要です。
ブランチを切り替えても実機のFWは変わりません。実機には同じビルドのupper/mainを使ってください。

[公開検証データ](../tests/data/logger-avs-20261113)を掲載しています。ソースやデータの公開と、
FWのRelease配布は別です。旧データ消去用のハッシュは自分のバックアップから生成してください。

同じcharging-loggerブランチに、`--features logger-observer`で作る観測専用の
単一ファイル候補を追加しました。能動制御は含みません。[詳細](LOGGER-OBSERVER.ja.md)。
