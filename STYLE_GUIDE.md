# PageSeeds Design System — Style Guide

Both the **PageSeeds website** (`pageseeds/`) and the **PageSeeds app** (`pageseeds-app/`) share this design language. Keep them in sync.

---

## Fonts

| Role | Family | Weights | Usage |
|---|---|---|---|
| **Sans (body)** | Manrope | 400, 500, 600, 700, 800 | All UI text, labels, body copy |
| **Display (headings)** | Fraunces | 500, 600, 700 | Section headers, hero titles, marketing copy |

```html
<!-- Google Fonts import -->
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
<link href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,500;9..144,600;9..144,700&family=Manrope:wght@400;500;600;700;800&display=swap" rel="stylesheet" />
```

```css
--font-sans: 'Manrope', ui-sans-serif, system-ui, sans-serif;
--font-display: 'Fraunces', ui-serif, Georgia, serif;
```

**Tailwind classes:**
- Body text → default (Manrope via `font-sans` base)
- Display headings → `font-display`

---

## Brand Palette

These are the core brand tokens. Reference them in CSS as `var(--brand-*)` or with Tailwind's arbitrary values.

| Token | Light value | Dark value | Purpose |
|---|---|---|---|
| `--brand-forest` | `#213629` | `#eff6ee` | Primary text, headings, icon fills |
| `--brand-clay` | `#b5652b` | `#ebb06f` | Accent highlights, check icons, badges |
| `--brand-seed` | `#d6a047` | `#f0c974` | Warm gold accent, gradient fills |
| `--brand-cream` | `#faf3e8` | `#101712` | Page section backgrounds |
| `--brand-paper` | `#fffdf8` | `#162018` | Lightest background, card fills |
| `--copy` | `#53625b` | `#c1cec5` | Body copy, secondary text |

---

## Tailwind / ShadCN Color Tokens

All tokens use CSS variables mapped via `@theme`. Both projects are **Tailwind v4**.

### Light mode

| Token | Value (oklch) | Hex approx | Usage |
|---|---|---|---|
| `background` | `oklch(0.989 0.011 84.57)` | `#fffdf8` | Page background |
| `foreground` | `oklch(0.26 0.028 145.62)` | `#213629` | Primary text |
| `card` | `oklch(1 0 0 / 0.85)` | `white/85` | Card surface |
| `card-foreground` | same as foreground | | |
| `primary` | `oklch(0.36 0.055 145.4)` | `#2d5c3a` | Primary buttons, focused rings |
| `primary-foreground` | `oklch(0.99 0.008 95.1)` | `#fffdf8` | Text on primary |
| `secondary` | `oklch(0.97 0.014 84.4)` | `#f5ede0` | Secondary buttons |
| `secondary-foreground` | `oklch(0.31 0.026 145.74)` | `#2d4f3c` | Text on secondary |
| `muted` | `oklch(0.962 0.01 86.1)` | `#f0e8d8` | Muted backgrounds |
| `muted-foreground` | `oklch(0.52 0.02 146.3)` | `#53625b` | Placeholder/secondary text |
| `accent` | `oklch(0.95 0.022 75.4)` | `#f2e5cc` | Accent hover states |
| `accent-foreground` | `oklch(0.31 0.026 145.74)` | `#2d4f3c` | Text on accent |
| `border` | `oklch(0.88 0.015 84.6)` | `#ddd0bc` | Borders |
| `input` | same as border | | Input borders |
| `ring` | `oklch(0.71 0.06 63.2)` | `#c8903a` | Focus rings |
| `radius` | `0.9rem` | | Base border-radius |

### Dark mode

| Token | Value (oklch) |
|---|---|
| `background` | `oklch(0.16 0.018 145.2)` |
| `foreground` | `oklch(0.96 0.01 96.1)` |
| `primary` | `oklch(0.85 0.045 144.7)` |
| `muted` | `oklch(0.24 0.018 145.2)` |
| `border` | `oklch(0.3 0.015 145.2)` |

*(Full dark values are in `globals.css`)*

---

## Border Radius

`--radius: 0.9rem` is the base. ShadCN derives:

| Variable | Value | Tailwind |
|---|---|---|
| `--radius-sm` | `calc(0.9rem - 4px)` | `rounded-sm` |
| `--radius-md` | `calc(0.9rem - 2px)` | `rounded-md` |
| `--radius-lg` | `0.9rem` | `rounded-lg` |
| `--radius-xl` | `calc(0.9rem + 4px)` | `rounded-xl` |

