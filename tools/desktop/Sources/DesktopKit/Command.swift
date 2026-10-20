// The commands taperec sends, parsed from the command line; free of macOS so it tests anywhere.

/// A modifier held during a chord; `primary` is the platform's main shortcut modifier, ⌘ on macOS.
public enum Modifier: String, CaseIterable, Equatable {
    case primary, cmd, ctrl, alt, shift, fn
}

/// The key a chord presses: a named key such as `return`, or one printable character.
public enum ChordKey: Equatable {
    case named(String)
    case character(Character)
}

/// Modifiers plus one key, e.g. `primary+shift+p` or `return`.
public struct Chord: Equatable {
    public let modifiers: [Modifier]
    public let key: ChordKey

    /// Parses `[modifier+]...key` the way taperec's tapes write it.
    public init?(_ text: String) {
        var parts = text.split(separator: "+", omittingEmptySubsequences: false).map(String.init)
        guard let last = parts.popLast(), !last.isEmpty else { return nil }
        var modifiers: [Modifier] = []
        for part in parts {
            guard let modifier = Modifier(rawValue: part), !modifiers.contains(modifier) else { return nil }
            modifiers.append(modifier)
        }
        if namedKeys.contains(last) {
            key = .named(last)
        } else if last.count == 1, let c = last.first, c.isASCII, c != "+", !c.isWhitespace {
            key = .character(c)
        } else {
            return nil
        }
        self.modifiers = modifiers
    }
}

/// The named keys tapes may press, in the order of taperec's `NAMED_KEYS`.
public let namedKeys: [String] = [
    "return", "enter", "esc", "tab", "space", "delete", "fwd-delete",
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12", "f13", "f14", "f15", "f16",
    "arrow-up", "arrow-down", "arrow-left", "arrow-right", "home", "end", "page-up", "page-down",
    "num-0", "num-1", "num-2", "num-3", "num-4", "num-5", "num-6", "num-7", "num-8", "num-9",
    "num-clear", "num-equals", "num-divide", "num-multiply", "num-minus", "num-plus", "num-enter",
    "mute", "volume-up", "volume-down", "brightness-up", "brightness-down",
    "play-pause", "play-previous", "play-next", "keys-light-toggle", "keys-light-up", "keys-light-down",
]

/// One request from taperec.
public enum Command: Equatable {
    case pointer
    case move(x: Int, y: Int)
    case click
    case key(Chord)
    case type(Character)
    case paste(String)
    case windows
    case buttons
    case dock
    case focus(String)
    case record(String)
    case encoder

    /// Parses the arguments after the program name.
    public init?(_ arguments: [String]) {
        switch arguments.first {
        case "pointer" where arguments.count == 1: self = .pointer
        case "move" where arguments.count == 3:
            guard let x = Int(arguments[1]), let y = Int(arguments[2]), x >= 0, y >= 0 else { return nil }
            self = .move(x: x, y: y)
        case "click" where arguments.count == 1: self = .click
        case "key" where arguments.count == 2:
            guard let chord = Chord(arguments[1]) else { return nil }
            self = .key(chord)
        case "type" where arguments.count == 2:
            guard arguments[1].count == 1, let c = arguments[1].first else { return nil }
            self = .type(c)
        case "paste" where arguments.count == 2: self = .paste(arguments[1])
        case "windows" where arguments.count == 1: self = .windows
        case "buttons" where arguments.count == 1: self = .buttons
        case "dock" where arguments.count == 1: self = .dock
        case "focus" where arguments.count == 2 && !arguments[1].isEmpty: self = .focus(arguments[1])
        case "record" where arguments.count == 2 && !arguments[1].isEmpty: self = .record(arguments[1])
        case "encoder" where arguments.count == 1: self = .encoder
        default: return nil
        }
    }
}

/// How the helper ends: taperec reads these codes.
public enum Exit: Int32 {
    case ok = 0
    case failed = 1
    case notTrusted = 2
    case untypable = 3
}

/// The usage line for a malformed command.
public let usage = """
    usage: taperec-desktop pointer | move X Y | click | key CHORD | type CHAR | paste TEXT
                           | windows | buttons | dock | focus APP | record MOVIE | encoder
    """
