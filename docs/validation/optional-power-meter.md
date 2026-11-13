# Optional power meter / 任意パワーメーター

Status: validated against a source build based on stable 0.5.2 (`c25f82f`) and released in 0.5.3. / 状態：安定版0.5.2（`c25f82f`）を基点としたソースで検証し、0.5.3で公開。

- Rust workspace: 429 tests passed (mock transports; no real instrument). / Rust全429テスト成功（模擬通信、実機なし）。
- Python: 68 packaging/notebook/client tests and 14 common-control tests passed. / Python配布・Notebook・連携68件と共通制御14件が成功。
- Strict Clippy, rustfmt and local documentation links passed. / 厳格Clippy・rustfmt・文書リンクを確認。
- Mac GUI: explicit power-meter simulation captured across all five workflows; compact-layout validator passed. Requested window 1143 × 800; application minimum sizing produced a 1566 × 800 logical-pixel window at 2×. / Mac GUIで全5測定の明示的な模擬表示を撮影し、配置検証に成功。要求サイズ1143 × 800に対し、アプリの最小幅により1566 × 800論理px・2倍で表示。

Checked cases include W/dBm conversion, a single sample-plane multiplier, preservation of negative noise, exact native replies, finite-value/range/wavelength/unit/sensor validation, independent live polling, fresh point reads, in-flight cancellation without losing the USB session, optional/missing meter behavior, every workflow's CSV precision, legacy device IDs and per-workflow opt-in. Config acknowledgements and stale-session status are isolated from background live events.

W/dBm換算・試料補正の一度だけの適用・負のノイズ値保持・生応答保持・有限値／範囲／波長／単位／センサー検証・独立ライブ更新・各点の新規読取・接続を維持する読取中断・未使用／未接続時の測定・全測定CSVの精度・旧機器ID・測定別の任意選択を確認しました。設定完了通知と旧接続の状態をバックグラウンドライブイベントから分離しています。

Physical PM100USB/PM100D access, sensor calibration, the Windows driver installation and installer lifecycle have not been qualified by these tests. No release or physical qualification is implied. / PM100USB/PM100D実機・センサー校正・Windowsドライバー導入・インストーラーの実行過程は未検証です。これらのテストは公開済みまたは実機検証済みであることを示しません。

Captured source SHA-256 / 撮影時ソースSHA-256:

```json
{
  "crates/trkr-desktop/src/ui/power_meter.rs": "99cb4e619f08ee00420b99724c99769d8dcf70bbac2ccbb52acee8b09d4acd2d",
  "crates/trkr-desktop/src/power_meter.rs": "9fd22506c21f27a1bca406903e4d0f461bbdcdb528e1791723f7267a2aec3919"
}
```
