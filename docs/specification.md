# SUPER HDD

The user's October 21, 2026 clarification in [product-scope.md](product-scope.md)
defines the current core workflow and acceptance criteria. Earlier broad feature
requests below must not obscure drive selection, capacity, cache inspection and
clearing, or selected-game/program caching.

## COMPLETE PRODUCTION DEVELOPMENT MASTER PROMPT
### Real SSD-Powered HDD Acceleration for Windows
### Premium Pastel Liquid Glass Desktop Experience

**Project Name:** Super HDD  
**Repository Name:** `super-hdd`  
**Application Name:** `SuperHDD.exe`  
**Windows Service:** `SuperHDDService`  
**Kernel Component:** `SuperHDDCacheDriver`  
**Target Platform:** Windows 11 x64, with Windows 10 x64 support where validated  
**Product Category:** SSD-Backed Transparent HDD Acceleration  
**Primary Design Language:** Pastel Blue & Lavender Liquid Glass  
**Primary User Experience:** One-Click Acceleration  
**Primary Engineering Requirement:** Real SSD Caching from the Beginning

---

# PART 1 — PROJECT IDENTITY AND OBJECTIVES

## 1. YOUR ROLE

Act as a complete senior engineering and design team with expertise in:

- Windows Kernel Development
- Windows Storage and Volume Filter Drivers
- Windows Driver Kit (WDK)
- C and C++ Systems Programming
- Rust Backend Development
- Native Windows Services
- Storage Caching Algorithms
- Filesystem and Block I/O
- Tauri Desktop Applications
- React and TypeScript
- Desktop UI/UX Design
- Glassmorphism and Modern Interface Engineering
- Windows Security
- Software Testing and Performance Benchmarking
- Software Installation and Distribution

Your assignment is to design, develop, integrate, test, and prepare a real Windows application called **Super HDD**.

Do not deliver only UI mockups, product concepts, architectural proposals, or simulated acceleration.

Build actual working software.

## 2. PRODUCT VISION

Super HDD is a premium Windows desktop application that allows users to accelerate mechanical hard drives using a portion of an SSD or NVMe drive as a persistent caching layer.

The application takes inspiration from the functionality of PrimoCache but must be dramatically simpler to configure and operate.

### Core vision

**Your HDD. Supercharged.**

The user should be able to:

1. Launch Super HDD.
2. Automatically discover installed HDDs and SSDs.
3. Select the HDD to accelerate.
4. Select the SSD that will contain cache data.
5. Choose exactly how much SSD space to allocate.
6. Click **Enable Acceleration**.
7. Have Super HDD activate real transparent SSD read caching.
8. Monitor real cache performance through a beautiful interface.

Do not make ordinary users understand:

- L1 and L2 caching
- Driver stack architecture
- Cache replacement algorithms
- Cache blocks
- Metadata journaling
- I/O request processing
- Write synchronization

These details should be handled automatically or exposed only in an Advanced section.

## 3. PRODUCT PRIORITIES

The project must prioritize:

1. Real, correct SSD-backed HDD acceleration.
2. User data safety.
3. Extremely simple one-click configuration.
4. User control over SSD cache capacity.
5. Modern, premium visual quality.
6. Minimal resource consumption.
7. Accurate performance measurement.
8. Stable background operation.
9. Intelligent download staging.
10. Professional Windows installation and updates.

Visual beauty and actual functionality are equally important, but visual polish must never hide an incomplete or unsafe storage engine.

---

# PART 2 — ABSOLUTE TECHNICAL REQUIREMENTS

## 4. REAL SSD CACHING IS MANDATORY

Super HDD must implement actual transparent HDD acceleration.

It must not substitute the caching engine with:

- Ordinary file copying.
- Disk cleanup.
- Moving applications to SSD.
- Symbolic links.
- Junction points.
- RAM disks.
- OS optimization tweaks.
- Fake acceleration dashboards.
- Artificial performance measurements.
- Download relocation alone.

A successful implementation must demonstrate that repeated eligible reads from a supported HDD volume can be fulfilled from an SSD-backed cache instead of accessing the original HDD for those blocks again.

The original HDD file paths must remain unchanged.

Applications should require no modification.

## 5. NO DEDICATED RAM CACHE

Super HDD must use **SSD storage exclusively for its dedicated HDD cache payloads**.

Strictly prohibited:

- L1 RAM caching.
- Dedicated in-memory HDD data cache.
- User-adjustable RAM caching.
- RAM disks.
- RAM-based deferred write caching.
- Keeping a second application-managed payload cache in memory.

Permitted:

- Normal program memory.
- Windows kernel memory.
- Temporary I/O buffers.
- Cache metadata.
- Synchronization structures.
- Block mapping indices.
- Operating-system memory caching.

Do not interfere with the ordinary Windows Cache Manager.

The requirement is that Super HDD does not create a dedicated RAM cache tier.

## 6. COMPLETE USER CONTROL

Users must control:

- Source HDD.
- SSD cache destination.
- Cache capacity.
- Automatic or manual sizing.
- Enable/disable.
- Pause/resume.
- Cache clearing.
- Capacity changes.
- Cache removal.

The user must be allowed to select a precise cache capacity, within validated safety and storage constraints.

Automatic recommendations are optional and must never override a valid manual selection.

## 7. NO POSTPONEMENT OF ACCELERATION

The real cache engine is a Day-One engineering requirement.

Do not implement a beautiful interface first and promise that real acceleration will be added in a later product version.

Kernel-driver development, Windows service development, and desktop UI development must begin as coordinated workstreams.

The first essential technical milestone is a verified SSD cache hit.

A working download manager is not sufficient to declare the application complete.

## 8. SAFE CACHING STRATEGY

The HDD is the authoritative data source.

The initial production design must use:

- Read-through caching.
- Coherent write handling.
- Safe invalidation.
- Correct synchronization.
- Recoverable cache metadata.
- Graceful cache failure behavior.

Do not implement unsafe asynchronous write-back caching.

The initial goal is safe read acceleration, not risky acknowledgment of writes before they reach the source HDD.

---

# PART 3 — NEW OFFICIAL VISUAL IDENTITY

# 9. PRIMARY DESIGN REFERENCE

**IMPORTANT: The newly supplied Super HDD interface designs are the official visual reference.**

They supersede the previous cream, beige, champagne, and gold designs.

Use the eight new screens as the visual source of truth whenever the reference images are available:

1. Welcome Screen
2. Acceleration Setup
3. Main Dashboard
4. Performance
5. Downloads
6. Gaming Mode
7. Storage Overview
8. Settings

The goal is to reproduce their overall design language, visual quality, layout proportions, material appearance, and interaction patterns while improving clarity and usability.

Do not independently invent an unrelated design.

Do not revert to the old warm beige identity.

