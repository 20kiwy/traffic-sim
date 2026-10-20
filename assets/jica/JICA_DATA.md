# JICA TR2 (11897576_02) — Road & Bridge Engineering Extraction

Source: JICA *Dar es Salaam Transport Policy and System Development
Master Plan*, Technical Report 2 (openjicareport.jica.go.jp/pdf/11897576_02.pdf).
All figures copied to `assets/jica/figs/`; constants implemented in
`crates/roads/src/tanzania.rs`. Extracted 2025 (Task 7).

## Priority intersections (Fig 5.5.1, p26)

| Site | Roads | JICA recommendation | In DSM scene |
|---|---|---|---|
| Ubungo (Kijazi) | Morogoro × Sam Nujoma × Nelson Mandela | flyover (built 2021) | ✔ split flyover |
| Tazara | Nyerere × Nelson Mandela | Mandela flyover over Nyerere | ✔ Fig 5.3.10 design |
| Gerezani (Bandari) | Kilwa × Bandari × Sokoine | BRT flyover + widenings | ✔ BRT flyover 260 m |
| Mwenge | Sam Nujoma × New Bagamoyo | next flyover candidate | ✔ at-grade 1L2TH1R |
| Morocco | Morogoro Rd | programmed upgrade | ✔ signalized |
| Magomeni | Morogoro Rd | programmed upgrade | ✔ signalized |
| Chang'Ombe | Nyerere Rd | upgrade with BRT | ✔ signalized |
| Selanda Bridge | Mandela × Kilwa | longer-term plan | ✔ signalized |
| Mandela Ring East | Nelson Mandela beltway | beltway reinforcement | ✔ signalized |

Road-class legend (Fig 5.5.1): 6-lane (under construction), 4-lane,
2-lane, other roads, railway.

## Tazara flyover — Fig 5.3.10 concept design (p12)

- PLAN S=1:2,000 / PROFILE V=1:500 H=1:2,000
- **BRIDGE LENGTH L=300.000 m**; approaches **L=137.303 m (W)** / **L=175.408 m (E)**
- Spans: PC hollow slab 45 + 45 | **PC box girder 120 m over Nyerere**
  (Fig 5.3.11 plan "PC Box Girder L=120,000") | hollow slab 45 + 45
- Vertical curves VC=90/4.2 (W), VC=100/3.5, VC=95/1.3 (E)
- Gradients ≈ 1.1 → 3.5 → 5.0 → 4.0 → 1.1 %, crown 7 m above Nyerere
- Two 2-lane decks, 17 m total width (§5.3.5), right-turn lanes under
  the flyover (Fig 5.3.11)

## Tazara at-grade (Fig 5.3.11 plan S=1:1,000, p13/30)

- Service roads both sides ("grading access"), channelization islands,
  zebra bars + stop lines on every Nyerere approach, bus lanes ("bus")
  both directions = BRT Phase 2, 4-phase 120 s signal G20/Y3/AR2
  (Fig 5.3.2), Bus Bay Type II, new 4500 mm concrete piles.

## Gerezani / Bandari (§5.4 + Table 5.4.2 p25, Fig 5.4.4–5.4.11)

- Bandari Road: widen to **six lanes over 1.67 km** (4 mixed + 2 BRT)
- Sokoine Drive (Option B): widen to 4 lanes over 1.45 km
- Bandari/Gerezani: 1300 m + 370 m to Kariakoo terminal
- **34 m six-lane section**: 3 m walk | 9.5 m carriageway | 9 m BRT
  median | 9.5 m | 3 m walk (→ lane ≈ 3.17 m)
- **27 m four-lane section**: 3 | 9.5 | 2 | 9.5 | 3
- **BRT-only flyover of Nyerere** (Fig 5.4.5): PC hollow girder,
  260 m bridge, 265 m transitions, **11 m width**, 8 spans, profile
  10+25+50+50+50+50+25+10, gradients 5/4 %
- **Gerezani Bridge** (Fig 5.4.10): 4-lane
- **BRT Port Bridge** (Fig 5.4.11): 2-lane BRT, 9 m + 2.5 m ped/NMV
  walkway; T-type PC girder 35 m / 34 m single span, pile foundation
- **Box culvert** 3.5 m × 5.0 m, 52 m single shell (41 m Option D)
- Costs: 20.9 / 26.6 / 33.1 billion TSh; construction 2.5–3.0 yr

## Ubungo (§5.2)

- 1L + 2TH + 1R approaches (Table 5.2.1, sat flow 1800/2000 pcu/h)
- 2-phase 130 s signal (Fig 5.2.7, AR2), BRT Phase I median busway,
  split-structure flyover (Fig 5.2.20): BRT turns between the decks

## Drainage

- §5.5.3: Nelson Mandela EC project = "considerable enhancements of
  road surface, **drainage** and traffic control over 16 km"
- TANROADS practice (implemented in `tanzania.rs`): roadside trapezoidal
  channel 0.6 m bottom × 0.7 m deep at 2.2 m offset; urban kerb+gutter
  0.45 m; JICA box culverts 3.5 × 5.0 m. Rendered by
  `roads::mesh3d::drain_parts` in 3D and drawn in the 2D plan.

## Image enhancement index (`assets/jica/figs/`)

| File | Content |
|---|---|
| page_13/fig5311_atgrade/fig5311_flyover | Tazara at-grade vs flyover plans (S=1:1000) |
| fig5310_plan / fig5310_profile | Tazara flyover concept design (420 dpi) |
| fig545_brt_flyover | BRT-only flyover profile + Option B map |
| page_26 | Fig 5.5.1 priority intersections map |
| page_23 | Bandari widening + BRT Port Bridge sections |
| page_19 / page_25 | §5.4.3 text + Table 5.4.2 |
