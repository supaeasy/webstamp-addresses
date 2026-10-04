# webstamp-addresses

🇩🇪 [Deutsch](#deutsch) · 🇬🇧 [English](#english)

<a id="deutsch"></a>
## Deutsch

Kleine Anwendung in Rust (Windows, macOS, Linux), die **Webstamps (e-Stamps) der Schweizer Post** mit Absender- und Empfängeradresse versieht und den Umschlag direkt druckt. Als Standardformat wird der **C5-Umschlag** (229 × 162 mm, quer) verwendet – das entspricht dem Format der von der Post erzeugten Stempel-PDF.

Die Anwendung nimmt die heruntergeladene Webstamp-PDF als Vorlage, lässt den Stempel (inkl. Datamatrix-Code) unverändert und ergänzt nur die Adressen.

### Funktionen

- **Webstamp-PDF als Vorlage:** per Drag & Drop, über „Stempel-PDF öffnen…“ oder als Startargument. Die zuletzt benutzte Vorlage wird beim nächsten Start wieder geladen.
- **Empfänger- und Absenderadresse** (mehrzeilig), Schriftgröße je Block einstellbar.
- **Standard-Absender:** einmal speichern („Als Standard speichern“), danach immer vorbelegt. Der Absender lässt sich pro Umschlag abschalten.
- **Frei verschiebbare Adressblöcke:** per Maus in der Vorschau ziehen oder die Position in mm eingeben. Die Positionen werden gespeichert.
- **Bild oder Vektorgrafik einfügen** (z. B. Logo; PNG, JPG, BMP, GIF und **SVG**, auch mit Transparenz; SVGs werden in der jeweiligen Druckauflösung gerendert und bleiben scharf): frei verschiebbar wie die Adressblöcke, Größe über den Eckgriff oder die Breite in mm (Seitenverhältnis bleibt). Wird mitgedruckt und ins PDF übernommen.
- **Zonen der Schweizer Post, getrennt für Inland und Ausland** (Selektor über der Vorschau; nur Vorschau, hellblau): Frankierzone, Absenderzone, Codierzone und Lese- bzw. Adressfeld nach den [Spezifikationen Briefgestaltung](https://www.post.ch/briefgestaltung) der Post (Format bis B5, quer), damit Adressen maschinenlesbar platziert werden.
- **Hilfslinien:** eine horizontale und eine vertikale Linie, frei verschiebbar (nur Vorschau, werden nicht gedruckt).
- **Direktdruck** über den Windows-Druckdialog (Drucker und Papierformat C5 wählbar). Der Umschlag wird bei Hochformat-Einzug automatisch gedreht; zusätzlich „180° drehen“ und ein Feinversatz in mm für die Druckerjustage.
- **Als PDF speichern** zur Kontrolle vor dem Druck. Die gewählten Schriften werden als Teilschrift eingebettet (Text bleibt durchsuchbar); fehlt eine Schrift, wird Helvetica verwendet und in der Statusleiste gemeldet.
- **Schrift, Ausrichtung und Breite pro Adressfeld:** Empfänger und Absender haben je eigene Schriftart (Auswahlfeld mit Autovervollständigung über alle installierten Schriften), Größe, Textbreite sowie Ausrichtung *Links*, *Blocksatz* oder *Rechts*.
- **Fettdruck:** Text markieren und Strg+B (oder Button **B**) drücken – fette Teile werden im Text als `**fett**` markiert und gedruckt.
- **Hell-/Dunkelmodus:** folgt automatisch dem Windows-Design.
- Einstellungen werden automatisch gesichert unter `%APPDATA%\webstamp-addresses\config.json`.

### Bedienung

1. Webstamp-PDF in das Fenster ziehen.
2. Empfänger eintragen, ggf. Absender anpassen und als Standard speichern.
3. Adressblöcke in der Vorschau an die gewünschte Stelle ziehen.
4. „Drucken…“ → im Druckdialog unter „Eigenschaften“ Papierformat **C5** wählen.

> Ein Webstamp ist ein Wertzeichen und nur einmal gültig. Zuerst auf Normalpapier oder mit „Microsoft Print to PDF“ testen und den Stempel erst für den echten Druck verwenden.

### Download

Fertige Programme (Windows x64, Linux x64, macOS universal) gibt es unter [Releases](../../releases). Ein Release entsteht automatisch, sobald ein Tag `vX.Y.Z` gepusht wird.

### Plattformen

| | Windows | macOS / Linux |
|---|---|---|
| Vorschau, Adressen, Bild, Zonen | ✔ | ✔ |
| Als PDF speichern | ✔ | ✔ |
| Drucken | direkt über den Windows-Druckdialog (GDI, gewählte Schrift) | PDF (mit eingebetteter gewählter Schrift) wird per CUPS (`lp`) an den **Standarddrucker** gesendet |

macOS/Linux sind bisher nur gebaut, aber nicht auf echter Hardware getestet. Unter macOS ist die Datei nicht signiert (ggf. Rechtsklick → Öffnen bzw. `xattr -d com.apple.quarantine webstamp-addresses`).

### Bauen

Voraussetzungen: Rust (stable); Windows: MSVC-Build-Tools; Linux: `libgtk-3-dev libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev`.

```bash
cargo build --release
```

Das Ergebnis liegt unter `target/release/` (eine einzelne Datei, unter Windows `webstamp-addresses.exe`). Die Stempel-PDF wird intern mit [hayro](https://crates.io/crates/hayro) gerendert, die Oberfläche basiert auf [egui/eframe](https://crates.io/crates/eframe).

### Hinweise

- Einstellungen liegen unter `%APPDATA%\webstamp-addresses\config.json` (Windows), `~/Library/Application Support/webstamp-addresses/` (macOS) bzw. `~/.config/webstamp-addresses/` (Linux).
- Dieses Projekt steht in keiner Verbindung zur Schweizerischen Post.

---

<a id="english"></a>
## English

A small application written in Rust (Windows, macOS, Linux) that adds sender and recipient addresses to **Swiss Post Webstamps (e-stamps)** and prints the envelope directly. The default envelope format is **C5** (229 × 162 mm, landscape), which matches the stamp PDF generated by the Post.

The app takes the downloaded Webstamp PDF as a template, leaves the stamp (including its Data Matrix code) untouched, and only adds the addresses.

### Features

- **Webstamp PDF as template:** drag & drop, “Stempel-PDF öffnen…” (open), or pass it as a command-line argument. The last used template is reloaded on startup.
- **Recipient and sender address** (multi-line) with a font size per block.
- **Default sender:** save it once (“Als Standard speichern”) and it is pre-filled from then on. The sender can be switched off per envelope.
- **Freely movable address blocks:** drag them in the preview or enter the position in mm. Positions are persisted.
- **Insert an image or vector graphic** (e.g. a logo; PNG, JPG, BMP, GIF and **SVG**, transparency supported; SVGs are rendered at the actual print resolution and stay sharp): freely movable like the address blocks, resizable via the corner handle or the width in mm (aspect ratio is kept). It is printed and included in the PDF export.
- **Swiss Post zones, separate for domestic and international mail** (selector above the preview; preview only, light blue): franking, sender, coding and reading/address zone according to the Post's [letter design specifications](https://www.post.ch/briefgestaltung) (format up to B5, landscape), so addresses are placed machine-readably.
- **Guide lines:** one horizontal and one vertical line, freely movable (preview only, never printed).
- **Direct printing** through the Windows print dialog (choose printer and paper size C5). The envelope is rotated automatically for portrait paper feeds; there is also a “rotate 180°” option and a fine offset in mm for printer calibration.
- **Save as PDF** to check the result before printing. The selected fonts are embedded as subsets (text stays searchable); if a font is missing, Helvetica is used and reported in the status bar.
- **Font, alignment and width per address field:** recipient and sender each have their own font (search-as-you-type picker over all installed fonts), size, text width and alignment *left*, *justified* or *right*.
- **Bold text:** select text and press Ctrl+B (or the **B** button) – bold parts are marked as `**bold**` in the text and printed bold.
- **Light/dark mode:** follows the Windows theme automatically.
- Settings are saved automatically to `%APPDATA%\webstamp-addresses\config.json`.

### Usage

1. Drop the Webstamp PDF onto the window.
2. Enter the recipient; adjust the sender and save it as the default if needed.
3. Drag the address blocks to the desired position in the preview.
4. “Drucken…” (print) → in the print dialog, choose paper size **C5** under “Properties”.

> A Webstamp is a postage value and valid only once. Test on plain paper or with “Microsoft Print to PDF” first, and only use the stamp for the real print.

### Download

Prebuilt binaries (Windows x64, Linux x64, macOS universal) are available under [Releases](../../releases). A release is created automatically when a tag `vX.Y.Z` is pushed.

### Platforms

| | Windows | macOS / Linux |
|---|---|---|
| Preview, addresses, image, zones | ✔ | ✔ |
| Save as PDF | ✔ | ✔ |
| Printing | directly via the Windows print dialog (GDI, selected font) | the PDF (with the selected font embedded) is sent to the **default printer** via CUPS (`lp`) |

macOS/Linux builds are so far only compiled, not tested on real hardware. On macOS the binary is unsigned (right-click → Open, or `xattr -d com.apple.quarantine webstamp-addresses`).

### Build

Requirements: Rust (stable); Windows: MSVC build tools; Linux: `libgtk-3-dev libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev`.

```bash
cargo build --release
```

The result is in `target/release/` (a single file, `webstamp-addresses.exe` on Windows). The stamp PDF is rendered with [hayro](https://crates.io/crates/hayro); the UI is built on [egui/eframe](https://crates.io/crates/eframe).

### Notes

- Settings are stored in `%APPDATA%\webstamp-addresses\config.json` (Windows), `~/Library/Application Support/webstamp-addresses/` (macOS) or `~/.config/webstamp-addresses/` (Linux).
- This project is not affiliated with Swiss Post.
