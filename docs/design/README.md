# UI style — "sticker"

Every Tutor UI (the upload/review website now, the student app later) uses the
**sticker** style: Turn 2 ("outlined, lime + lavender") in
[`Naija Exam Prep App.dc.html`](./Naija%20Exam%20Prep%20App.dc.html), screens 2a–2c.
Turn 1 (purple, no outlines) in the same file is an earlier exploration — don't
use it. `android-frame.jsx` and `support.js` are the design tool's runtime,
kept only so the file opens.

The web implementation lives in `apps/web/src/styles.css` (tokens) and
`apps/web/src/components/ui.tsx` (button, card, chip, badge). Use those rather
than restyling from scratch.

## Tokens

| Token | Value | Use |
|---|---|---|
| `ink` | `#16130F` | All text, every outline, button bases, dark pills |
| `cream` | `#FDFAF2` | Page background, empty progress tracks |
| `paper` | `#FFFFFF` | Cards, secondary buttons, option rows |
| `lime` | `#D6F84A` | Primary actions, correct answers, highlights, progress fill |
| `lavender` | `#C7B2F5` | Strong cards and bottom sheets, "answered" states |
| `lavender-soft` | `#E6DBFC` | Soft panels, option letter tiles, secondary chips |
| `muted` | `#6B6157` | Secondary text |
| `faint` | `#8A8177` | Disabled / not-yet text |
| `on-dark-muted` | `#A79E92` | Secondary text on ink |

## Type

- **Baloo 2**, weight 800 — headings, big numbers, buttons, option letters.
  Sizes: 28–30 page titles, 21–25 section/card titles, 16–18 buttons.
- **Nunito**, 700–800 — everything else. Question text 19–21/1.4 weight 700;
  body 15 weight 600–700; meta 12–13 weight 700.
- **Labels**: 11–12px, weight 800, uppercase, letter-spacing 1.2px.
- **Highlight** (`.hl`): a lime background block, like a highlighter. Use it rarely —
  only for the one thing on screen the reader must notice (e.g. the correct value
  in an explanation). Never on titles or decoratively.

## Shape

- **Outline everything**: `2.5px solid ink` on cards, buttons, chips, inputs,
  progress bars (2px on small chips/tiles). **No drop shadows, no gradients.**
- Radii: 16–18px cards and option rows, 20px hero cards, 13px small buttons and
  icon tiles, 8–10px chips, 24px top corners on bottom sheets.
- Filled colour blocks over white-with-thin-border; colour carries the hierarchy.

## Sticker button

A solid ink base with the coloured face lifted 4px; on press the face drops
onto the base.

```html
<button class="rounded-[16px] bg-ink">
  <span class="block rounded-[16px] border-[2.5px] border-ink bg-lime px-4 py-3.5
               font-display text-lg font-extrabold -translate-y-1 active:translate-y-0">
    Start
  </span>
</button>
```

Lime = primary, white = secondary, lavender = alternative. One lime button per
area.

## States

- **Correct option**: lime fill, 3px border, ink letter tile with lime letter, `CORRECT` chip.
- **Wrong pick**: struck through (2px), ink letter tile, outlined `YOUR PICK` chip.
- **Other options after reveal**: cream fill, 45% opacity.
- **Dark pill** (timer, XP, countdown, location badges): ink background, lime or cream text.
- **Chips**: cream or white, 2px ink border, radius 10, uppercase 11px label.

## Tone

Encouraging, not childish. A wrong answer is information ("Not this time —
here's why"), never failure. Celebrate streaks and level-ups, not single
questions.

Dark mode isn't designed for this style yet.