If the reference images are unavailable, follow the detailed specifications below.

## 10. NEW DESIGN LANGUAGE

### Pastel Liquid Glass

The entire interface must use:

**Cool Pastel Minimalism + Frosted Liquid Glass + Soft 3D Hardware + Gentle Blue/Pink Lighting**

Visual characteristics:

- Light bluish-white environments.
- Muted slate-blue accents.
- Powder-blue translucent surfaces.
- Very subtle lavender tinting.
- Pale pink lighting.
- Frosted-glass panels.
- Soft diffuse shadows.
- Gentle volumetric depth.
- Silver-blue hardware illustrations.
- Thin white borders.
- Softly rounded shapes.
- Elegant typography.
- Clean, spacious layouts.

The design must feel premium and futuristic while remaining practical for a Windows utility.

It should not look like a website template, an RGB gaming dashboard, or an old administrative tool.

## 11. EXACT PRIMARY COLOR PALETTE

Use the following five colors as the main brand palette.

### Primary Slate Blue

`#6E7F9A`

Usage:

- Main action buttons.
- Selected sidebar indicators.
- Important interactive controls.
- Primary cache usage visuals.
- Active slider regions.
- Selected toggle backgrounds.
- Prominent chart accents.

### Powder Blue

`#93B5C6`

Usage:

- Secondary progress bars.
- SSD cache visualizations.
- Cool translucent backgrounds.
- Secondary chart series.
- Icon backgrounds.
- Active interface highlights.

### Cool Mist Gray

`#C9CCD5`

Usage:

- Inactive controls.
- Neutral surfaces.
- Dividers.
- Capacity-track backgrounds.
- Secondary glass tinting.
- Subtle borders.

### Soft Lavender

`#E4D8DC`

Usage:

- Secondary glass surfaces.
- Muted decorative reflections.
- Soft background gradients.
- Low-priority card accents.
- Ambient UI depth.

### Blush Pink

`#FFE3E3`

Usage:

- Delicate ambient lighting.
- Decorative highlights.
- Secondary chart accents.
- Special feature illustrations.
- Warm balancing elements.

Pink must remain soft and restrained.

Do not use vibrant magenta or highly saturated red.

## 12. SUPPORTING UI COLORS

Use supporting neutral colors only to improve readability and hierarchy.

Suggested values:

Main Background:
`#F7F9FD`

Secondary Background:
`#EDF3FA`

Elevated Background:
`#F5F7FD`

Primary Text:
`#14203D`

Secondary Text:
`#5F7090`

Muted Text:
`#7F8CA4`

Glass Surface:
`rgba(255,255,255,0.52)`

Elevated Glass:
`rgba(255,255,255,0.70)`

Glass Border:
`rgba(255,255,255,0.78)`

Soft Divider:
`rgba(110,127,154,0.12)`

Primary Button:
Use a subtle gradient based on `#6E7F9A` and a lighter slate-blue shade.

Danger actions:
Use accessible muted red or rose tones.

Success indicators:
Use restrained, readable green only when conveying actual success.

All colors must be centralized in theme tokens.

Do not scatter hardcoded colors across the codebase.

## 13. GRADIENT SYSTEM

Use subtle atmospheric gradients.

Preferred compositions:

- Bluish white transitioning into powder blue.
- Powder blue blending into lavender.
- Lavender blending into pale pink.
- Frosted white over softly colored backgrounds.

Example:

`linear-gradient(135deg, #F7F9FD 0%, #E9F1FC 38%, #E4D8DC 75%, #FFE3E3 100%)`

This is an example, not a mandatory gradient for every surface.

Avoid:

- Strong rainbow gradients.
- Overly saturated blues.
- Gold accents.
- Bright purple.
- Heavy black gradients.
- Harsh contrast.
- Excessive glowing borders.

The palette should feel coordinated, soft, and consistent.

## 14. GLASS MATERIAL ENGINE

Create a reusable glass material design system.

Recommended material variants:

### Glass Base

- Semi-transparent white tint.
- Soft background blur.
- Thin pale border.
- Minimal shadow.
- Low visual elevation.

### Glass Elevated

- Higher-opacity frosted surface.
- Stronger blur.
- Slightly brighter edge reflection.
- Gentle diffuse shadow.

### Glass Interactive

- Soft blue active states.
- Smooth hover brightening.
- Subtle inset highlights.
- Visible keyboard focus.

### Glass Decorative

- More transparent.
- Used behind 3D illustrations.
- May have gentle blue/lavender reflections.
- Never used behind dense text without readability protection.

Example CSS concepts:

`backdrop-filter: blur(22px) saturate(125%);`

`background: rgba(255,255,255,0.58);`

`border: 1px solid rgba(255,255,255,0.75);`

Adapt these values based on actual Windows rendering performance.

Frosted glass should look polished, not foggy or excessively blurry.

## 15. BORDER RADIUS SYSTEM

Recommended starting values:

- Main application shell: 26–30 px.
- Large dashboard panels: 20–24 px.
- Standard cards: 16–20 px.
- Input fields: 12–14 px.
- Buttons: 12–16 px.
- Small chips: 9–12 px.
- Floating illustrations: custom smooth silhouettes.

Avoid excessively round pill-shaped controls everywhere.

The interface should maintain a professional desktop feel.

## 16. SHADOW AND LIGHTING SYSTEM

Use:

- Wide, diffuse shadows.
- Cool blue-gray shadow tints.
- Very soft inner white highlights.
- Subtle pink ambient reflections.
- Almost invisible panel elevation.
- Soft separation between surfaces.

Avoid:

- Heavy black shadows.
- Strong drop-shadow outlines.
- Excessive neon glow.
- Bright colored shadows around every card.

The UI should feel softly illuminated, not artificially glowing.

## 17. TYPOGRAPHY

### English

Preferred:

- Segoe UI Variable
- Inter

### Arabic

Preferred:

- Thmany font, if available and properly licensed.

Fallback:

- IBM Plex Sans Arabic.

Support full RTL layouts.

Suggested visual hierarchy:

- Main page heading: 28–36 px.
- Large onboarding heading: 48–64 px.
- Section heading: 18–22 px.
- Card title: 15–18 px.
- Normal text: 13–15 px.
- Labels: 12–14 px.
- Secondary metadata: 12–13 px.

These are starting points for Windows desktop scaling.

Avoid tiny text.

Maintain strong legibility against frosted surfaces.

## 18. ICON SYSTEM

Use clean, consistent outline icons.

Recommended:

- Lucide.
- Custom SVG icons.
- Custom SSD/HDD illustrations.

Sidebar icons should be:

- Consistent stroke width.
- Minimal.
- Soft blue/slate.
- Properly aligned.
- Approximately 19–22 px.

Do not mix several unrelated icon styles.