Buttons use `rounded-full`. Cards use `rounded-xl` or `rounded-[1.6rem]`.

---

## Backgrounds

### Page background gradient (light)
```css
background:
  radial-gradient(circle at top left, rgba(214, 160, 71, 0.2), transparent 32%),
  radial-gradient(circle at 85% 0%, rgba(63, 102, 71, 0.14), transparent 36%),
  linear-gradient(180deg, var(--brand-paper) 0%, var(--brand-cream) 52%, #f6ecdd 100%);
```

### Subtle grid texture overlay (::before pseudo-element)
```css
opacity: 0.22;
background-image:
  linear-gradient(rgba(111, 91, 49, 0.08) 1px, transparent 1px),
  linear-gradient(90deg, rgba(111, 91, 49, 0.08) 1px, transparent 1px);
background-size: 36px 36px;
mask-image: radial-gradient(circle at 50% 15%, black, transparent 72%);
```

### Card background
```css
/* Standard card */
bg-white/72  /* or bg-white/74 */
/* Hero card */
bg-[linear-gradient(180deg,rgba(255,251,243,0.97),rgba(255,248,238,0.88))]
```

---

## Shadows

| Pattern | CSS |
|---|---|
| Standard card | `shadow-[0_18px_60px_rgba(75,52,21,0.08)]` |
| Elevated card | `shadow-[0_26px_90px_rgba(85,53,18,0.14)]` |
| Logo pill | `shadow-[0_12px_40px_rgba(35,51,34,0.08)]` |

---

## Typography Scale

Using Tailwind classes:

| Use | Classes |
|---|---|
| Hero H1 | `font-display text-5xl sm:text-6xl lg:text-7xl leading-[0.94] font-semibold text-[color:var(--brand-forest)]` |
| Section H2 | `font-display text-4xl leading-tight text-[color:var(--brand-forest)]` |
| Section H3 | `font-display text-3xl` |
| Body large | `text-lg sm:text-xl leading-8 text-muted-foreground` |
| Body default | `text-base leading-7 text-muted-foreground` |
| Body small | `text-sm leading-6 text-[color:var(--brand-forest)]` |
| Label/badge | `text-[0.72rem] tracking-[0.22em] uppercase` |
| Nav links | `text-sm font-medium text-muted-foreground` |

---

## Component Patterns

### Badge
```tsx
<Badge className="rounded-full border border-border/70 bg-white/80 px-4 py-1.5 text-[0.72rem] tracking-[0.22em] text-[color:var(--brand-forest)] uppercase shadow-sm">
  Label
</Badge>
```

### Primary Button
```tsx
<Button size="lg" className="rounded-full px-6">
  Action <ArrowRight className="size-4" />
</Button>
```

### Card
```tsx
<Card className="overflow-hidden border-border/60 bg-white/74 shadow-[0_18px_60px_rgba(75,52,21,0.08)]">
  ...
</Card>
```

### Inline stat/feature tile
```tsx
<div className="rounded-[1.6rem] border border-border/60 bg-white/72 p-5 shadow-[0_24px_80px_rgba(75,52,21,0.08)] backdrop-blur-sm">
  <p className="text-sm font-semibold text-[color:var(--brand-forest)]">Title</p>
  <p className="mt-2 text-sm leading-6 text-muted-foreground">Body</p>
</div>
```

### Check-list item
```tsx
<div className="flex items-start gap-3 rounded-2xl border border-border/50 bg-white/70 p-4">
  <CheckCircle2 className="mt-0.5 size-5 text-[color:var(--brand-clay)]" />
  <p className="text-sm leading-6 text-[color:var(--brand-forest)]">Point</p>
</div>
```

---

## Layout

- Max-width container: `width: min(1160px, calc(100vw - 2rem)); margin-inline: auto;` (`.page-wrap`)
- Section padding: `py-8` to `py-16`, `px-4`
- Grid gaps: `gap-5` to `gap-10`
- Sticky header: `sticky top-0 z-50 backdrop-blur-xl bg-[color:var(--header-bg)]/92`

---

## Selection colour
```css
::selection { background: rgba(214, 160, 71, 0.24); }
```

---

## ShadCN config
Both projects use style `"new-york"`, `baseColor: "zinc"`, `cssVariables: true`.
