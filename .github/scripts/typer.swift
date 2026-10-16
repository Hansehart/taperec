// Types its argument through the HID event tap, one Unicode character per key press, then presses Return.
import CoreGraphics
import Foundation

let text = CommandLine.arguments[1]
let source = CGEventSource(stateID: .hidSystemState)

func press(_ key: CGKeyCode, _ character: UInt16?) {
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down)!
        if var c = character { event.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c) }
        event.post(tap: .cghidEventTap)
        usleep(down ? 15_000 : 35_000)
    }
}

for unit in text.utf16 { press(0, unit) }
press(36, nil)