## 19. PREMIUM 3D GRAPHIC SYSTEM

Use a carefully controlled 3D illustration language.

The new designs feature:

- Floating silver HDD hardware.
- Silver-blue SSD devices.
- Smooth glass-like panels.
- Blue/pink light trails.
- Translucent floating objects.
- Soft rounded geometric forms.
- Diffuse pastel ambient light.

Use similar materials:

- Brushed cool aluminum.
- Pearl-white ceramic.
- Frosted glass.
- Silver-blue metallic surfaces.
- Translucent lavender acrylic.
- Soft pink reflective lighting.

Illustrations should be realistic enough to communicate storage hardware but stylized enough to match the premium UI.

Avoid generic cartoon devices.

Avoid overly complex photorealistic scenes.

### Important

Do not bake essential UI controls or dynamic text into generated 3D illustrations.

Keep labels, charts, device names, status information, and buttons as real interface elements.

Illustrations are decorative assets, not substitutes for functional UI.

## 20. MOTION DESIGN

Use fluid, subtle animation.

Required interactions:

- Sidebar selection transitions.
- Hover responses.
- Button press animations.
- Smooth slider movement.
- Soft card entrance.
- Chart transitions.
- Cache activation feedback.
- Storage allocation changes.
- Gentle data-flow animation in the main dashboard.

Recommended timing:

- Hover: 120–180 ms.
- Button: 140–220 ms.
- Panel transitions: 200–350 ms.
- Longer decorative animation: carefully optimized.

Use restrained easing such as cubic-bezier curves or subtle springs.

Respect reduced-motion preferences.

Do not create heavy continuous GPU animations in a background utility.

---

# PART 4 — APPLICATION LAYOUT SYSTEM

## 21. WINDOW ARCHITECTURE

Design Super HDD as a premium native Windows desktop application.

Primary reference canvas:

**1440 × 900 px**

The UI must also adapt to:

- 1366 × 768
- 1600 × 900
- 1920 × 1080
- High-DPI scaling
- Different Windows text scaling settings

Use flexible responsive layout rules.

Do not create a fixed screenshot-sized interface.

Recommended minimum usable window size:

Approximately 1050 × 680, subject to testing.

Provide appropriate scrolling when the window becomes smaller.

## 22. WINDOW SHELL

Use a visually integrated Windows title bar.

Top-left:

Super HDD brand icon and product name.

Top-right:

- Optional quick settings.
- Minimize.
- Maximize/restore.
- Close.

Preserve standard Windows window-management behavior, keyboard accessibility, and snap support where technically feasible.

Use smooth rounded outer corners where supported.

## 23. SIDEBAR NAVIGATION

Follow the new designs.

Use a narrow, elegant sidebar with soft glass styling.

Suggested width:

230–265 px.

Main sections:

- Home
- Acceleration
- Performance
- Downloads
- Gaming
- Storage
- Advanced
- Settings

Sidebar characteristics:

- Very light translucent background.
- Thin separation border.
- Generous vertical spacing.
- Consistent outline icons.
- Muted slate-blue text.
- Blue selected-item background.
- Thin darker-blue selection indicator.
- Smooth selection transition.

The active item should have a soft blue background resembling the supplied designs.

Avoid strongly colored rectangular buttons.

## 24. MAIN CONTENT AREA

Use a consistent page structure.

Top:

- Page title.
- One-sentence description.
- Contextual controls.

Middle:

- Main feature content.

Bottom:

- Secondary actions, detailed statistics, or helpful information.

Recommended page padding:

28–36 px.

Use an 8 px spacing scale.

Avoid inconsistent margins, excessive vertical gaps, and uneven card heights.

Every screen must feel part of one coherent design system.

---

# PART 5 — COMPLETE SCREEN IMPLEMENTATIONS

# SCREEN 01 — WELCOME

## Purpose

Introduce Super HDD through a premium onboarding experience.

This screen should closely match the new blue/pink welcome design.

## Layout

Two-column hero composition.

Left side:

- Super HDD branding.
- Large headline.
- Short description.
- Primary action button.

Right side:

- Large sculptural HDD.
- Floating silver-blue SSD.
- Translucent layers.
- Subtle blue/pink illuminated ribbons.
- Soft floating shapes.

Bottom:

A large horizontal frosted-glass panel containing four feature highlights.

## Main Heading

**Make Your HDD Super Fast.**

## Supporting Text

"Use a part of your SSD to accelerate your HDD with one click. Faster loading, smoother performance, same files, no complicated settings."

## Main Action

**Get Started**

## Feature Highlights

- Real Acceleration — SSD Powered.
- Simple Setup — One Click.
- Safe & Reliable — Your Data Stays Safe.
- Modern Experience — Beautiful & Easy.

Use small soft-blue/pink circular icon backgrounds.

## Visual Requirements

The hero illustration should be the most visually expressive element in the application.

Use:

- Silver HDD.
- Silver SSD.
- Translucent glass sheets.
- Gentle pastel lighting.
- Floating soft shapes.
- Subtle curved light ribbons.

Do not overload the composition.

Do not add unnecessary technical statistics.

## Functionality

- Begin initial setup.
- Select language.
- Continue to hardware discovery.
- Remember completed onboarding.

---

# SCREEN 02 — ACCELERATION SETUP

## Purpose

Allow users to configure real SSD-powered HDD acceleration with minimal effort.

This is the most important functional screen.

## Layout

Page title:

**Accelerate Your HDD**

Subtitle:

"Select your drives and allocate SSD space for caching."

First row:

Two large equal-width glass cards.

### Left Card — Source HDD

Display:

- Physical HDD illustration.
- Drive letter.
- Model name.
- Capacity.
- Used space.
- Interface.
- Selectable source HDD.

Example display:

D: HDD  
Seagate Barracuda 4 TB  
HDD / SATA

Use actual detected information in production.

### Right Card — Cache SSD

Display:

- Silver-blue SSD illustration.
- Drive letter.
- Model name.
- Capacity.
- Available storage.
- SSD/NVMe indicator.
- Selectable cache destination.

Example display:

C: NVMe SSD  
Samsung 980 Pro 1 TB  
SSD / NVMe

## Cache Capacity Panel

Below the two device cards:

Create one wide glass panel.

Header:

**Cache Size**

On the right:

- Numeric input.
- Storage unit selector.

Main control:

A long smooth slider.

Use a muted slate-blue fill and a premium circular thumb.

Show:

- Minimum eligible allocation.
- Maximum safe allocation.
- Exact selected capacity.

Below the slider:

Two large selectable modes.

### Automatic

"Recommended size based on available SSD space and usage."

### Custom

"Manually set the cache size that works for you."

Use soft glass selection states rather than ordinary radio buttons.

## Recommendation Row

