// Mouse and keyboard through CGEvent, posted where hardware events enter the system.
#if os(macOS)
import AppKit
import Carbon.HIToolbox
import CoreGraphics
import IOKit.hidsystem

/// How long a key stays down; holding it loses fewer presses than releasing at once.
let keyHold: useconds_t = 15_000
/// The pause after a release, before the next event.
let keyGap: useconds_t = 5_000

let eventSource = CGEventSource(stateID: .hidSystemState)

func post(_ event: CGEvent?) {
    event?.post(tap: .cghidEventTap)
}

func pointerLocation() -> CGPoint {
    CGEvent(source: nil)?.location ?? .zero
}

func moveMouse(to point: CGPoint) {
    post(CGEvent(mouseEventSource: eventSource, mouseType: .mouseMoved,
                 mouseCursorPosition: point, mouseButton: .left))
}

func clickMouse() {
    let at = pointerLocation()
    post(CGEvent(mouseEventSource: eventSource, mouseType: .leftMouseDown,
                 mouseCursorPosition: at, mouseButton: .left))
    usleep(30_000)
    post(CGEvent(mouseEventSource: eventSource, mouseType: .leftMouseUp,
                 mouseCursorPosition: at, mouseButton: .left))
}

/// One key down then up, with `flags` held and optionally carrying the character it types.
func tapKey(_ code: CGKeyCode, flags: CGEventFlags = [], text: String? = nil) {
    for down in [true, false] {
        guard let event = CGEvent(keyboardEventSource: eventSource, virtualKey: code, keyDown: down) else { continue }
        event.flags = flags
        if let text {
            var units = Array(text.utf16)
            event.keyboardSetUnicodeString(stringLength: units.count, unicodeString: &units)
        }
        post(event)
        usleep(down ? keyHold : keyGap)
    }
}

/// The key code and event flag of each modifier; `primary` is ⌘ on macOS.
func modifierKey(_ modifier: Modifier) -> (code: CGKeyCode, flag: CGEventFlags) {
    switch modifier {
    case .primary, .cmd: return (CGKeyCode(kVK_Command), .maskCommand)
    case .ctrl: return (CGKeyCode(kVK_Control), .maskControl)
    case .alt: return (CGKeyCode(kVK_Option), .maskAlternate)
    case .shift: return (CGKeyCode(kVK_Shift), .maskShift)
    case .fn: return (CGKeyCode(kVK_Function), .maskSecondaryFn)
    }
}

/// Holds `modifiers` the way a keyboard does, runs `body`, then releases them in reverse.
func holding(_ modifiers: [Modifier], _ body: (CGEventFlags) -> Void) {
    var flags: CGEventFlags = []
    for modifier in modifiers {
        let (code, flag) = modifierKey(modifier)
        flags.insert(flag)
        let event = CGEvent(keyboardEventSource: eventSource, virtualKey: code, keyDown: true)
        event?.flags = flags
        post(event)
    }
    body(flags)
    for modifier in modifiers.reversed() {
        let (code, flag) = modifierKey(modifier)
        flags.remove(flag)
        let event = CGEvent(keyboardEventSource: eventSource, virtualKey: code, keyDown: false)
        event?.flags = flags
        post(event)
    }
}

/// Named keys by virtual key code (Carbon's `kVK_*`), as cliclick's `kp:` names them.
let namedKeyCodes: [String: Int] = [
    "return": kVK_Return, "enter": kVK_ANSI_KeypadEnter, "esc": kVK_Escape, "tab": kVK_Tab,
    "space": kVK_Space, "delete": kVK_Delete, "fwd-delete": kVK_ForwardDelete,
    "f1": kVK_F1, "f2": kVK_F2, "f3": kVK_F3, "f4": kVK_F4, "f5": kVK_F5, "f6": kVK_F6,
    "f7": kVK_F7, "f8": kVK_F8, "f9": kVK_F9, "f10": kVK_F10, "f11": kVK_F11, "f12": kVK_F12,
    "f13": kVK_F13, "f14": kVK_F14, "f15": kVK_F15, "f16": kVK_F16,
    "arrow-up": kVK_UpArrow, "arrow-down": kVK_DownArrow, "arrow-left": kVK_LeftArrow,
    "arrow-right": kVK_RightArrow, "home": kVK_Home, "end": kVK_End,
    "page-up": kVK_PageUp, "page-down": kVK_PageDown,
    "num-0": kVK_ANSI_Keypad0, "num-1": kVK_ANSI_Keypad1, "num-2": kVK_ANSI_Keypad2,
    "num-3": kVK_ANSI_Keypad3, "num-4": kVK_ANSI_Keypad4, "num-5": kVK_ANSI_Keypad5,
    "num-6": kVK_ANSI_Keypad6, "num-7": kVK_ANSI_Keypad7, "num-8": kVK_ANSI_Keypad8,
    "num-9": kVK_ANSI_Keypad9, "num-clear": kVK_ANSI_KeypadClear, "num-equals": kVK_ANSI_KeypadEquals,
    "num-divide": kVK_ANSI_KeypadDivide, "num-multiply": kVK_ANSI_KeypadMultiply,
    "num-minus": kVK_ANSI_KeypadMinus, "num-plus": kVK_ANSI_KeypadPlus, "num-enter": kVK_ANSI_KeypadEnter,
]

