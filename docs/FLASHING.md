# Two-stage firmware flashing / 2段階の書き込み

[English](#english) · [日本語](#日本語)

## English

Download `firmware-upper.X1C`, `firmware.X1C` and `SHA256SUMS` from the **same build**. Download the Windows x64 `flasher-x86_64-pc-windows-msvc.zip` and its checksum from the [upstream flasher release](https://github.com/kralonur/kws-x1-standalone-rs/releases/tag/flasher-v0.1.2). Verify the archive checksum, then put the extracted `flasher.exe` and the three firmware files in one folder. Use a USB data cable from the PC to the meter's USB-C receptacle and leave the meter's other connector empty.

Open PowerShell in that folder. Verify the two files against `SHA256SUMS`, and let the flasher check their format before writing:

```powershell
$hashes = @{}
foreach ($name in 'firmware-upper.X1C', 'firmware.X1C') {
    $match = @(Get-Content .\SHA256SUMS | Where-Object {
        $_ -match ('^[0-9A-Fa-f]{64}\s+' + [regex]::Escape($name) + '$')
    })
    if ($match.Count -ne 1) { throw "Missing or duplicate checksum for $name" }
    $expected = $match[0].Substring(0, 64).ToLowerInvariant()
    $actual = (Get-FileHash -LiteralPath $name -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "Checksum mismatch for $name" }
    $hashes[$name] = $expected
    .\flasher.exe --dry-run --image $name --confirm-sha256 $expected
    if ($LASTEXITCODE -ne 0) { throw "Validation failed for $name" }
}
```

1. **Stage 1.** Unplug the meter. Hold **Back + OK** while plugging it into the PC, keep the buttons down for about one second, then release them. The display should show `Bootloader V1.1` and `--info` should report `mode=0`. Run the following in the same PowerShell window:

   ```powershell
   .\flasher.exe --info
   .\flasher.exe --image .\firmware-upper.X1C --confirm-sha256 $hashes['firmware-upper.X1C']
   ```

   Wait for bootloader acceptance, then for **two short backlight flashes followed by steady illumination**. The meter is waiting for stage 2.

2. **Stage 2.** Unplug, then reconnect with **Back + OK** held as before. Check `Bootloader V1.1` and `mode=0` again. Run:

   ```powershell
   .\flasher.exe --info
   .\flasher.exe --image .\firmware.X1C --confirm-sha256 $hashes['firmware.X1C']
   ```

   Wait for bootloader acceptance and the measurement screen after restart. If a command fails, stop at that stage, re-enter bootloader mode, and retry the **same** file from the verified pair. If you did not see the two flashes after stage 1, repeat stage 1 before moving on.

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