Two information cards:

**Recommended: 150 GB**

**Remaining SSD Space: 220 GB**

These values are illustrative; actual figures must be calculated dynamically.

## Primary Button

A large full-width slate-blue gradient button:

**Enable Acceleration →**

This must be visually prominent.

## Functionality

The interface must connect to actual backend operations:

- Detect drives.
- Select HDD.
- Select SSD.
- Check compatibility.
- Calculate available SSD space.
- Suggest capacity.
- Accept custom capacity.
- Configure cache.
- Activate caching.
- Report driver errors.
- Show activation progress.

Do not activate caching when the selected storage configuration is unsupported.

---

# SCREEN 03 — MAIN DASHBOARD

## Purpose

Present acceleration status and performance in one elegant overview.

Follow the new blue/pink dashboard closely.

## Header

Title:

**Supercharge Your HDD**

Subtitle:

"Use SSD caching to make your HDD faster and more responsive."

Top-right:

A compact status glass card.

Example:

**Acceleration Active**

Include a small readable green success indicator.

Do not use green as the primary application accent.

## Main Visual Card

Create a wide premium frosted-glass scene.

Left:

Physical source HDD illustration.

Center:

Floating translucent SSD Cache module.

Right:

SSD/NVMe illustration.

Connect them with subtle animated flowing lines.

Suggested labeling:

Source HDD  
SSD Cache  
Cache SSD

Important technical detail:

The visualization is conceptual.

It must not imply that all data automatically travels through the cache.

The animation should reflect actual cache activity whenever possible.

When idle, keep animation minimal.

## Storage Indicators

Show source and cache-device capacity with accurate progress indicators.

## Performance Cards

Below the main visualization, display four cards.

### Card 1 — Cache Hit Rate

Large circular indicator.

Actual percentage obtained from driver telemetry.

### Card 2 — Cache Used

Circular capacity indicator.

Show actual occupied cache and allocated capacity.

### Card 3 — SSD Served Reads

Display genuine read throughput served from SSD cache.

Include a small activity sparkline.

### Card 4 — HDD Read Activity

Display actual source HDD read activity.

Include a small activity sparkline.

Use the pastel palette consistently.

## Bottom Actions

- Pause
- Configure
- Performance
- Clear Cache

Dangerous or disruptive actions must request confirmation.

## Bottom Status Bar

Example:

"Super HDD is running in the background."

Provide a details link.

## Special States

Design polished states for:

- Acceleration Active
- Paused
- Cache Warming Up
- Disabled
- Driver Missing
- Unsupported Device
- SSD Unavailable
- Error / Recovery Required

No fake active status.

---

# SCREEN 04 — PERFORMANCE

## Purpose

Offer detailed, accurate performance monitoring.

## Header

Title:

**Performance**

Subtitle:

"Real-time cache statistics and disk activity."

Top-right:

Time range selector:

- Live
- 1 Hour
- 1 Day
- 1 Week
- 1 Month

Historical views must display actual stored telemetry.

## Main Charts

Two large side-by-side glass chart cards.

### Left — Cache Hit Rate

Display:

- Smooth line/area chart.
- Percentage axis.
- Time axis.
- Current value.
- Hover tooltips.

Use slate-blue and powder-blue chart styling.

### Right — Read Throughput

Display:

- SSD Cache throughput.
- HDD read throughput.
- Separate clearly labeled series.
- Actual units.
- Time axis.
- Hover values.

Use:

SSD Cache: Powder Blue.

HDD: Soft Pink/Rose, with adequate contrast.

Do not use misleading stacked graphs if data series are not logically additive.

## Lower Metrics

Create six compact cards.

1. Cache Hits.
2. Cache Misses.
3. Data Served from SSD.
4. Data Read from HDD.
5. Cache Occupancy.
6. Cache Evictions.

Each card:

- Icon.
- Large metric.
- Short label.
- Small activity visualization where meaningful.

## Benchmarking

Include a secondary benchmarking panel or action.

Support controlled performance tests.

Report:

- Baseline HDD read performance.
- Cold-cache behavior.
- Warm-cache behavior.
- Real cache hit rate.
- Source HDD I/O avoided.
- Measured throughput/latency improvement.

Clearly label sample/demo data when used during UI development.

Do not treat ordinary Windows RAM caching as evidence of Super HDD SSD acceleration.

---

# SCREEN 05 — DOWNLOADS

## Purpose

Manage SSD-assisted downloads in a clean, modern interface.

## Header

Title:

**Downloads**

Subtitle:

"Download files to SSD first, then move them to HDD."

Top-right:

**+ New Download**

Use a slate-blue primary button.

## Main Layout

Two-column composition.

Left:

Download list.

Right:

Download settings.

## Download List

Tabs:

- Active
- Completed
- Scheduled

Each download card includes:

- File icon or thumbnail.
- Filename.
- Total size.
- Current progress.
- Progress bar.
- Current download speed.
- Estimated remaining time when reliable.
- Pause/resume action.
- Open-folder action.
- Cancel action.

The UI must distinguish between:

- Downloading.
- Verifying.
- Moving to HDD.
- Completed.
- Paused.
- Failed.

Use soft white glass cards separated by thin dividers.

## Download Storage Panel

At the bottom:

Display:

- Allocated staging capacity.
- Used space.
- Free space.
- Visual capacity bar.

Use blue progress indicators.

## Right Settings Panel

Options:

### After Completion

- Move to HDD.
- Keep on SSD.
- Ask before moving.

### When to Move

- Immediately.
- When idle.
- On schedule.

### Destination

User-selected destination folder.

### Download Speed Limit

Optional.

### Notifications

Enable/disable completion alerts.

## Functional Requirements

Implement an actual managed downloader.

Use the SSD for staging.

After successful completion, move/copy data safely to the chosen HDD location.

Verify completed transfers before deleting source staging files.

Do not claim that SSD staging increases internet connection speed beyond network limitations.

---

# SCREEN 06 — GAMING MODE

## Purpose

Provide gaming-oriented SSD caching policies through a friendly visual experience.

## Header

Title:

**Gaming Mode**

Subtitle:

"Optimized for faster game loading and smoother asset access."

Top-right:

A large premium toggle:

Enabled / Disabled.

## Main Layout

Left:

Large gaming illustration.

Right:

Vertical optimization feature cards.

## Illustration

Use:

- Floating modern game controller.
- Soft white/silver material.
- Pastel blue lighting.
- Translucent panels.
- A small number of game-inspired visual elements.

Do not overwhelm the page with game-cover artwork.

## Optimization Cards

### Optimized Cache Policy

Prioritize frequently reused game data.

### Smart Prefetching

Prefetch eligible data based on validated access patterns.

### Reduced Cache Pollution

Avoid unnecessary caching of one-time sequential reads.

