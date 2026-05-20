# Hardware Smoke Test

English version: [`hardware_smoke_test.md`](hardware_smoke_test.md).

この checklist は API/GUI 変更後に everyday GUI (`kohdalab-gui`) を実機で短く確認するためのものです。通常実験に入る前に、接続、初期化、小さい move、短い run、output file、clean disconnect を確認します。

```powershell
uv run kohdalab-gui
```

known config profiles:

```text
src/kohdalab/config/trkr_config_kikuchi.json       CONEXCC / TRA12CC
src/kohdalab/config/trkr_config_kikuchi_agap.json  CONEXAGAP / AG-M100D
```

## Record

```text
Date:
Operator:
Config file:
Scanner profile: CONEXCC / CONEXAGAP
Lock-in model:
Delay-stage controller:
Notes:
```

## Preflight

- [ ] 正しい config file が load されている。
- [ ] lock-in resource、delay-stage port、scanner ports が実機と一致している。
- [ ] CONEXAGAP の場合、X/Y が同じ COM port を共有し、axis U/V が正しい。
- [ ] scanner scale label が actuator unit と合っている。TRA12CC は `sample um / mm`、AG-M100D は `sample um / deg`。
- [ ] output directory が writable で、壊したくない raw-data folder ではない。
- [ ] 小さい test move をしても sample clearance と range が安全。

## Connection

- [ ] `uv run kohdalab-gui` で起動。
- [ ] connect 前に `Read Live`。期待値: missing device が clean に報告され、GUI が固まらない。
- [ ] lock-in / COM-port の `Refresh`。期待値: VISA/serial refresh 中も GUI が応答する。
- [ ] `Lock-in` を connect。期待値: connected になり X/Y/R/Theta が更新される。
- [ ] `Delay Stage` を connect。期待値: `t` live position が更新される。
- [ ] `Scanner X` / `Scanner Y` を connect。期待値: `x/y` live position が更新される。
- [ ] `Disconnect All` 後に `Connect All`。期待値: 全 device が再接続される。

## Initialization

- [ ] delay-stage `Initialize`。期待値: initializing log、`t_ps=0` 方向へ移動、完了後 live `t` 更新。
- [ ] scanner X `Initialize`。期待値: scanner X initializing、origin move、live `x` 更新。
- [ ] scanner Y `Initialize`。期待値: scanner Y initializing、origin move、live `y` 更新。
- [ ] initialize 中、該当 controls が disabled になり完了後 re-enabled になる。

## Origins And Manual Moves

- [ ] `t/x/y` の current-origin button を押す。期待値: corrected values がほぼ zero。
- [ ] 小さい absolute delay move を1回実行し、元の値または `t=0` に戻す。
- [ ] 小さい corrected delay move。期待値: raw `t` と corrected `t_cor` が整合する。
- [ ] delay-stage Move button を連打。期待値: duplicate clicks が無視/debounce され、unexpected response error が出ない。
- [ ] scanner X の小さい absolute/corrected move を実行し、戻す。
- [ ] scanner Y でも同様に実行。
- [ ] Snapshot に `delay_stage_mm`, `delay_stage_pulse`, `x_scanner_*`, `y_scanner_*` が適切に出る。

## Short Measurement Runs

smoke test では小さい range と少ない point count を使います。

Signal Monitor:

- [ ] `n_points=3`、短い interval にする。
- [ ] Start。期待値: plot 更新、counter が 3 に到達、motion controls は不要に lock されない。
- [ ] `Save Now`。期待値: CSV に `timestamp, measurement, fast_axis, target_elapsed_s, elapsed_s, X_V, Y_V, R_V, Theta_deg` が含まれる。

TRKR:

- [ ] current origin 周辺の短い range、例 3 points。
- [ ] 最初の smoke run では `Return to zero` を有効。
- [ ] Start。期待値: delay-stage controls が lock、scanner controls は使える、plot 更新、最後に zero へ戻る。
- [ ] CSV に `measurement, fast_axis, target_t_cor_ps, t_cor_ps, t_ps, X_V, Y_V, R_V, Theta_deg, delay_stage_mm, delay_stage_pulse` が含まれる。

SRKR:

- [ ] X axis を選び、current origin 周辺の短い range、例 3 points。
- [ ] Start SRKR X。期待値: scanner X controls が lock、Y と delay-stage は使える、plot 更新、X が zero へ戻る。
- [ ] Y axis でも繰り返す。期待値: scanner Y controls が lock、SRKR tab に戻ると X rows も残っている。
- [ ] CSV に active scanner axis の `x_cor_um/x_um/x_scanner_*` または `y_cor_um/y_um/y_scanner_*` が含まれる。

STRKR / SRKR 2D:

- [ ] 初回は 2 x 2 の tiny range にする。
- [ ] STRKR を `t` と spatial axis 1つで Start。期待値: scanned delay/scanner controls だけ lock、unused axis は動かない。
- [ ] SRKR 2D を X/Y で Start。期待値: scanner X/Y controls が lock、delay-stage controls は使える。
- [ ] 左 plots が current fast-axis line、右 plots が fast axis を x、slow axis を y にした heatmap として更新される。
- [ ] heatmap は absolute max 正規化の red/blue diverging scale で表示される。
- [ ] ETA は最初は `-` のままで、first fast line と次の slow move が終わってから推定される。

## Lock-In Wait And Recovery

- [ ] TRKR の `Use TC*4`。期待値: connected lock-in の TC から wait が更新される。
- [ ] SRKR の `Use TC*4` も同様。
- [ ] STRKR / SRKR 2D の `Use TC*4` も同様。
- [ ] live status 中に `Use TC*4` を連打。期待値: overlapping click が無視され、VISA/GPIB listener error dialog が出ない。
- [ ] stale VISA session が疑わしい場合は lock-in disconnect/reconnect 後に retry。期待値: reconnect attempt が log され、app restart なしで回復。

## Disconnect And Restart

- [ ] `Disconnect All`。期待値: connected statuses が clear、unhandled exception なし。
- [ ] GUI を閉じる。期待値: worker thread warning や hang がない。
- [ ] device connected のまま GUI を閉じる。期待値: closing state になり、main window を固めず disconnect。
- [ ] `uv run kohdalab-gui` で再起動。
- [ ] 同じ config を load。期待値: saved origin/output settings が残る。
- [ ] `Connect All` でもう一度接続。
- [ ] `Disconnect All` して close。

## Pass Criteria

- [ ] everyday GUI が clean に open/close できる。
- [ ] individual connect/disconnect と Connect All/Disconnect All が動く。
- [ ] delay stage と scanner X/Y の Initialize が完了する。
- [ ] 小さい absolute/corrected moves で live values が整合する。
- [ ] Signal Monitor、TRKR、SRKR X/Y、STRKR、SRKR 2D が短い run を完了する。
- [ ] CSV headers が `kohdalab.api.measurement_rows` と一致する。
- [ ] logs に unexpected traceback がない。
- [ ] hardware-specific issue は config file、controller、port/resource、operation、exact log message と一緒に記録する。
