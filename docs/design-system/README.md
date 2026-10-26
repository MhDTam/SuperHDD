# Pastel liquid glass

Primary tokens are centralized in `apps/desktop/src/style.css`:
slate `#6E7F9A`, powder `#93B5C6`, mist `#C9CCD5`, lavender `#E4D8DC`,
blush `#FFE3E3`. Text uses `#14203D` and `#5F7090`. CSS components define
glass, interactive surfaces, shadows, spacing, focus rings and motion behavior.

Eight supplied screenshots in `UI/` are visual references only. Their numbers and
device models are never inferred to describe the user's actual hardware. Dynamic
UI remains actual HTML/React controls, including all charts and status labels.

Fonts use local Segoe UI/Arabic system fallbacks; no external font CDN is queried.
Lucide supplies the interface outline icons. The adopted application identity is
the user's `UI/ChatGPT Image Oct 21, 2026, 05_27_41 AM.png` artwork. Its transparent
monogram and complete logo are `apps/desktop/public/assets/brand-icon.png` and
`brand-logo.png`. The monogram appears in the sidebar and browser favicon; the
complete logo appears on the welcome screen. The old database SVG is retained as
a legacy source and is no longer used for application branding.

The built-in imagegen tool extracted transparent versions from the supplied
artwork. The original reference is preserved unchanged. Both extracted PNGs are
1254 by 1254 RGBA images with transparent exterior corners. Run `npm run icons`
to regenerate native platform sizes from `brand-icon.png`; the Windows ICO
contains six resolutions. Tauri explicitly bundles the generated PNG, ICO and
ICNS files, which are also used for the executable and installer identity.

Arabic strings are in `src/locales/ar.json`; the document direction mirrors the
layout, while `bdi`, explicit LTR inputs and isolated identifiers preserve paths.
Reduced-motion and light/dark preferences persist locally.

## Generated project asset

`apps/desktop/public/assets/storage-hero.png` was generated with the built-in
imagegen tool, with actual transparency preserved. No generated controls, labels
or statistics are used. Source output is preserved in the user's generated-images
directory. Final prompt:

> Use case: stylized-concept. Asset type: transparent decorative hero illustration
> for the Super HDD Windows desktop utility. Premium soft 3D sculptural silver HDD
> with an exposed metallic platter behind a smaller floating silver-blue SSD,
> gentle translucent frosted glass panes and two delicate powder blue and pale
> blush pink light ribbons connecting the devices, a pearl-white oval display
> plinth underneath. Three-quarter isometric perspective, centered composition,
> entire objects visible with generous clear margins. Brushed cool aluminium,
> pearl ceramic and frosted glass, soft diffuse shadows and pastel ambient
> illumination. Palette slate blue #6E7F9A, powder blue #93B5C6, cool gray #C9CCD5,
> very restrained lavender #E4D8DC and blush #FFE3E3. Transparent background; no
> letters, labels, numbers, logos, UI, controls, charts or watermark.