### Persistent Cache

Retain valid frequently accessed data across restarts when safe.

Each card must correspond to a real implemented optimization.

## Detected Games

At the bottom:

Display supported detected games installed on HDD volumes.

Each item:

- Game image or icon.
- Game title.
- Storage size.
- Acceleration preference toggle.
- Source drive.

Use genuine game discovery where possible.

Do not fabricate installed games.

If game discovery cannot be performed, provide manual selection or an honest empty state.

## Technical Requirement

Gaming mode changes must affect real caching policies.

It must not simply change the UI theme or display animated graphics.

Do not guarantee FPS improvements.

---

# SCREEN 07 — STORAGE OVERVIEW

## Purpose

Explain how HDD and SSD capacity is being used.

## Header

Title:

**Storage Overview**

Subtitle:

"See how your SSD and HDD storage is being used."

## Device Cards

Use the new design's large horizontal device cards.

### SSD Device Card

Left:

3D SSD illustration.

Top:

- Drive letter.
- Device model.
- Device capacity.
- Used percentage.

Main visualization:

A horizontal segmented capacity bar.

Categories:

- System & Apps.
- Super HDD Cache.
- Download Staging.
- Other.
- Free Space.

Use colors from the approved palette.

Each category should include its numerical size.

Ensure every segment represents actual SSD storage usage.

### HDD Device Card

Display:

- HDD illustration.
- Device details.
- Used space.
- Free space.
- Capacity breakdown where supported.

Important:

Do not incorrectly represent SSD cache allocation as physical storage consumed on the HDD.

Cache capacity must be attributed to the SSD on which it resides.

## Bottom Summary

Display:

- Total Storage.
- Used Space.
- Free Space.
- Overall Usage.

Use an elegant, compact glass summary panel.

## Functionality

- Real Windows drive discovery.
- Accurate capacities.
- Cache allocation inspection.
- Safe resizing.
- Cache removal.
- Device refresh.
- Multiple-device support.

Do not rely only on drive letters to determine physical storage relationships.

---

# SCREEN 08 — SETTINGS

## Purpose

Provide organized configuration without overwhelming users.

## Header

Title:

**Settings**

Subtitle:

"Customize Super HDD to fit your workflow."

## Main Layout

Left:

Settings and controls.

Right:

Premium decorative illustration panel.

Use the supplied design's large soft-blue/silver 3D gear.

The illustration must remain decorative.

## Horizontal Settings Tabs

- General
- Performance
- Downloads
- Advanced
- Language

Use a soft blue selected tab.

## General

Settings:

### Start with Windows

Launch Super HDD at startup.

### Start Minimized

Start in the system tray.

### Check for Updates

Automatically check for application updates.

### Theme

- Light
- Dark
- Follow System

### Language

- English
- Arabic

### Notifications

Enable important system notifications.

## Performance

Options:

- Default cache policy.
- Cache persistence.
- Prefetch policy.
- Background operation limits.
- Telemetry history.
- Benchmark settings.

## Downloads

Options:

- Default staging SSD.
- Staging allocation.
- HDD destination.
- Transfer policy.
- Download limits.
- Completion notifications.

## Advanced

Options:

- Cache block size.
- Replacement policy.
- Cache diagnostics.
- Driver logs.
- Metadata validation.
- Cache reset.
- Driver/service status.
- Error reporting.

Only show controls that are truly implemented.

## Language

Support English and Arabic with complete RTL adaptation.

---

# PART 6 — ADDITIONAL REQUIRED STATES AND SCREENS

## 25. DRIVER INSTALLATION SCREEN

Create a beautiful system-setup screen using the same design system.

Show:

- Driver compatibility.
- Required permissions.
- Installation progress.
- Actual driver status.
- Restart requirement.
- Recovery information.

No traditional plain administrative dialog unless Windows itself supplies one.

Do not silently bypass Windows security.

## 26. SYSTEM HEALTH SCREEN

Use premium glass status cards.

Display:

- Cache Driver Status.
- Windows Service Status.
- Source HDD Health.
- Cache SSD Availability.
- Cache Metadata Integrity.
- Last Recovery.
- Recent Errors.

Include:

- Run Diagnostics.
- Export Logs.
- Reset Cache.
- Restore Defaults.

Use actual diagnostic data.

## 27. EMPTY STATES

Design polished empty states for:

- No HDD detected.
- No compatible SSD detected.
- No active cache.
- No downloads.
- No games found.
- No performance history.

Each state needs:

- Small relevant illustration.
- Clear message.
- Short explanation.
- Useful next action.

Do not fill empty areas with decorative clutter.

## 28. LOADING AND ERROR STATES

Provide:

- Skeleton loaders.
- Device scanning progress.
- Cache preparation progress.
- Activation progress.
- Driver installation states.
- Recoverable failure messages.
- Retry actions.

Do not create fake progress percentages.

If actual progress cannot be measured, use a clearly indeterminate indicator.

---

# PART 7 — REAL WINDOWS CACHE ENGINE

## 29. KERNEL DRIVER

Implement a real Windows driver named:

`SuperHDDCacheDriver`

Recommended language:

C or C++ using Windows Driver Kit.

The driver must support transparent SSD-backed caching for a verified Windows storage configuration.

Begin driver work immediately.

Do not postpone it until the UI is complete.

## 30. DRIVER ARCHITECTURE RESEARCH

Evaluate:

- Volume filters.
- Storage filters.
- File-system minifilters.
- Documented Windows storage APIs.

Choose the interception point that can satisfy the required transparent caching and coherency behavior.

A volume-level filter is an initial candidate, not an assumed universally correct solution.

The chosen architecture must account for:

- Cached I/O.
- Noncached I/O.
- Paging I/O.
- Memory-mapped files.
- Concurrent I/O.
- Source writes.
- Flush requests.
- Cache invalidation.
- Device removal.
- Shutdown.
- Filesystem and storage-stack interactions.

Document the decision, then implement it.

Do not claim universal caching coverage before validating the relevant paths.

## 31. FIRST SUPPORTED ENVIRONMENT

Focus the first working implementation on:

- Windows 11 x64.
- Compatible local HDD.
- NTFS data volume.
- Separate compatible SSD/NVMe device.
- Validated cache backing storage.

This limited hardware scope is acceptable initially, but caching within that scope must work.

Do not initially enable unvalidated system-volume or encrypted-volume caching.

Add broader compatibility after testing.

## 32. CACHE READ PROCESS

When an eligible read arrives:

1. Identify the source volume.
2. Determine the logical block range.
3. Look up the SSD cache mapping.
4. Validate the cache entry.
5. If a valid cache hit exists, read from SSD.
6. If missing, read from HDD.
7. Return correct data.
8. Admit eligible missing blocks into SSD cache.
9. Update actual statistics.

