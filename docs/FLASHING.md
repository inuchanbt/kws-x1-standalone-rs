# Two-stage firmware flashing / 2段階の書き込み

[English](#english) · [日本語](#日本語)

## English

Download `firmware.X1C` and `SHA256SUMS` from the **same build**. The image needs flasher 0.2.0 or newer; until it is released, build it from this repository with `cargo build --release -p flasher --locked` (`target\release\flasher.exe`). Put `flasher.exe` and both files in one folder. Use a USB data cable from the PC to the meter's USB-C receptacle, leave the meter's other connector empty and connect only one meter.

Open PowerShell in that folder. Verify the image against `SHA256SUMS`, and let the flasher check its format before writing:

```powershell
$match = @(Get-Content .\SHA256SUMS | Where-Object { $_ -match '^[0-9A-Fa-f]{64}\s+firmware\.X1C$' })
if ($match.Count -ne 1) { throw "Missing or duplicate checksum for firmware.X1C" }
$hash = $match[0].Substring(0, 64).ToLowerInvariant()
$actual = (Get-FileHash -LiteralPath .\firmware.X1C -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $hash) { throw "Checksum mismatch for firmware.X1C" }
.\flasher.exe --dry-run --image .\firmware.X1C --confirm-sha256 $hash
if ($LASTEXITCODE -ne 0) { throw "Validation failed for firmware.X1C" }
```

Unplug the meter. Hold **Back + OK** while plugging it into the PC, keep the buttons down for about one second, then release them. The display should show `Bootloader V1.1` and `--info` should report `mode=0`. Run:

```powershell
.\flasher.exe --info
.\flasher.exe --image .\firmware.X1C --confirm-sha256 $hash
```

The flasher writes the image through the bootloader (`[1/2] Factory bootloader`), then sends the remaining data to the meter's temporary installer after it restarts (`[2/2] USB installer`), without a reconnect. `Transfer complete` does not mean the update is finished: keep the meter connected until the measurement screen appears.

- If the flasher stops after stage 1, or the power is lost, the meter waits in the installer with a blank, backlit screen. Reconnect it **without** holding any button and run the same command with the same file.
- If the backlight blinks, or anything else fails, re-enter bootloader mode with **Back + OK** and flash the same file again.

## 日本語

**同じビルド**の `firmware-upper.X1C`、`firmware.X1C`、`SHA256SUMS` を用意します。[本家のflasherリリース](https://github.com/kralonur/kws-x1-standalone-rs/releases/tag/flasher-v0.1.2) から Windows x64 用の `flasher-x86_64-pc-windows-msvc.zip` とチェックサムを取得し、ZIPを照合してから展開します。`flasher.exe` とファームの3ファイルを同じフォルダーに置きます。データ通信対応のUSBケーブルをPCから本体のUSB-Cメス端子につなぎ、反対側の端子は空けておきます。

そのフォルダーでPowerShellを開き、英語セクション冒頭の [照合コマンド](#english) を実行します。`SHA256SUMS` と一致し、2本とも `--dry-run` が通ったら、同じPowerShell画面で次に進みます。

1. **1段階目。** USBを抜き、**Back（戻る）＋OKを先に押して**PCに差し込み、約1秒で離します。画面に `Bootloader V1.1` が出て、`--info` が `mode=0` を返したら `firmware-upper.X1C` を書き込みます。

   ```powershell
   .\flasher.exe --info
   .\flasher.exe --image .\firmware-upper.X1C --confirm-sha256 $hashes['firmware-upper.X1C']
   ```

   照合が終わって再起動が指示されたら、**バックライトが短く2回点滅して、その後点灯したまま**になるのを待ちます。

2. **2段階目。** USBを抜き、再び **Back＋OK** を押したままPCに差し込み、約1秒で離します。`Bootloader V1.1` と `mode=0` を確認してから `firmware.X1C` を書き込みます。

   ```powershell
   .\flasher.exe --info
   .\flasher.exe --image .\firmware.X1C --confirm-sha256 $hashes['firmware.X1C']
   ```

   照合完了後、ボタンを押さずに測定画面が出るのを待ちます。途中でエラーが出たら、書き込みモードに入り直し、照合済みの組から**同じ段階のファイル**を再実行します。1段階目の2回点滅を確認できなかった場合は、1段階目から再試行します。
