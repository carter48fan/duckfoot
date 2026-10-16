# Keyboard shortcuts

Press `?` in the app for the same list, or **Help** for the guide and a tour.

<!-- keymap:start (generated from crates/photo-app/src/keymap.rs) -->
| | |
|---|---|
| **Photos** | |
| `←` / `→` | previous / next photo, in the filmstrip's order |
| `Ctrl+S` | save now (edits also save on their own) |
| `Ctrl+E` | export this photo: size, format, metadata, then where |
| `Ctrl+Shift+E` | batch export the selection |
| **Editing** | |
| `Alt+↑` / `Alt+↓`, `Alt+←` / `Alt+→` | choose a value of the card last worked on, then turn it (as a right-drag on the photo does) |
| `Ctrl+Z` / `Ctrl+Shift+Z` | undo / redo, per photo |
| `Ctrl+C` / `Ctrl+V` | copy / paste the adjustments (crop and rotation stay put) |
| `Ctrl+Shift+V` | choose what paste carries |
| `Ctrl+'` | virtual copy: another edit of the same photo |
| `Ctrl+Alt+V` | the same edits as the photo open before this one |
| `Esc` with a mask chosen or healing | put the mask's handles away / stop healing |
| `C` | crop tool on / off |
| `Enter` / `Esc` | apply / cancel the crop |
| `X` / `O` while cropping | turn the crop's shape / next guide |
| `Shift` while resizing a crop | keep its shape |
| `[` / `]` | rotate 90° left / right |
| `H` / `V` | mirror / flip |
| `Esc` while dragging | put the control back where the drag started |
| `W`, then click | white balance from something that should be grey |
| **View** | |
| `\` | before / after split |
| `S` | soft proof: the preview in the export's colour space |
| `Ctrl+0` / `Ctrl+1` / `Z` | fit / 100% / toggle between them |
| `F` / `G` | show or hide the shelf / show it as a grid (desktop) |
| `?` or `F1` | this list of shortcuts |
| `Ctrl+,` | preferences (desktop) |
| **Library** | |
| `0`–`5` | star rating for the selection (`0` clears) |
| `6` / `7` / `8` / `9` | red / yellow / green / blue label (again takes it off) |
| `P` / `X` / `U` | flag as pick / reject / clear the flag |
| `Ctrl+A` | select every photo on the shelf |
| `Shift`+click / `Ctrl`+click | select a range / add to the selection |
| **Mouse and touch** | |
| Scroll or pinch over the photo | zoom about the pointer |
| Drag the photo | pan, once zoomed in |
| Drag a slider | live; one undo step per gesture |
| `Shift` while dragging a slider | ten times finer |
| Double-click a slider or its label | back to its default |
| Drag in the curve | move the nearest point; the ends keep their place |
| Drag a 3D view | orbit it; `Ctrl`+scroll zooms, double-click resets |
| Click a module's header, on the All tab | open it and close the others; `Shift`+click opens it alongside |
| Touch a control in a module that's off | turns the module on |
| Click a History row | jump to that step |
| Drag the split divider | move it; double-click centres it |
| Hold the scope's clip readout | show clipping while held |
| Drop files on the window | add them to the shelf and open the first |
<!-- keymap:end -->

Press `?` in the app for the same list, or **Help** for the guide and a tour. On a Mac, `Ctrl` is `⌘`.

Hotkeys deliberately do not fire while a text field has focus.