Support partial hits, multi-block reads, and concurrent operations.

Use safe asynchronous cache population where appropriate.

## 33. CACHE WRITE AND COHERENCY

HDD remains authoritative.

When a write occurs:

- Preserve correct source HDD write behavior.
- Coordinate overlapping reads.
- Invalidate or update affected cached blocks.
- Prevent stale cache entries.
- Handle partial-block writes.
- Maintain correct completion semantics.

Do not acknowledge an authoritative HDD write simply because it reached SSD.

Use a formal coherency protocol with appropriate synchronization and generation tracking.

## 34. CACHE BACKING STORAGE

Prefer application-managed SSD storage without requiring ordinary users to create partitions.

Investigate:

- Safe preallocated cache files.
- Validated backing-store access.
- Filesystem-compatible allocation.
- Bounded persistent metadata.

Prevent:

- Recursive caching I/O.
- Storage-stack deadlocks.
- Cross-device dependency loops.
- Unsafe raw-sector writes.
- Cache metadata corruption.

Do not assume an ordinary user-mode cache file can safely be read from an arbitrary kernel path without an engineered design.

Prove the backing-store mechanism before using it in production.

## 35. CACHE ALGORITHMS

Implement:

- Cache mapping.
- Cache hit/miss.
- Admission policy.
- Eviction.
- Safe cache population.
- Write invalidation.
- Metadata persistence.
- Cache resizing.
- Error recovery.

Evaluate algorithms such as:

- CLOCK.
- Segmented LRU.
- TinyLFU.
- Frequency/recency hybrid.

Choose based on maintainability and measured performance.

Avoid automatically admitting large one-time sequential reads that would displace useful cached data.

## 36. CACHE BLOCK SIZE

Evaluate:

- 4 KB.
- 16 KB.
- 32 KB.
- 64 KB.
- 128 KB.

Choose a validated default.

Advanced users may modify supported options.

Do not expose block-size complexity during initial setup.

## 37. PERSISTENCE

Allow cache persistence across restarts when safe.

Implement:

- Cache version identifiers.
- Source-volume identity.
- SSD identity.
- Entry validity metadata.
- Metadata checksums.
- Recovery markers.
- Consistent metadata updates.
- Safe invalidation on uncertainty.

Never reuse a stale cache simply because metadata still exists.

## 38. CACHE RESIZING

The user must be able to change cache capacity.

Increasing allocation:

- Check available storage.
- Expand backing storage.
- Update metadata.
- Continue safely.

Decreasing allocation:

- Coordinate active operations.
- Evict excess entries.
- Reduce backing allocation safely.
- Return freed storage.

Pause/reconfigure when online resizing cannot be guaranteed safe.

Do not corrupt or delete unrelated SSD files.

## 39. FAILURE SAFETY

On cache SSD error:

- Reject invalid cached data.
- Fall back to HDD when safe.
- Disable affected caching when necessary.

On metadata corruption:

- Invalidate uncertain entries.
- Recover or rebuild safely.

On system restart:

- Validate cache metadata.
- Validate device identities.
- Restore only safe configurations.

On service failure:

- The driver must follow documented fail-safe behavior.

On SSD removal:

- Avoid additional operations to inaccessible storage.
- Preserve authoritative HDD contents.
- Restore direct I/O where safe.

---

# PART 8 — STORAGE AND DOWNLOAD MANAGEMENT

## 40. CACHE ALLOCATION MANAGEMENT

Allow the user to allocate SSD space using:

- Automatic recommendation.
- Custom GB input.
- Slider.
- Drive selection.

Example:

Source HDD: 4 TB  
Cache SSD: 1 TB NVMe  
SSD Free Space: 420 GB  
Cache Allocation: 180 GB

These numbers are examples, not defaults.

Enforce real allocation quotas.

Maintain appropriate free-space safety reserves.

## 41. SEPARATE DOWNLOAD STAGING

Allow the user to reserve SSD space separately for managed downloads.

Example:

HDD Cache: 180 GB  
Download Staging: 50 GB

Do not combine allocations invisibly.

The user should see actual space usage in the Storage Overview screen.

## 42. MANAGED DOWNLOAD ENGINE

Implement:

- URL-based downloads.
- HTTP range resume when supported.
- Genuine download speed.
- Pause/resume.
- Transfer to HDD.
- File integrity verification.
- Recovery from interrupted operations.
- Retention policies.
- Quota enforcement.

Do not imply SSD staging automatically accelerates network bandwidth.

## 43. GAMING OPTIMIZATION

Implement actual supported cache-policy adjustments.

Possible strategies:

- Increase cache retention of reused game blocks.
- Reduce one-time sequential cache pollution.
- Controlled prefetch.
- Persistent valid cache retention.

Avoid unvalidated assumptions about game behavior.

Game-specific configuration should only exist when supported by real engine policies.

---

# PART 9 — WINDOWS SERVICE AND BACKEND

## 44. BACKGROUND SERVICE

Create:

`SuperHDDService`

Recommended language:

Rust.

Responsibilities:

- Discover physical storage.
- Manage cache configurations.
- Communicate with the driver.
- Manage SSD quotas.
- Collect telemetry.
- Handle recovery.
- Coordinate download staging.
- Monitor device availability.
- Persist user preferences.

The UI must not need to remain open for caching to work.

## 45. BACKEND API CONTRACTS

Implement typed internal commands.

Examples:

- `ListPhysicalDisks`
- `ListVolumes`
- `GetEligibleCacheConfigurations`
- `CreateCache`
- `EnableCache`
- `PauseCache`
- `ResumeCache`
- `ResizeCache`
- `ClearCache`
- `DeleteCache`
- `GetCacheStatistics`
- `GetCacheHealth`
- `GetDriverStatus`
- `GetDownloads`
- `CreateDownload`
- `PauseDownload`
- `ResumeDownload`
- `GetStorageOverview`

All operations must validate user permissions, device identities, and inputs.

Do not expose insecure administrative IPC.

## 46. FRONTEND INTEGRATION

Every control must correspond to an actual supported backend operation.

Use a strongly typed frontend service layer.

Handle:

- Success.
- Failure.
- Loading.
- Unsupported operations.
- Permission errors.
- Disconnected devices.
- Cache busy conditions.
- Driver unavailable.
- Configuration conflicts.

Do not display fake success messages.

---

# PART 10 — PERFORMANCE AND RESOURCE USAGE

## 47. PERFORMANCE METRICS

Measure actual:

- Cache Hit Rate.
- Cache Hits.
- Cache Misses.
- Cache Occupancy.
- SSD Cache Read Throughput.
- HDD Read Throughput.
- Cache Fill Writes.
- Cache Evictions.
- Driver Overhead.
- Source HDD I/O Avoided.

