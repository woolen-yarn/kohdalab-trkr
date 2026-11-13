# Redistributing KohdaLab TRKR

Distribute the complete portable archive or installer with its `support/`
licenses, corresponding source archive, notices, and checksums. Do not ship
only the GUI executable. The repository MIT license applies to original
KohdaLab Rust code and the thin Python notebook bridge; dependencies and native
helpers retain their own licenses.

| Component | License boundary | Required material |
| --- | --- | --- |
| Rust GUI, engine, demo launcher, notebook bridge | MIT plus dependency licenses | Source and dependency notices included by the release packager |
| VISA helper | MIT | `native/visa_helper.cpp` and its notice |
| GPIB helper and its vendored components | GPL and upstream terms | Matching source, GPL text, and notices supplied with the packaged helper |

The Rust application invokes the GPIB helper as a separate process over its
documented standard-input/output protocol. This records an engineering boundary;
it is not a legal conclusion about GPL obligations. If the helper changes,
distribute the exact corresponding source and preserve its notices.

No vendor firmware, vendor VISA runtime, or manufacturer SDK is redistributed
by this project. Operators obtain their chosen VISA runtime and any adapter
firmware through sources that grant them permission. Do not imply vendor
endorsement.

The GPIB helper is excluded from the first-party SignPath allowlist in
[CODE_SIGNING_POLICY.md](CODE_SIGNING_POLICY.md). Redistribution permission and
eligibility for Foundation signing are separate questions.

## 日本語

portableアーカイブまたはインストーラーは、`support/`のライセンス、対応ソース、権利表記、チェックサムを含む完全な形で配布してください。GUI実行ファイルだけを配布しないでください。MITライセンスはKohdaLab独自のRustコードと薄いPython Notebook連携部に適用されます。依存物とネイティブhelperには個別のライセンスが適用されます。

| 構成要素 | ライセンスの境界 | 必要な資料 |
| --- | --- | --- |
| Rust GUI・エンジン・Demo起動部・Notebook連携 | MITと依存物のライセンス | 配布処理が同梱するソースと依存物の権利表記 |
| VISA helper | MIT | `native/visa_helper.cpp`と権利表記 |
| GPIB helperと同梱上流部品 | GPLと上流の条件 | 配布helperと一致するソース、GPL本文、権利表記 |

Rustアプリは、文書化された標準入出力プロトコルを通じてGPIB helperを別プロセスとして実行します。これは技術的な境界の記録であり、GPLの法的義務についての結論ではありません。helperを変更した場合は、そのバイナリに正確に対応するソースと権利表記を配布してください。

本プロジェクトはベンダーのファームウェア、VISAランタイム、メーカーSDKを再配布しません。操作者は、利用許諾のある入手元から必要なVISAランタイムとアダプターファームウェアを取得してください。ベンダーの推薦・承認を示唆しないでください。

GPIB helperは[コード署名ポリシー](CODE_SIGNING_POLICY.md)の第一者SignPath許可リストから除外しています。再配布の許諾とFoundation署名の対象資格は別の問題です。
