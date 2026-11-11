# kws-x1-standalone-rs

[English README](README.md) · [Windowsでの2段階書き込み](docs/FLASHING.md#日本語)

KOWSI KWS-X1 USB-C 電力計向けの非公式Rustファームウェアです。このフォークは [kralonur版](https://github.com/kralonur/kws-x1-standalone-rs) をベースに、画面キャプチャの本体保存とUSB取り出しを追加しています。既存のUSBブートローダーを使ってアプリ部分を書き換えます。

## ダウンロードと書き込み

- [このフォークのReleases](https://github.com/inuchanbt/kws-x1-standalone-rs/releases) に、実機確認を終えた同じビルドの `firmware-upper.X1C`、`firmware.X1C`、`SHA256SUMS` が揃ったパッケージを掲載します。スクショのUSB取り出しには、条件付きで利用できるMSC用共用IDを使います。[USB識別子と利用条件](docs/USB-IDENTITY.md)
- Windows用の `flasher.exe` は [本家のflasherリリース](https://github.com/kralonur/kws-x1-standalone-rs/releases/tag/flasher-v0.1.2) から入手します。配布済みファイルの書き込みにRustやPythonは不要です。
- **Back＋OKを押しながら接続**して書き込みモードに入り、最初に `firmware-upper.X1C` を書き込みます。バックライトの短い2回点滅を確認したら、再度 **Back＋OKを押しながら差し直し**、`firmware.X1C` を書き込みます。ハッシュ照合、PowerShellコマンド、確認画面は [2段階書き込み手順](docs/FLASHING.md#日本語) を参照してください。

## 画面キャプチャ

1. 撮りたい画面で **左＋右を約0.5秒**押し、`CAPTURE SAVED` を待ちます。PD SESSION／PD SNIFF、CHARGER、CABLE、SYSTEMでは、**約2秒**長押しするとスクロール範囲を含む全行が縦長BMPになります。
2. USBを抜き、もう一方の端子を空けて、**左＋右だけ**を押しながら本体のUSB-Cメス端子をPCへつなぎます。Windowsのエクスプローラーで読み取り専用の `KWS X1` ドライブを開き、BMPをコピーします。
3. ボタンを押さずに差し直すと測定画面に戻ります。

ファイル名は `OVERVIEW09.BMP`、`CHARGER10.BMP` のように、メニュー名と全メニュー共通の撮影順です。メニュー情報のない旧スクショは `SCREEN` 名で表示します。日時はファイルの撮影順を示さないため、番号を使ってください。保存枚数は圧縮後のサイズと残り容量で変わります。

`CAPTURE FULL - COPY/CLEAR` が出たら、残したいBMPをPCにコピーします。消去するには **左＋右＋Back** を押しながらPCに差し直し、`ERASE ALL CAPTURES?` が出たら全ボタンを離して **OKを2秒**押します。**Back** でキャンセルできます。この操作はすべてのキャプチャを消去します。

## ビルドと互換性

このフォークのキャプチャ機能とUSB取り出しはKWS-X1実機で確認しています。ファームの全機能、他機種への互換性、給電時の注意点は [英語README](README.md) と [安全設計の説明](docs/SAFETY.md) を参照してください。

ソースからのビルドとX1Cの生成方法は [Build and validation](README.md#build-and-validation) にあります。標準のUSB識別子はMSC用の共用ID `16c0:27df` です。独自に利用権限のある番号を指定する場合は、4桁の16進数を `KWS_X1_USB_VID` と `KWS_X1_USB_PID` の両方に指定します。公開前には、その2本を実機に入れてUSB取り出しを確認します。

ライセンスは [MIT](LICENSE-MIT) または [Apache 2.0](LICENSE-APACHE) です。