/// Media and brightness keys, sent as system-defined events (IOKit's `NX_KEYTYPE_*`).
let systemKeyCodes: [String: Int32] = [
    "mute": NX_KEYTYPE_MUTE, "volume-up": NX_KEYTYPE_SOUND_UP, "volume-down": NX_KEYTYPE_SOUND_DOWN,
    "brightness-up": NX_KEYTYPE_BRIGHTNESS_UP, "brightness-down": NX_KEYTYPE_BRIGHTNESS_DOWN,
    "play-pause": NX_KEYTYPE_PLAY, "play-previous": NX_KEYTYPE_PREVIOUS, "play-next": NX_KEYTYPE_NEXT,
    "keys-light-toggle": NX_KEYTYPE_ILLUMINATION_TOGGLE, "keys-light-up": NX_KEYTYPE_ILLUMINATION_UP,
    "keys-light-down": NX_KEYTYPE_ILLUMINATION_DOWN,
]

func tapSystemKey(_ code: Int32) {
    for (flags, state) in [(0xa00, 0xa), (0xb00, 0xb)] {
        let event = NSEvent.otherEvent(
            with: .systemDefined, location: .zero, modifierFlags: NSEvent.ModifierFlags(rawValue: UInt(flags)),
            timestamp: 0, windowNumber: 0, context: nil, subtype: 8,
            data1: Int((code << 16) | Int32(state << 8)), data2: -1)
        post(event?.cgEvent)
        usleep(keyGap)
    }
}

/// US ANSI key codes for printable characters, and whether they need ⇧.
let characterKeys: [Character: (code: Int, shift: Bool)] = {
    let plain: [Character: Int] = [
        "a": kVK_ANSI_A, "b": kVK_ANSI_B, "c": kVK_ANSI_C, "d": kVK_ANSI_D, "e": kVK_ANSI_E,
        "f": kVK_ANSI_F, "g": kVK_ANSI_G, "h": kVK_ANSI_H, "i": kVK_ANSI_I, "j": kVK_ANSI_J,
        "k": kVK_ANSI_K, "l": kVK_ANSI_L, "m": kVK_ANSI_M, "n": kVK_ANSI_N, "o": kVK_ANSI_O,
        "p": kVK_ANSI_P, "q": kVK_ANSI_Q, "r": kVK_ANSI_R, "s": kVK_ANSI_S, "t": kVK_ANSI_T,
        "u": kVK_ANSI_U, "v": kVK_ANSI_V, "w": kVK_ANSI_W, "x": kVK_ANSI_X, "y": kVK_ANSI_Y,
        "z": kVK_ANSI_Z, "0": kVK_ANSI_0, "1": kVK_ANSI_1, "2": kVK_ANSI_2, "3": kVK_ANSI_3,
        "4": kVK_ANSI_4, "5": kVK_ANSI_5, "6": kVK_ANSI_6, "7": kVK_ANSI_7, "8": kVK_ANSI_8,
        "9": kVK_ANSI_9, "-": kVK_ANSI_Minus, "=": kVK_ANSI_Equal, "[": kVK_ANSI_LeftBracket,
        "]": kVK_ANSI_RightBracket, "\\": kVK_ANSI_Backslash, ";": kVK_ANSI_Semicolon,
        "'": kVK_ANSI_Quote, ",": kVK_ANSI_Comma, ".": kVK_ANSI_Period, "/": kVK_ANSI_Slash,
        "`": kVK_ANSI_Grave, " ": kVK_Space,
    ]
    let shifted: [Character: Character] = [
        "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9",
        ")": "0", "_": "-", "+": "=", "{": "[", "}": "]", "|": "\\", ":": ";", "\"": "'", "<": ",",
        ">": ".", "?": "/", "~": "`",
    ]
    var keys: [Character: (code: Int, shift: Bool)] = [:]
    for (c, code) in plain { keys[c] = (code, false) }
    for (c, base) in shifted { keys[c] = (plain[base] ?? 0, true) }
    for c in "abcdefghijklmnopqrstuvwxyz" { keys[Character(c.uppercased())] = (plain[c] ?? 0, true) }
    return keys
}()

/// Presses a chord: its modifiers held, then the named key or character.
func pressChord(_ chord: Chord) {
    switch chord.key {
    case .named(let name):
        if let code = systemKeyCodes[name] {
            holding(chord.modifiers) { _ in tapSystemKey(code) }
        } else if let code = namedKeyCodes[name] {
            holding(chord.modifiers) { flags in tapKey(CGKeyCode(code), flags: flags) }
        }
    case .character(let c):
        let key = characterKeys[c] ?? (code: 0, shift: false)
        let modifiers = key.shift && !chord.modifiers.contains(.shift) ? chord.modifiers + [.shift] : chord.modifiers
        holding(modifiers) { flags in tapKey(CGKeyCode(key.code), flags: flags) }
    }
}

/// Types one character; Return and Tab as their keys, anything else as Unicode text.
func typeCharacter(_ c: Character) -> Bool {
    switch c {
    case "\n": tapKey(CGKeyCode(kVK_Return))
    case "\t": tapKey(CGKeyCode(kVK_Tab))
    default:
        guard !(c.unicodeScalars.contains { $0.properties.generalCategory == .control }) else { return false }
        let code = characterKeys[c]?.code ?? 0
        tapKey(CGKeyCode(code), text: String(c))
    }
    return true
}
#endif