Use genuine driver/service telemetry.

Do not confuse SSD cache throughput with total SSD throughput.

Do not count ordinary operating-system memory-cache hits as SSD cache hits.

## 48. BENCHMARKING

Implement controlled comparison testing.

Include:

- Baseline HDD.
- Cold cache.
- Warm cache.
- Random read tests.
- Sequential read tests.
- Real file workloads.
- Representative game loading.
- Source HDD I/O verification.

Use suitable tools such as DiskSpd.

Record test conditions and repeatability.

No invented performance multipliers.

## 49. RESOURCE EFFICIENCY

The application should be lightweight.

Targets:

- Low idle CPU usage.
- Low service memory usage.
- Efficient driver metadata.
- Minimal unnecessary SSD writes.
- Bounded background activity.
- Responsive UI.
- Smooth animations.
- No continuous full-disk scanning.

Measure regressions as well as improvements.

---

# PART 11 — WINDOWS INSTALLATION AND SECURITY

## 50. INSTALLER

Build a real Windows installer.

Include:

- SuperHDD.exe.
- SuperHDDService.
- SuperHDDCacheDriver package.
- Required application resources.
- Localization resources.
- Installation configuration.
- Uninstaller.

The installer should:

1. Validate supported Windows version.
2. Validate architecture.
3. Check driver compatibility.
4. Explain required permissions.
5. Install the service.
6. Install the signed driver where supported.
7. Initialize the application.
8. Launch setup.

Follow Windows driver-signing requirements.

Do not bypass Secure Boot or signature enforcement.

## 51. SECURITY

Implement:

- Least privilege.
- Authenticated IPC.
- Proper ACLs.
- Validated driver IOCTL requests.
- Bounds checking.
- Safe kernel memory management.
- Secure installation.
- Secure update procedures.
- User data protection.

Support Windows security features where validated.

Never require disabling standard security protections as a default installation method.

## 52. UNINSTALLATION

Users must be able to uninstall safely.

Uninstallation should:

- Deactivate caching.
- Preserve HDD data.
- Handle active cache tasks.
- Remove application-managed cache storage safely.
- Remove service/driver components when appropriate.
- Preserve ordinary user files.
- Explain any reboot requirements.

---

# PART 12 — LOCALIZATION AND ACCESSIBILITY

## 53. LANGUAGE SUPPORT

Support:

- English.
- Arabic.

Arabic must be a first-class UI language.

Provide full RTL mirroring.

Support correct mixed-direction rendering for:

- File paths.
- Drive letters.
- Hardware models.
- Units.
- Numbers.

Use translation files instead of hardcoded interface strings.

## 54. ACCESSIBILITY

Requirements:

- Keyboard navigation.
- Visible focus indicators.
- Accessible text contrast.
- Screen reader labels.
- Tooltips for icon-only controls.
- Reduced-motion preference.
- Appropriate Windows scaling.
- Readable charts.
- Clear error messages.

Do not sacrifice usability for glass effects.

Decorative background layers must never obscure text.

---

# PART 13 — REPOSITORY AND CODE STRUCTURE

## 55. PROJECT STRUCTURE

Create a modular repository.

`super-hdd/`

Desktop application:

`apps/desktop/`

Windows service:

`services/superhdd-service/`

Kernel driver:

`drivers/superhdd-cache-driver/`

Reusable Rust modules:

`crates/storage-discovery/`

`crates/cache-control/`

`crates/cache-metadata/`

`crates/workspace-manager/`

`crates/download-manager/`

`crates/optimization-engine/`

`crates/telemetry/`

`crates/windows-ipc/`

Frontend shared packages:

`packages/design-system/`

`packages/shared-types/`

`packages/localization/`

Installer:

`installer/`

Testing:

`tests/unit/`

`tests/integration/`

`tests/driver/`

`tests/recovery/`

`tests/performance/`

Documentation:

`docs/architecture/`

`docs/design-system/`

`docs/cache-engine/`

`docs/security/`

`docs/benchmarks/`

`docs/release/`

Adapt the structure when necessary without reducing architectural separation.

---

# PART 14 — IMPLEMENTATION PLAN

## 56. DEVELOPMENT TRACKS

Development must proceed through coordinated tracks.

### Track A — Real Acceleration Engine

Highest technical priority.

Begin immediately:

1. Create WDK project.
2. Validate interception architecture.
3. Implement safe I/O pass-through.
4. Implement SSD cache backing access.
5. Implement cache lookup.
6. Implement read hits and misses.
7. Implement cache population.
8. Implement write coherency.
9. Implement eviction.
10. Implement genuine statistics.

### Track B — Windows Service

Develop concurrently:

1. Service scaffold.
2. Disk discovery.
3. Cache allocation.
4. Driver communication.
5. Configuration persistence.
6. Telemetry.
7. Error handling.
8. Recovery.
9. Resizing.
10. Download staging.

### Track C — Premium Interface

Develop concurrently:

1. Design tokens.
2. Glass material components.
3. Window shell.
4. Sidebar.
5. Welcome screen.
6. Acceleration setup.
7. Dashboard.
8. Performance.
9. Downloads.
10. Gaming.
11. Storage.
12. Settings.
13. RTL localization.

### Track D — Quality and Testing

Begin early:

1. Unit tests.
2. Disposable-disk tests.
3. Driver Verifier.
4. Integration tests.
5. Crash recovery.
6. Performance measurements.
7. Security checks.
8. UI tests.

---

# PART 15 — FIRST MANDATORY TECHNICAL PROOF

## 57. VERIFIED SSD CACHE HIT

The first critical proof of functionality must demonstrate:

1. Create a disposable HDD test volume.
2. Create a compatible SSD cache backing store.
3. Enable Super HDD caching.
4. Read a test block from HDD.
5. Verify cache miss.
6. Verify data admission to SSD.
7. Read the same block again under controlled conditions.
8. Verify an SSD cache hit.
9. Verify the HDD did not perform the corresponding physical read.
10. Verify returned data is byte-for-byte correct.
11. Modify source data.
12. Verify stale cache data is not returned.
13. Disable cache.
14. Verify ordinary source-volume access remains correct.

Do not declare this milestone complete based only on successful compilation.

Execute the test and collect evidence.

Never test unverified kernel storage code on irreplaceable user data.

---

# PART 16 — UI IMPLEMENTATION QUALITY

## 58. REPRODUCE THE NEW DESIGNS FAITHFULLY

The new Super HDD visual references are not loose inspiration.

They define the official design direction.

Maintain:

- Blue/pink atmospheric backgrounds.
- Frosted translucent cards.
- Slate-blue buttons.
- Powder-blue controls.
- Soft lavender lighting.
- Silver-blue 3D hardware illustrations.
- Spacious sidebar navigation.
- Large readable headings.
- Balanced cards.
- Soft rounded corners.
- Thin white borders.
- Smooth polished interactions.

