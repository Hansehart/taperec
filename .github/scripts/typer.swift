// Types text as Unicode key presses, holding each key for a while: typer <hold ms> <gap ms> <text>.
import CoreGraphics
import Foundation

let hold = useconds_t(CommandLine.arguments[1])! * 1000
let gap = useconds_t(CommandLine.arguments[2])! * 1000
let source = CGEventSource(stateID: .hidSystemState)

func press(_ key: CGKeyCode, _ character: UInt16?) {
    for down in [true, false] {
        let event = CGEvent(keyboardEventSource: source, virtualKey: key, keyDown: down)!
        if var c = character { event.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c) }
        event.post(tap: .cghidEventTap)
        usleep(down ? hold : gap)
    }
}

for unit in CommandLine.arguments[3].utf16 { press(0, unit) }
press(36, nil)
