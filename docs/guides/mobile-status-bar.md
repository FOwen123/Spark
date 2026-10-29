# Mobile status bar

Mobile screens look finished when they carry a believable system status bar. Build it as its own frame at the top of the artboard.

## Dimensions

- iOS (iPhone with a notch or island): 54 px tall at 1x on a 393 px wide artboard (iPhone 15/16). Time sits on the left, centred in the left ear; signal, Wi-Fi and battery sit on the right ear.
- Android: 24–32 px tall depending on density; time on the left, icons on the right.
- Keep 16–24 px horizontal padding and vertically centre the contents.

## Structure

```html
<div layer-name="Status bar" style="display:flex; align-items:center; justify-content:space-between; height:54px; padding:0 32px; width:100%">
  <span style="font: 600 17px/22px Geist, system-ui; letter-spacing:-0.02em">9:41</span>
  <div layer-name="Indicators" style="display:flex; gap:6px; align-items:center">
    <!-- signal, wifi and battery as small inline SVGs -->
  </div>
</div>
```

## Tips

- Show 9:41 for the time, full signal and Wi-Fi, and a full battery, so the screen looks calm.
- Match the bar's text color to the background: black on light screens, white on dark or photographic headers.
- Draw indicator icons as simple SVG shapes (bars, arcs, a rounded rectangle for the battery) instead of emoji or text.
- The home indicator on iOS is a 134 × 5 px rounded bar, 8 px above the bottom edge.