Do not randomly replace these with conventional Windows controls that visually clash.

Use native Windows functionality behind custom-designed controls when practical.

## 59. CONSISTENCY RULES

All screens must share:

- One sidebar design.
- One application shell.
- One typography system.
- One color system.
- One glass material system.
- One icon system.
- One spacing scale.
- One button system.
- One input system.
- One animation language.

Do not style each page independently.

A new screen should immediately look like part of Super HDD.

## 60. RESPONSIVE DESKTOP DESIGN

Ensure that:

- Cards resize correctly.
- Charts remain readable.
- Sidebar never overlaps content.
- Buttons remain reachable.
- Long drive names truncate gracefully.
- Text does not overflow.
- RTL layouts remain aligned.
- Window scaling does not break the interface.

Use responsive desktop grids.

Avoid implementing the application as a single static screenshot.

## 61. PRODUCTION DATA VS DEMO DATA

During UI development, clearly distinguish sample data from actual system data.

Do not display invented:

- HDD model names.
- SSD models.
- Cache hit percentages.
- Download speeds.
- Disk throughput.
- Storage usage.
- Driver success messages.

in production as though they were measured.

Provide a clearly labeled Demo Mode for design review if necessary.

Real usage must be backed by real APIs and driver telemetry.

---

# PART 17 — TESTING AND ACCEPTANCE CRITERIA

## 62. FUNCTIONAL ACCEPTANCE

The application is not complete unless:

- Real SSD caching works.
- Eligible repeated HDD reads can be served from SSD.
- Cache size is genuinely enforced.
- Users can select source HDD.
- Users can select cache SSD.
- Cache hits are accurately measured.
- Write coherency works.
- Cache invalidation works.
- Pause/resume works.
- Cache deletion is safe.
- Background service works.
- UI controls connect to real functionality.

## 63. VISUAL ACCEPTANCE

The application must:

- Match the supplied blue/pink Liquid Glass designs.
- Use the exact primary color palette.
- Maintain consistent card styling.
- Include polished 3D illustrations where specified.
- Use coherent typography.
- Provide fluid interactions.
- Support Windows resizing and scaling.
- Support Arabic RTL.
- Maintain professional spacing.
- Avoid visual clutter.
- Maintain readable contrast.

Do not consider generic Tailwind components with a blue background sufficient.

## 64. RELIABILITY ACCEPTANCE

Test:

- Cold and warm cache reads.
- Concurrent reads/writes.
- Partial block operations.
- Cache full conditions.
- Cache resizing.
- Corrupted metadata.
- Cache SSD failure.
- Source HDD failure.
- Restart.
- Service termination.
- Driver failures.
- Sleep/resume.
- Volume lifecycle changes.
- Installation/uninstallation.

All tests must report actual results.

## 65. PERFORMANCE ACCEPTANCE

Demonstrate measurable improvement under at least one valid storage-bound repeated-read workload.

Also document workloads where caching does not help.

Report:

- Original HDD throughput/latency.
- Cache-enabled throughput/latency.
- Cache hit rate.
- SSD cache I/O.
- HDD I/O.
- Driver overhead.
- Test environment.

Do not promise arbitrary speed multipliers.

---

# PART 18 — DELIVERY REQUIREMENTS

## 66. REQUIRED OUTPUT

Deliver a genuine working project including:

1. Full source repository.
2. Tauri desktop application.
3. React UI.
4. Complete pastel Liquid Glass design system.
5. Functional Windows background service.
6. Actual Windows SSD caching driver.
7. Storage discovery.
8. Cache allocation.
9. Cache resizing.
10. Cache telemetry.
11. Download staging.
12. Gaming optimization policies.
13. Settings and localization.
14. Automated tests.
15. Performance tests.
16. Windows installer project.
17. Security documentation.
18. Architecture documentation.
19. Build instructions.
20. Release validation instructions.

## 67. IMPLEMENTATION HONESTY

Use explicit progress categories:

- Implemented.
- Builds successfully.
- Runtime tested.
- Verified on hardware.
- Requires validation.
- Unsupported.

Do not describe source code as verified production functionality without testing.

If the development environment lacks Windows WDK or suitable hardware, still create the genuine source code and test infrastructure, clearly identify the blocker, and continue the work that can actually be performed.

Do not replace the driver with a fake service.

Do not bypass Windows security requirements.

## 68. CODING AGENT EXECUTION INSTRUCTIONS

You are building a real software product, not preparing a sales presentation.

Do not stop after producing a plan.

Start by:

1. Inspecting the development environment.
2. Creating the project repository.
3. Establishing the Windows driver architecture.
4. Creating the WDK project.
5. Implementing the first safe driver I/O path.
6. Creating the Windows service.
7. Creating the Tauri desktop project.
8. Implementing the exact new design tokens.
9. Building the application shell and acceleration screen.
10. Implementing the real cache-hit/cache-miss path.
11. Integrating actual backend states.
12. Testing SSD-served reads.
13. Continuing development of all required screens.
14. Implementing downloads and other storage management features.
15. Completing coherency, safety, benchmarking, and release preparation.

Keep the repository buildable at appropriate checkpoints.

Document every technical decision that affects reliability.

Fix compilation errors instead of accumulating broken placeholder modules.

Never claim functionality that has not been verified.

---

# PART 19 — FINAL PRODUCT STANDARD

## SUPER HDD — YOUR HDD. SUPERCHARGED.

The final application must feel like a premium Windows product built by an experienced systems-engineering and product-design team.

It must combine:

**Real SSD-backed HDD acceleration**

with:

**One-click simplicity**

and:

**Complete control over cache size**

inside:

**A beautiful Pastel Blue, Lavender, and Soft Pink Liquid Glass interface.**

The visual quality must match the newly supplied designs.

The HDD cache must be genuine.

There must be no dedicated RAM caching.

There must be no fake statistics.

There must be no unnecessary partition formatting.

There must be no complicated setup for ordinary users.

There must be no postponement of the main acceleration engine.

The user should open Super HDD, select the HDD and SSD, set the cache size, and activate real acceleration through one clear action.

The application should remain beautiful, safe, responsive, and understandable during every operation.

### FINAL EXECUTION COMMAND

**Build Super HDD as a real Windows desktop application, using the new blue/pink Liquid Glass designs as the official visual specification, and implement the functional SSD caching engine from the first development workstream.**

Do not treat the interface as an isolated prototype.

Do not treat acceleration as a future optional feature.

Do not replace kernel-level work with superficial optimizations.

Build the genuine integrated product, verify its behavior, and clearly identify any remaining signing, hardware-testing, or release-certification requirements.

**Start implementation now.**
