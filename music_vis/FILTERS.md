# Visualizer filters

Experimental filters for making patterns in the spectrograms easier to see. They run
once when the visualizer loads a `.cwt` file, before anything is drawn, so they cost
nothing during playback.

Measured numbers in this document come from "They're Red Hot" (Blood Sugar Sex Magik).
They show what a filter does to the data. How useful it looks is a judgement made in the
visualizer with the music playing.

## Contents

- [How filters work](#how-filters-work)
- [Filter reference](#filter-reference)
- [Display options](#display-options)
- [Peak re-synthesis (`--peaks`)](#peak-re-synthesis---peaks)
- [Comparing chains side by side](#comparing-chains-side-by-side)
- [Ordering guide](#ordering-guide)
- [Relationships and conflicts](#relationships-and-conflicts)
- [Starting recipes](#starting-recipes)
- [Measuring a chain](#measuring-a-chain)

## How filters work

```
./run.sh play <song> -f <filter>[:p1,p2,...] -f <filter> ... [display options]
./run.sh filters                      # built-in help with every filter and default
```

- **Order matters:** filters apply left to right, each one working on the previous
  one's output.
- **Parameters:** they are optional unless marked *required*, and you can leave out
  trailing ones. `harmonic:0.3` keeps the default `bins`.
- **Magnitude filters** change everything drawn from amplitude: the bars, the magnitude
  spectrogram, and the brightness of the phase spectrogram.
- **`phase-*` filters** change only the colours of the phase-deviation spectrogram. That
  graph is hidden unless you pass `--phase`, so without it these filters have no visible
  effect, and the visualizer prints a note.
- **Display options** apply after all filters, whatever their position on the command
  line.
- **Old `CWT1` files** have no phase data, so `phase-*` filters are skipped with a note.
  Re-run `./run.sh cwt <song>` to upgrade.

## Filter reference

### Level and balance

| Filter | Defaults | What it does | Use it for |
|---|---|---|---|
| `tilt[:octaves]` | `2` | Divides by the song's average spectrum, smoothed over ±`octaves`. | Evening out the bass-heavy slope while keeping steady content such as a bass line. **Usually the best first filter.** |
| `whiten[:secs]` | `0` | Divides each bin by its own average: over the whole song when `secs` is 0, otherwise over a rolling window. | Making every bin equally prominent. Cancels steady content and boosts rarely used bins in quiet passages; see [tilt vs whiten](#tilt-vs-whiten). |
| `agc[:secs]` | `3` | Divides each frame by the loudest frame peak within a rolling window. | Showing quiet passages with the same detail as loud ones. |
| `gate[:db,knee]` | `-15,6` | Fades out frames that were quiet in the **original** audio. Loudness is measured against the 99th-percentile frame, with a linear fade `knee` dB wide. | Flattening quiet sections. |

### Floor and contrast

| Filter | Defaults | What it does | Use it for |
|---|---|---|---|
| `contrast[:k]` | `1` | In each frame, subtracts `k` × the median bar and clips at 0. | Removing the raised floor left by `tilt` or `whiten`, so only real peaks remain. |
| `power[:exp]` | `0.5` | Raises magnitudes to the power `exp`. Below 1 compresses, above 1 sharpens peaks. | Final shaping. Pair with `--peak-pct` when `exp` > 1. |

### Smoothing

| Filter | Defaults | What it does | Use it for |
|---|---|---|---|
| `smooth-time[:secs]` | `0.1` | Centred moving average along time. | Removing frame-to-frame jitter so structure lasting a few seconds stands out. |
| `smooth-freq[:bins]` | `2` | Moving average across ±`bins` neighbouring bins. | Merging closely spaced partials into a broader band. |

### Change and rhythm

| Filter | Defaults | What it does | Use it for |
|---|---|---|---|
| `flux[:secs]` | `0.05` | Positive rise in log magnitude compared with `secs` earlier. | Onsets and hits; rhythm shows as vertical lines. |
| `novelty[:secs]` | `1` | Amount above the rolling-average background, clipped at 0. | What stands out from the recent background. |
| `periodic:period[,count]` | period *required*, count `2` | Averages the data with itself shifted by ±k × `period` seconds, for k = 1 to `count`. | Reinforcing anything that repeats every `period` s, such as a bar or a riff. |

### Structure separation

| Filter | Defaults | What it does | Use it for |
|---|---|---|---|
| `harmonic[:secs,bins]` | `0.25,8` | Harmonic/percussive separation (HPSS, using median filters): keeps horizontal structure. | Sustained notes and melody, with drums removed. |
| `percussive[:secs,bins]` | `0.25,8` | HPSS: keeps vertical structure. | Drum hits, with sustained notes removed. |
| `fundamental[:amount,n,secs]` | `0.8,6,0.15` | Finds prominent peaks in each frame and matches them by frequency ratio (within 35 cents of a whole number k = 2 to `n`). It then moves `amount` of each harmonic peak into its fundamental. The decisions use data smoothed over `secs` so they don't flicker. | Collapsing each note toward one line. |

### Phase-deviation filters

These work on the phase graph's pitch deviation (in cents). Each value is weighted by the
**current** magnitudes, so quiet bins count less.

| Filter | Defaults | What it does | Use it for |
|---|---|---|---|
| `phase-smooth[:secs]` | `0.25` | Weighted moving average along time. | Steadier pitch colours. |
| `phase-var[:secs]` | `0.5` | Weighted rolling standard deviation: white means a stable pitch, red means wobble. | Telling steady tones from vibrato and noise. |
| `phase-detrend[:secs]` | `1` | Subtracts the weighted rolling average. | Pitch bends relative to the note's own average pitch. |

## Display options

| Option | Default | What it does |
|---|---|---|
| `--scale db\|linear\|sqrt` | `db` | How magnitude maps to bar height and colour. |
| `--db-range N` | `50` | dB below the reference level that is drawn black (db scale only). |
| `--dev-range N` | `50` | Cents of pitch deviation shown as full blue or red. |
| `--bar-colors note\|index` | `note` | Bar colours by musical note (pitch class: every C red, then around the colour wheel by semitone; octaves share a colour), or the old blue→red by position. |
| `--peak-pct P` | `100` | The reference level is the P-th percentile instead of the maximum; anything above it is drawn at full height. Use `99.5` after filters that remove the floor. |

## Peak re-synthesis (`--peaks`)

```
--peaks N[,secs,merge,width]          defaults: -,0.5,0,1
```

`--peaks` is not a filter: it runs once, after the whole filter chain, wherever it
appears among a panel's options. It replaces each frame with a clean drawing of what the
filters picked out:

1. **Find peaks:** the frame's prominent peaks, using the same peak detection as
   `fundamental` (3 dB prominence, within 30 dB of the frame's loudest).
2. **Merge (optional):** the strongest peak absorbs every peak within `merge` octaves of
   it. The combined peak sits at their amplitude-weighted position, with their combined
   energy (√(a² + b²)). The next strongest remaining peak does the same, and so on.
3. **Keep the N strongest:** N is 1 to 16.
4. **Normalize (optional):** each peak is divided by the loudest peak within ±`secs`/2,
   so a quiet verse reaches the same height as a loud chorus. Use `0` to keep the
   filtered levels. Frames more than 40 dB below the song's loudest peak are not
   raised to full height.
5. **Redraw:** each peak becomes the lobe a pure sine tone at that frequency produces in
   this CWT (Morlet, ω₀ = 6). `width` scales the lobe: below 1 is sharper than a real
   tone, which separates close peaks. Where lobes overlap, the taller one is drawn.

The phase-deviation graph is redrawn to match: a sine tone reads as its own frequency
in every bin it reaches, so each lobe is white at its centre, red below it and blue
above it.

Notes:

- **Merge distances:** `0.083` is about a semitone, `0.17` a whole tone, `0.25` a minor
  third. Too large a value merges separate notes of a chord into one.
- **Flicker:** a peak that hovers around N-th place appears and disappears from frame
  to frame. Smoothing in the chain before it (`smooth-time`) or a smaller N helps.
- **Comparing:** put the same chain on both panels and add `--peaks` to the right one.
  The difference overlay then shows exactly what was removed (black) and added (white).

Example:

```
./run.sh play <song> -f tilt -f contrast --peak-pct 99.5 --vs -f tilt -f contrast --peaks 6,0.5,0.1
```

## Comparing chains side by side

`--vs` starts a new panel with default settings, so the same song can be compared under
different filters. Everything after it applies to that panel only: its filters and its
display options (`--scale`, `--db-range`, `--dev-range`, `--peak-pct`, `--bar-colors`). Up to 4 panels
are allowed, and all of them follow the same playback position.

```
./run.sh play <song> -f tilt -f contrast --peak-pct 99.5 --vs -f whiten -f contrast --peak-pct 99.5
```

The terminal lists which filters went to the left panel and which to the right.

**Difference overlay.** The right panel's bar graph shows how it differs from the left
panel's, bin by bin:

- **White:** the right panel is higher; the white part is its excess height.
- **Black with a grey top edge:** the right panel is lower; the black part is the left
  panel's excess, and the grey line marks the left panel's height. The grey line is
  there because black alone barely shows on the dark background.

Heights are compared as drawn, so each panel's own scale (`--peak-pct`) is taken into
account. Press D to toggle the overlay. With more than two panels, each extra panel is
compared with the leftmost one.
`--phase` and `--windowed` are global, so they can go anywhere on the command line.

Keys: Space pauses, Left/Right seek 5 s, 1/3/2 set the speed to 1×/0.75×/0.5× (the
pitch drops with it), D toggles the difference overlay, F11 toggles fullscreen and Esc
quits. The window
opens fullscreen unless you pass `--windowed`.

## Ordering guide

The general shape of a chain:

```
balance  ->  separation  ->  floor  ->  derived / smoothing  ->  shaping  ->  gate  ->  phase-*
tilt         harmonic        contrast   flux, novelty,          power         gate      phase-smooth
(whiten)     percussive                 periodic, smooth-*                              phase-var
             fundamental                                                                phase-detrend
```

### Filters that belong at the front

- **`tilt` / `whiten`:** filters that compare bins with each other (`contrast`,
  `fundamental`, `smooth-freq`) behave better once the bass-heavy slope is gone.
- **`harmonic` / `percussive`:** their median filters need the spectrum's natural shape.
  After `contrast`, the many zeros skew the medians. After `flux` or `novelty`, the data
  are changes over time rather than spectra, so "horizontal" and "vertical" lose their
  meaning.

### Filters that belong at the back

- **`gate` must come after `agc`**, and after a rolling `whiten:secs`. Both renormalise
  loudness and would undo the gate. On the test song, `agc` placed after `gate` blew the
  gated frames back up (the data changed by roughly 33×).
- **`power` near the end.** If you place it after `gate`, it only reshapes the gate's
  fade curve, which is harmless.
- **`phase-*` filters last.** They are weighted by the magnitudes as they stand at that
  point. Placed after `gate`, gated frames stop counting, which is usually what you want.

### Filters that can go anywhere

- **`gate`** measures loudness on the original data, so earlier filters don't change
  *which* frames it fades. Only later filters can undo it (see above).
- **`fundamental`** uses only relative measures, so scaling its input scales its output
  identically. It behaved about the same before and after `tilt contrast`. Putting
  `contrast` first does give it cleaner peaks to match.

## Relationships and conflicts

### tilt vs whiten

They are alternatives, so choose one.

- **`whiten` alone:** it erases content that is present all song. The steady bass at
  about 62 Hz dropped from 0.44 to 0.04 of full height. It also boosts rarely used bins
  in quiet passages, so frames 18–30 dB down drew bars nearly as tall as loud ones.
- **`tilt`:** it keeps the bass (0.35) and the normal relationship between loud and
  quiet frames.
- **Both together:** `whiten` after `tilt` gives *exactly* the same result as `whiten`
  alone, because each bin's constant factor cancels.

### Raised floor → `contrast` → `--peak-pct`

- **The raised floor:** `tilt` and `whiten` lift every bin toward the same level. After
  `whiten`, peaks stood only about 2.5× above the floor, compared with about 12×
  unfiltered.
- **`contrast`:** it removes that floor. Bars then look short because only the parts
  above the floor remain.
- **`--peak-pct 99.5`:** it restores bar height. On the test song the typical tallest
  bar went from 0.19 to 0.54.
- **Other floor-removing filters:** the same applies after `flux`, `novelty` and
  `power` > 1.

### agc vs gate

They pull in opposite directions: `agc` brings quiet passages up and `gate` hides them.
Used together, `gate` must come after `agc`, and the result is "equalise loudness, except
for near-silence".

### harmonic vs percussive

- **Pick one.** They are complementary masks.
- **Chaining them** applies the percussive mask to data that is already harmonic-only.
  The result is neither, and is cluttered: 40 peaks per frame, against 7.8 unfiltered.
- **`harmonic` alone** also raises clutter (22.8 peaks per frame), because it sharpens
  many small sustained partials. Following it with `tilt contrast` removes most of that.

### fundamental

- **No harmonic peaks:** it leaves frames without matching harmonics unchanged, so a
  sparse or noisy passage looks the same as before.
- **No extra smoothing needed:** it already smooths its own decisions, so `smooth-time`
  directly after it adds nothing.
- **Pitch range:** above about the 6th harmonic, the CWT's frequency resolution can't
  separate neighbouring harmonics, so a larger `n` gains little.
- **Shared harmonics:** in chords, a harmonic shared by two notes goes to the lower
  fundamental, and notes an octave apart merge.
- **Only the part above the dips moves:** a harmonic is lowered toward the straight line
  between the dips on either side of it, and that amount is added on top of the
  fundamental's peak. The curve stays continuous where it meets its neighbours, so no
  vertical walls appear in the bars. Where a harmonic merges with a neighbouring tone
  (a shallow dip), only the part sticking up above the dip is removed.
- **Measured effect:** with the defaults it moved about 7% of total amplitude, without
  adding choppiness (roughness 0.26, the same as unfiltered). Neighbouring bins never
  differ by more than about a tenth of the bar height, the same as the raw data.

### smooth-time and flux

- **`smooth-time` before `flux`** removes jitter that would otherwise register as tiny
  onsets.
- **`smooth-time` after `flux`** spreads each onset into a wider stripe.

### periodic

`periodic` needs the repetition period in seconds. One bar of 4/4 lasts `4 × 60 / BPM`
seconds; at 120 BPM that is `periodic:2`. A wrong period blurs instead of reinforcing.

### Display scale for zero-floored output

`contrast`, `flux`, `novelty` and `percussive` output a lot of zeros or near-zeros.
The default `db` scale makes faint values visible, which can look noisy. `--scale sqrt`
or `--scale linear` often reads more cleanly.

## Starting recipes

These are starting points. Every one of them parses and loads, but none has been judged
visually yet.

| Goal | Command options |
|---|---|
| General pattern view | `-f tilt -f contrast -f gate --peak-pct 99.5` |
| Notes as single lines | `-f tilt -f contrast -f fundamental -f gate --peak-pct 99.5` |
| Melody (no drums) | `-f harmonic -f tilt -f contrast --peak-pct 99.5` |
| Rhythm | `-f percussive -f flux --scale sqrt --peak-pct 99.5` |
| Repetition at a bar length | `-f tilt -f smooth-time:0.3 -f periodic:<bar secs> --peak-pct 99.5` |
| Pitch stability | `--phase -f tilt -f gate -f phase-var` |

## Measuring a chain

An ignored test compares filter chains by numbers rather than by eye:

```
cd music_vis
CWT_PATH="<path to song>.cwt" CHAINS="tilt contrast;tilt contrast fundamental" \
  cargo test --release roughness -- --ignored --nocapture
```

Separate chains with `;` and filters within a chain with spaces. An empty chain (a
leading `;`) shows the unfiltered data.

| Metric | Meaning |
|---|---|
| `roughness` | Average frame-to-frame change relative to the average value. Higher means more flicker or choppiness. Unfiltered is about 0.26. |
| `peaks/frame` | Average number of local peaks per frame. Higher means a more cluttered spectrum. Unfiltered is about 7.8. |
| `last-filter-changed` | How much the last filter in the chain changed the data, as a % of total amplitude. Large values from re-normalising filters such as `agc` or `whiten` are expected. |
