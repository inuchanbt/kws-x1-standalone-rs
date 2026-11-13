# Shared USB identity / 共用USB識別子

Screenshot and charging-logger export use Objective Development's shared **Mass Storage**
VID/PID **`16c0:27df`** under the terms in
[`USB-IDs-for-free.txt`](https://github.com/obdev/v-usb/blob/master/usbdrv/USB-IDs-for-free.txt).
The table lists `27df` for Mass Storage and permits implementations using
technology other than V-USB.

| Descriptor | Value |
|---|---|
| VID / PID | `16c0:27df` |
| Manufacturer, English/US (`0x0409`) | `usb.pd.epr.240w@gmail.com` |
| Product | `KWS-X1 Screenshot` (default) or `KWS-X1 Logger` (`charging-logger` feature) |
| Interface class / subclass / protocol | Mass Storage / SCSI / Bulk-Only (`08/06/50`) |
| Host driver | Operating system's standard USB storage driver |

These IDs are shared. The current export mode uses only the operating system's
class driver and has no custom host application for selecting a device. The
serial-number rules' item (7) permits omitting the serial number in this case.
The manufacturer and product strings are also provided to help identify the
device in system tools.

Any future custom discovery tool must follow the serial-number rules from
the linked terms, including a serial prefix based on a controlled domain or
email address. Keep control of an identifier used for that purpose for as long
as the devices are in use.

The firmware updater communicates with the original HID bootloader at
`1105:000a`. That is a separate USB identity.

Screenshot export defaults to the shared ID. A build can override it with
both four-digit hexadecimal `KWS_X1_USB_VID` and `KWS_X1_USB_PID`; the builder
is responsible for permission to use that pair and for matching its USB class.
The former `1209:0001` builds are for private testing.

## 日本語

スクショのUSB取り出しには、Objective Developmentが公開しているMSC用の共用ID
**`16c0:27df`**を使います。Windows標準のストレージドライバーで
`KWS X1` ドライブを開く方式です。製造者文字列は
`usb.pd.epr.240w@gmail.com`、製品名は `KWS-X1 Screenshot` です。

共用IDのうち「シリアル番号による識別」の条件(7)では、OS標準のクラスドライバー
だけで使い、独自アプリによる機器選択を行わない場合、シリアル番号を省略できます。
このファームはその条件でMSCとして動作します。今後、機器を検索する独自アプリを
追加する場合は、管理下のメールアドレス等で始まるシリアル文字列と照合処理を含め、
同じ利用条件に沿って実装する必要があります。

この番号は共用で、個別のPID申請をせずに上記条件で利用できます。ファーム書き込み用の
元のブートローダーは `1105:000a` で動作します。

Logger CSV export uses the same read-only MSC transport and shared-ID policy; it does not add a HID interface.

充電logger版の製品文字列は `KWS-X1 Logger` です。CSV出力も同じ読み取り専用MSC方式と共有ID方針を使用し、HIDインターフェースは追加しません。
