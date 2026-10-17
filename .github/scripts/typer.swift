// Types stdin line by line the way libnut and sendkeys post keys: typer <session|hid> <hold ms> <gap ms>.
import CoreGraphics
import Foundation

let tap: CGEventTapLocation = CommandLine.arguments[1] == "hid" ? .cghidEventTap : .cgSessionEventTap
let source = CommandLine.arguments[1] == "hid" ? CGEventSource(stateID: .hidSystemState) : nil
let hold = useconds_t(CommandLine.arguments[2])! * 1000
let gap = useconds_t(CommandLine.arguments[3])! * 1000

func press(_ key: CGKeyCode, _ character: UInt16?) {
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down)!
        if var c = character { event.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c) }
        event.post(tap: tap)
        if down && hold > 0 { usleep(hold) }
    }
    usleep(gap)
}

while let line = readLine() {
    for unit in line.utf16 { press(0, unit) }
    press(36, nil)
}
